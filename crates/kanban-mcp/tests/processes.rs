use kanban_core::{CaptureTask, Database, ReviewStatus, ReviewTask, Status};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio},
    sync::{Arc, Barrier},
    thread,
};

struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn launch(data_dir: &Path) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_agentkanban-mcp"));
        command
            .env("AGENTKANBAN_DATA_DIR", data_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            input: Some(input),
            output,
            next_id: 1,
        };
        let response = client.request("initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Integration test","version":"1"}}));
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
        writeln!(
            client.input.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .unwrap();
        client
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        if self.output.read_line(&mut line).unwrap() == 0 {
            let mut stderr = String::new();
            std::io::Read::read_to_string(self.child.stderr.as_mut().unwrap(), &mut stderr).ok();
            panic!("server closed stdout before replying: {stderr}");
        }
        let response: Value =
            serde_json::from_str(&line).expect("stdout must contain only JSON-RPC");
        assert_eq!(response["id"], id);
        response
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.request("tools/call", json!({"name":name,"arguments":arguments}));
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }

    fn close(mut self) {
        self.input.take();
        let status = self.child.wait().unwrap();
        assert!(status.success());
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.input.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn args(project: &Path, key: &str, status: &str) -> Value {
    json!({"project_path":project,"task_key":key,"title":format!("任务 {key}"),"status":status,"progress":"真实 stdio 进程写入"})
}

fn run_cli(data_dir: &Path, arguments: &[&str], input: Option<&[u8]>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_agentkanban-mcp"));
    command
        .args(arguments)
        .env("AGENTKANBAN_DATA_DIR", data_dir)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child.stdin.take().unwrap().write_all(input).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn cli_json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "CLI must print exactly one JSON value: {error}; stdout={}; stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn cli_call(data_dir: &Path, name: &str, arguments: Value) -> (Output, Value) {
    let bytes = serde_json::to_vec(&arguments).unwrap();
    let output = run_cli(
        data_dir,
        &["--call", name, "--input-file", "-"],
        Some(&bytes),
    );
    let result = cli_json(&output);
    (output, result)
}

#[test]
fn independent_processes_share_data_after_gui_connection_closes() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let path = data_dir.join("agentkanban.sqlite3");
    let gui = Database::open(&path).unwrap();
    assert!(gui.board().unwrap().projects.is_empty());
    drop(gui);
    let mut client = Client::launch(&data_dir);
    let created = client.tool(
        "task_upsert",
        args(root.path(), "offline-gui", "in_progress"),
    );
    assert_eq!(created["isError"], false);
    let id = created["structuredContent"]["id"].clone();
    let mut completion = args(root.path(), "offline-gui", "done");
    completion["expected_updated_at"] = created["structuredContent"]["updated_at"].clone();
    assert_eq!(client.tool("task_upsert", completion)["isError"], false);
    client.close();
    let reopened = Database::open(&path).unwrap();
    let board = reopened.board().unwrap();
    assert_eq!(board.projects[0].tasks.len(), 1);
    assert_eq!(json!(board.projects[0].tasks[0].id), id);
    assert_eq!(board.projects[0].tasks[0].status, Status::Done);
    let mut restarted = Client::launch(&data_dir);
    assert_eq!(
        restarted.tool("task_list", json!({}))["structuredContent"]["items"],
        json!([])
    );
    assert_eq!(
        restarted.tool("task_list", json!({"include_done":true}))["structuredContent"]["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    restarted.close();
}

#[test]
fn four_mcp_processes_concurrently_create_and_update_without_lost_tasks() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("shared-data");
    // Start from a completely absent database to cover concurrent initialization too.
    let barrier = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let barrier = barrier.clone();
            let project = root.path().to_path_buf();
            let data_dir = data_dir.clone();
            thread::spawn(move || {
                barrier.wait();
                let mut client = Client::launch(&data_dir);
                for index in 0..12 {
                    let key = format!("worker-{worker}-{index}");
                    let first = client.tool("task_upsert", args(&project, &key, "todo"));
                    assert_eq!(first["isError"], false, "{first}");
                    let mut update = args(&project, &key, "in_progress");
                    update["expected_updated_at"] =
                        first["structuredContent"]["updated_at"].clone();
                    let updated = client.tool("task_upsert", update);
                    assert_eq!(updated["isError"], false, "{updated}");
                    assert_eq!(
                        first["structuredContent"]["id"],
                        updated["structuredContent"]["id"]
                    );
                }
                client.close();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    let board = db.board().unwrap();
    assert_eq!(board.projects.len(), 1);
    assert_eq!(board.projects[0].tasks.len(), 48);
    assert!(board.projects[0]
        .tasks
        .iter()
        .all(|task| task.status == Status::InProgress));
    assert_eq!(board.revision, 96);
}

#[test]
fn storage_write_errors_reach_the_client_and_server_recovers() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let mut client = Client::launch(&data_dir);
    let path = data_dir.join("agentkanban.sqlite3");
    let connection = Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TRIGGER simulate_failure BEFORE INSERT ON tasks BEGIN SELECT RAISE(ABORT,'simulated write failure'); END;").unwrap();
    let response = client.tool("task_upsert", args(root.path(), "failure", "todo"));
    assert_eq!(response["isError"], true);
    assert!(response["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("simulated write failure"));
    assert_eq!(Database::open(&path).unwrap().revision().unwrap(), 0);
    connection
        .execute_batch("DROP TRIGGER simulate_failure")
        .unwrap();
    assert_eq!(
        client.tool("task_upsert", args(root.path(), "recovery", "todo"))["isError"],
        false
    );
    client.close();
}

#[test]
fn live_mcp_can_take_over_captured_work_and_rediscover_a_human_rejection() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("workflow-data");
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    let capture = db
        .capture(CaptureTask {
            project_path: root.path().to_str().unwrap().into(),
            task_key: "user-stable-uuid".into(),
            title: "制作本地报告".into(),
            request: "报告需要中文标题\n保留原始输入数据".into(),
        })
        .unwrap();
    let mut client = Client::launch(&data_dir);
    let listed = client.tool(
        "task_list",
        json!({"project_path":root.path(),"task_key":"user-stable-uuid"}),
    );
    let task = &listed["structuredContent"]["items"][0];
    assert_eq!(task["id"], capture.id);
    assert_eq!(task["request"], "报告需要中文标题\n保留原始输入数据");
    assert_eq!(task["agent_updated_at"], Value::Null);
    let mut update = args(root.path(), "user-stable-uuid", "done");
    update["expected_updated_at"] = task["updated_at"].clone();
    update["agent"] = json!("MCP process test");
    update["deliverables"] = json!([{"label":"报告","uri":"report.html"}]);
    let done = client.tool("task_upsert", update.clone());
    assert_eq!(done["isError"], false);
    let done_stamp = done["structuredContent"]["updated_at"]
        .as_str()
        .unwrap()
        .to_string();
    let rejected = db
        .review(ReviewTask {
            id: capture.id,
            expected_updated_at: done_stamp.clone(),
            accepted: false,
            note: "请补坐标单位".into(),
        })
        .unwrap();
    update["expected_updated_at"] = json!(done_stamp);
    let stale = client.tool("task_upsert", update.clone());
    assert_eq!(stale["isError"], true);
    assert!(stale["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    let listed = client.tool("task_list", json!({"task_key":"user-stable-uuid"}));
    let task = &listed["structuredContent"]["items"][0];
    assert_eq!(task["id"], capture.id);
    assert_eq!(task["status"], "todo");
    assert_eq!(task["review_status"], "changes_requested");
    assert_eq!(task["user_note"], "请补坐标单位");
    assert_eq!(task["updated_at"], rejected.updated_at);
    update["expected_updated_at"] = task["updated_at"].clone();
    update["progress"] = json!("已补充坐标单位，重新提交");
    let redelivered = client.tool("task_upsert", update);
    assert_eq!(redelivered["isError"], false);
    db.review(ReviewTask {
        id: capture.id,
        expected_updated_at: redelivered["structuredContent"]["updated_at"]
            .as_str()
            .unwrap()
            .into(),
        accepted: true,
        note: String::new(),
    })
    .unwrap();
    let final_task = db
        .board()
        .unwrap()
        .projects
        .pop()
        .unwrap()
        .tasks
        .pop()
        .unwrap();
    assert_eq!(final_task.review_status, ReviewStatus::Accepted);
    assert_eq!(final_task.id, capture.id);
    assert_eq!(
        final_task.agent_updated_at.as_deref(),
        redelivered["structuredContent"]["updated_at"].as_str()
    );
    assert_eq!(
        client.tool("task_list", json!({}))["structuredContent"]["items"],
        json!([])
    );
    client.close();
}

#[test]
fn pause_from_another_process_reaches_a_running_server() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("data");
    let mut client = Client::launch(&data_dir);
    let gui = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    gui.set_tracking_paused(true).unwrap();
    let paused = client.tool("task_upsert", args(root.path(), "paused", "todo"));
    assert_eq!(paused["isError"], false);
    assert_eq!(paused["structuredContent"]["paused"], true);
    assert!(gui.board().unwrap().projects.is_empty());
    gui.set_tracking_paused(false).unwrap();
    let resumed = client.tool("task_upsert", args(root.path(), "resumed", "todo"));
    assert!(resumed["structuredContent"]["id"].is_i64());
    client.close();
}

#[test]
fn two_processes_that_both_saw_no_task_cannot_silently_overwrite_a_creation() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("create-race");
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|index| {
            let project = root.path().to_path_buf();
            let data_dir = data_dir.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let mut client = Client::launch(&data_dir);
                let page = client.tool(
                    "task_list",
                    json!({"project_path":project,"task_key":"auto:race"}),
                );
                assert_eq!(page["structuredContent"]["items"], json!([]));
                barrier.wait();
                let mut input = args(&project, "auto:race", "in_progress");
                input["progress"] = json!(format!("writer-{index}"));
                let response = client.tool("task_upsert", input);
                client.close();
                response
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| result["isError"] == false)
            .count(),
        1
    );
    let loser = results
        .iter()
        .find(|result| result["isError"] == true)
        .unwrap();
    assert!(loser["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    assert_eq!(db.revision().unwrap(), 1);
    assert_eq!(db.board().unwrap().projects[0].tasks.len(), 1);
}

#[test]
fn independent_processes_using_one_version_have_exactly_one_update_winner() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("update-race");
    let mut creator = Client::launch(&data_dir);
    let created = creator.tool("task_upsert", args(root.path(), "auto:cas", "todo"));
    let stamp = created["structuredContent"]["updated_at"].clone();
    creator.close();
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = (0..2)
        .map(|index| {
            let project = root.path().to_path_buf();
            let data_dir = data_dir.clone();
            let barrier = barrier.clone();
            let stamp = stamp.clone();
            thread::spawn(move || {
                let mut client = Client::launch(&data_dir);
                let mut input = args(&project, "auto:cas", "in_progress");
                input["progress"] = json!(format!("writer-{index}"));
                input["expected_updated_at"] = stamp;
                barrier.wait();
                let response = client.tool("task_upsert", input);
                client.close();
                response
            })
        })
        .collect();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| result["isError"] == false)
            .count(),
        1
    );
    let loser = results
        .iter()
        .find(|result| result["isError"] == true)
        .unwrap();
    assert!(loser["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    assert_eq!(
        Database::open(data_dir.join("agentkanban.sqlite3"))
            .unwrap()
            .revision()
            .unwrap(),
        2
    );
}

#[test]
fn killed_mcp_can_be_continued_once_via_cli_and_restarted_without_losing_identity_or_steps() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("fallback-data");
    let mut client = Client::launch(&data_dir);
    let mut original = args(root.path(), "auto:transport-failure", "in_progress");
    original["agent"] = json!("Codex");
    original["goal"] = json!("保护原始步骤和用户需求");
    original["steps"] = json!([
        {"title":"完成实现","status":"in_progress","note":"保留实现备注"},
        {"title":"验证结果","status":"todo","note":"保留验证备注"}
    ]);
    let created = client.tool("task_upsert", original);
    assert_eq!(created["isError"], false);
    let id = created["structuredContent"]["id"].as_i64().unwrap();
    client.child.kill().unwrap();
    client.child.wait().unwrap();
    drop(client);

    let query_path = root.path().join("读取原任务 中文.json");
    let query = json!({"project_path":root.path(),"task_key":"auto:transport-failure"});
    std::fs::write(&query_path, serde_json::to_vec(&query).unwrap()).unwrap();
    let output = run_cli(
        &data_dir,
        &[
            "--call",
            "task_list",
            "--input-file",
            query_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let task = cli_json(&output)["items"][0].clone();
    assert_eq!(task["id"], id);
    assert_eq!(task["goal"], "保护原始步骤和用户需求");

    let mut update = args(root.path(), "auto:transport-failure", "in_progress");
    update["progress"] = json!("通过本地备用通路完成第一步");
    update["expected_updated_at"] = task["updated_at"].clone();
    update["step_updates"] = json!([{"index":0,"status":"done"}]);
    update["agent"] = Value::Null;
    let (output, receipt) = cli_call(&data_dir, "task_upsert", update);
    assert!(output.status.success(), "{receipt}");
    assert_eq!(receipt.as_object().unwrap().len(), 3);
    assert_eq!(receipt["id"], id);
    assert!(receipt.get("content").is_none() && receipt.get("structuredContent").is_none());
    assert!(output.stderr.is_empty());

    let mut restarted = Client::launch(&data_dir);
    let latest = restarted.tool("task_list", query)["structuredContent"]["items"][0].clone();
    assert_eq!(latest["id"], id);
    assert_eq!(latest["updated_at"], receipt["updated_at"]);
    assert_eq!(latest["progress"], "通过本地备用通路完成第一步");
    assert_eq!(latest["agent"], "Codex");
    assert_eq!(
        latest["steps"],
        json!([
            {"title":"完成实现","status":"done","note":"保留实现备注"},
            {"title":"验证结果","status":"todo","note":"保留验证备注"}
        ])
    );
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    let reports = db.reports(id).unwrap();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].payload.get("agent"), Some(&Value::Null));
    assert_eq!(
        reports[0].payload["step_updates"],
        json!([{"index":0,"status":"done"}])
    );
    assert!(reports[0].payload.get("steps").is_none());
    assert!(reports[0].payload.get("branch").is_none());
    restarted.close();
}

#[test]
fn cli_pause_returns_success_without_task_reads_or_writes_and_next_normal_call_can_resume() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("paused-cli");
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    db.set_tracking_paused(true).unwrap();
    let revision = db.revision().unwrap();
    for name in ["task_list", "task_upsert", "task_archive"] {
        // Even missing task fields must not reach task operations while paused.
        let (output, result) = cli_call(&data_dir, name, json!({}));
        assert!(output.status.success(), "{result}");
        assert_eq!(result["paused"], true);
        assert_eq!(result["recorded"], false);
        assert!(result["message"]
            .as_str()
            .unwrap()
            .contains("do not retry or poll"));
    }
    assert_eq!(db.revision().unwrap(), revision);
    assert!(db.board().unwrap().projects.is_empty());
    db.set_tracking_paused(false).unwrap();
    let (output, receipt) = cli_call(
        &data_dir,
        "task_upsert",
        args(root.path(), "after-resume", "todo"),
    );
    assert!(output.status.success(), "{receipt}");
    assert!(receipt["id"].is_i64());
    assert_eq!(db.revision().unwrap(), revision + 1);
}

#[test]
fn cli_conflicts_and_invalid_arguments_exit_nonzero_without_mutating_current_work() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("cli-guards");
    let original = args(root.path(), "guarded-cli", "todo");
    let (output, created) = cli_call(&data_dir, "task_upsert", original.clone());
    assert!(output.status.success(), "{created}");
    let mut update = original.clone();
    update["progress"] = json!("最新进展");
    update["expected_updated_at"] = created["updated_at"].clone();
    let (output, updated) = cli_call(&data_dir, "task_upsert", update.clone());
    assert!(output.status.success(), "{updated}");
    let db = Database::open(data_dir.join("agentkanban.sqlite3")).unwrap();
    let before = db.board().unwrap();
    let reports = db.reports(created["id"].as_i64().unwrap()).unwrap();
    update["progress"] = json!("不能覆盖新进展");
    let mut missing_token = update.clone();
    missing_token
        .as_object_mut()
        .unwrap()
        .remove("expected_updated_at");
    let mut bad_field = original.clone();
    bad_field["unknown_field"] = json!("拒绝");
    for invalid in [update, missing_token, bad_field, json!([]), Value::Null] {
        let (output, error) = cli_call(&data_dir, "task_upsert", invalid);
        assert!(!output.status.success());
        assert!(error["error"].is_string(), "{error}");
        assert!(!output.stderr.is_empty());
        let after = db.board().unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.projects[0].tasks, before.projects[0].tasks);
        assert_eq!(
            db.reports(created["id"].as_i64().unwrap()).unwrap(),
            reports
        );
    }
    let (output, error) = cli_call(
        &data_dir,
        "task_archive",
        json!({
            "project_path":root.path(),"task_key":"guarded-cli","expected_updated_at":created["updated_at"]
        }),
    );
    assert!(!output.status.success());
    assert!(error["error"].as_str().unwrap().contains("Conflict"));
    assert_eq!(db.revision().unwrap(), before.revision);
}

#[test]
fn cli_unicode_files_and_stdin_accept_utf8_with_or_without_bom() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("unicode-cli");
    let input_path = root.path().join("工具参数 带空格与中文.json");
    let mut input = args(root.path(), "unicode-input", "todo");
    input["title"] = json!("中文标题 🧪");
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend(serde_json::to_vec_pretty(&input).unwrap());
    std::fs::write(&input_path, bytes).unwrap();
    // Both flag orders are accepted; the file is ordinary pretty-printed JSON.
    let output = run_cli(
        &data_dir,
        &[
            "--input-file",
            input_path.to_str().unwrap(),
            "--call",
            "task_upsert",
        ],
        None,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let created = cli_json(&output);
    let query = json!({"project_path":root.path(),"task_key":"unicode-input"});
    for bom in [false, true] {
        let mut bytes = if bom {
            vec![0xef, 0xbb, 0xbf]
        } else {
            Vec::new()
        };
        bytes.extend(serde_json::to_vec(&query).unwrap());
        let output = run_cli(
            &data_dir,
            &["--call", "task_list", "--input-file", "-"],
            Some(&bytes),
        );
        assert!(output.status.success());
        let listed = cli_json(&output);
        assert_eq!(listed["items"][0]["id"], created["id"]);
        assert_eq!(listed["items"][0]["title"], "中文标题 🧪");
    }
}

#[test]
fn cli_rejects_unknown_duplicate_or_incomplete_flags_and_bounded_invalid_input() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("must-not-create-data");
    for flags in [
        vec!["--call", "task_delete", "--input-file", "-"],
        vec![
            "--call",
            "task_list",
            "--call",
            "task_list",
            "--input-file",
            "-",
        ],
        vec![
            "--call",
            "task_list",
            "--input-file",
            "-",
            "--input-file",
            "-",
        ],
        vec!["--call", "task_list"],
        vec!["--input-file", "-"],
        vec!["--call"],
        vec!["--input-file"],
        vec!["--call", "task_list", "--input-file", ""],
        vec!["--help", "--call", "task_list"],
        vec!["--unexpected"],
    ] {
        let output = run_cli(&data_dir, &flags, None);
        assert!(!output.status.success(), "{flags:?}");
        assert!(cli_json(&output)["error"].is_string());
        assert!(
            !data_dir.exists(),
            "argument errors must precede database opening"
        );
    }
    let input_path = root.path().join("invalid.json");
    for bytes in [
        vec![b' '; agentkanban_mcp::MAX_INPUT_BYTES + 1],
        vec![b'{', b'"', b'x', b'"', b':', b'"', 0xff, b'"', b'}'],
        b"{invalid json}".to_vec(),
        Vec::new(),
    ] {
        std::fs::write(&input_path, bytes).unwrap();
        let output = run_cli(
            &data_dir,
            &[
                "--call",
                "task_list",
                "--input-file",
                input_path.to_str().unwrap(),
            ],
            None,
        );
        assert!(!output.status.success());
        assert!(cli_json(&output)["error"].is_string());
        assert!(
            !data_dir.exists(),
            "input decoding errors must precede database opening"
        );
    }
}

#[test]
fn cli_archive_restore_uses_the_same_version_guard_and_minimal_receipts() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("cli-archive");
    let (_, created) = cli_call(
        &data_dir,
        "task_upsert",
        args(root.path(), "archive-cli", "in_progress"),
    );
    let query = json!({"project_path":root.path(),"task_key":"archive-cli"});
    let (output, failure) = cli_call(&data_dir, "task_archive", query.clone());
    assert!(!output.status.success());
    assert!(failure["error"].as_str().unwrap().contains("Conflict"));
    let mut archive = query.clone();
    archive["expected_updated_at"] = created["updated_at"].clone();
    let (output, archived) = cli_call(&data_dir, "task_archive", archive);
    assert!(output.status.success(), "{archived}");
    assert_eq!(archived.as_object().unwrap().len(), 3);
    assert_eq!(archived["id"], created["id"]);
    let (_, task) = cli_call(&data_dir, "task_list", query.clone());
    assert_eq!(task["items"][0]["archived"], true);
    let mut restore = query;
    restore["archived"] = json!(false);
    restore["expected_updated_at"] = archived["updated_at"].clone();
    let (output, restored) = cli_call(&data_dir, "task_archive", restore);
    assert!(output.status.success(), "{restored}");
    assert_eq!(restored["id"], created["id"]);
    assert_eq!(restored["status"], "in_progress");
}

#[test]
fn cli_help_and_version_remain_read_only_and_document_one_shot_usage() {
    let root = tempfile::tempdir().unwrap();
    let data_dir = root.path().join("unused-data");
    for flag in ["--help", "-h"] {
        let output = run_cli(&data_dir, &[flag], None);
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("--call") && text.contains("--input-file"));
        assert!(text.contains("AGENTKANBAN_DATA_DIR"));
    }
    let output = run_cli(&data_dir, &["--version"], None);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("agentkanban-mcp ", env!("CARGO_PKG_VERSION"))
    );
    assert!(!data_dir.exists());
}
