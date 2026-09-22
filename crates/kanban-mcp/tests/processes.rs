use kanban_core::{Database, Status};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
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
        assert!(
            self.output.read_line(&mut line).unwrap() > 0,
            "server closed stdout before replying"
        );
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
    assert_eq!(
        client.tool("task_upsert", args(root.path(), "offline-gui", "done"))["isError"],
        false
    );
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
                    let updated = client.tool("task_upsert", args(&project, &key, "in_progress"));
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
