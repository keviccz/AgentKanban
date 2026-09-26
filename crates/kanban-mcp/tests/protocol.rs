use agentkanban_mcp::{serve, Server};
use kanban_core::{Database, SyncHealth, SyncOutcome, SyncTool, SyncTransport};
use serde_json::{json, Value};
use std::io::Cursor;

fn fixture() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("database.sqlite3")).unwrap();
    (root, db)
}

fn initialize(version: &str) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"Protocol test","version":"1"}}})
}

fn request(id: i64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

#[test]
fn only_received_known_tool_calls_record_health_and_empty_reads_allow_onboarding() {
    let (_root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(request(0, "tools/call", json!({"name":"task_list"})));
    server.handle(initialize("2025-11-25"));
    server.handle(request(2, "tools/list", json!({})));
    server.handle(request(3, "ping", json!({})));
    server.handle(request(4, "tools/call", json!({"name":"unknown"})));
    server.handle(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"task_list"}}));
    assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());

    let listed = server
        .handle(request(
            5,
            "tools/call",
            json!({"name":"task_list","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(
        listed["result"]["structuredContent"],
        json!({"items":[],"next_offset":null})
    );
    let health = db.get_sync_health().unwrap();
    assert_eq!(health.last_call, health.last_success);
    assert!(health.last_write.is_none());
    let event = health.last_call.unwrap();
    assert_eq!(event.transport, SyncTransport::Mcp);
    assert_eq!(event.tool, SyncTool::TaskList);
    assert_eq!(event.outcome, SyncOutcome::Ok);
    assert_eq!(db.revision().unwrap(), 0);
    assert!(db.initialize_tutorial().unwrap());
}

#[test]
fn tool_errors_are_private_categories_and_preserve_last_successful_write() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"private-task-key","title":"private title","status":"todo","progress":"private request"});
    let created = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    let receipt = &created["result"]["structuredContent"];
    assert_eq!(receipt.as_object().unwrap().len(), 3);
    let successful = db.get_sync_health().unwrap();

    let mut invalid = args.clone();
    invalid["status"] = json!("private-invalid-value");
    let failed = server
        .handle(request(
            3,
            "tools/call",
            json!({"name":"task_upsert","arguments":invalid}),
        ))
        .unwrap();
    assert_eq!(failed["result"]["isError"], true);
    let health = db.get_sync_health().unwrap();
    assert_eq!(health.last_write, successful.last_write);
    assert_eq!(health.last_success, successful.last_success);
    assert_eq!(
        health.last_call.unwrap().error.as_deref(),
        Some("Invalid tool arguments")
    );
    let raw = db.get_setting("sync_health").unwrap().unwrap();
    assert!(!raw.contains("private"));
    assert!(!raw.contains(root.path().to_str().unwrap()));

    let mut conflict = args;
    conflict["progress"] = json!("new content without expected token");
    let failed = server
        .handle(request(
            4,
            "tools/call",
            json!({"name":"task_upsert","arguments":conflict}),
        ))
        .unwrap();
    assert_eq!(failed["result"]["isError"], true);
    assert_eq!(
        db.get_sync_health()
            .unwrap()
            .last_call
            .unwrap()
            .error
            .as_deref(),
        Some("Version conflict; re-read the task")
    );
    assert_eq!(db.revision().unwrap(), 1);
    assert_eq!(
        db.reports(receipt["id"].as_i64().unwrap()).unwrap().len(),
        1
    );

    db.set_tracking_paused(true).unwrap();
    let paused = server
        .handle(request(
            5,
            "tools/call",
            json!({"name":"task_archive","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(paused["result"]["structuredContent"]["paused"], true);
    let health = db.get_sync_health().unwrap();
    assert!(health.paused);
    assert_eq!(health.last_call.unwrap().outcome, SyncOutcome::Paused);
    assert_eq!(health.last_write, successful.last_write);
    assert_eq!(health.last_success, successful.last_success);
}

#[test]
fn health_storage_failure_never_changes_tool_success_or_tool_failure() {
    let (root, db) = fixture();
    rusqlite::Connection::open(db.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_health BEFORE INSERT ON settings WHEN NEW.key='sync_health'
         BEGIN SELECT RAISE(ABORT,'injected health failure'); END;",
        )
        .unwrap();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"auto:health-failure","title":"Task","status":"todo","progress":"Original operation succeeds"});
    let created = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    assert_eq!(created["result"]["isError"], false);
    assert_eq!(
        created["result"]["structuredContent"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(db.revision().unwrap(), 1);
    assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());
    let failed = server
        .handle(request(
            3,
            "tools/call",
            json!({"name":"task_upsert","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(failed["result"]["isError"], true);
    assert!(failed["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Invalid input"));
    assert_eq!(db.revision().unwrap(), 1);
}

#[test]
fn locked_health_write_does_not_block_a_successful_read_for_task_timeout() {
    let (_root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = std::time::Instant::now();
    let response = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_list","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(response["result"]["isError"], false);
    assert!(started.elapsed() < std::time::Duration::from_millis(600));
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());
    assert_eq!(db.revision().unwrap(), 0);
}

#[test]
fn initialization_negotiates_current_and_older_clients_and_presents_only_three_tools() {
    let (_root, db) = fixture();
    for version in [
        "2025-11-25",
        "2025-06-18",
        "2025-03-26",
        "2024-11-05",
        "unsupported",
    ] {
        let mut server = Server::new(db.clone());
        assert_eq!(
            server.handle(request(0, "tools/list", json!({}))).unwrap()["error"]["code"],
            -32002
        );
        let response = server.handle(initialize(version)).unwrap();
        assert_eq!(
            response["result"]["protocolVersion"],
            if version == "unsupported" {
                "2025-11-25"
            } else {
                version
            }
        );
        assert_eq!(
            response["result"]["capabilities"],
            json!({"tools":{"listChanged":false}})
        );
        assert!(server
            .handle(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none());
        let tools = server.handle(request(2, "tools/list", json!({}))).unwrap();
        let tools = tools["result"]["tools"].as_array().unwrap();
        let names: Vec<_> = tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["task_upsert", "task_list", "task_archive"]);
        assert!(tools
            .iter()
            .all(|tool| tool["inputSchema"]["additionalProperties"] == false));
        assert_eq!(
            server.handle(request(3, "ping", json!({}))).unwrap()["result"],
            json!({})
        );
        assert_eq!(
            server.handle(initialize(version)).unwrap()["error"]["code"],
            -32600
        );
    }
}

#[test]
fn tool_updates_return_compact_receipts_and_validation_errors_are_explicit() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"tracked-feature","title":"中文测试","status":"in_progress","progress":"正在实现"});
    let first = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    assert_eq!(first["result"]["isError"], false);
    let receipt = &first["result"]["structuredContent"];
    assert_eq!(receipt.as_object().unwrap().len(), 3);
    assert_eq!(receipt["status"], "in_progress");
    let repeated = server
        .handle(request(
            3,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    assert_eq!(first["result"], repeated["result"]);
    let mut invalid = args.clone();
    invalid["silent_typo"] = json!(true);
    let failure = server
        .handle(request(
            4,
            "tools/call",
            json!({"name":"task_upsert","arguments":invalid}),
        ))
        .unwrap();
    assert_eq!(failure["result"]["isError"], true);
    assert!(failure["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("unknown field"));
    for arguments in [
        json!({"limit":101}),
        json!({"limit":-1}),
        json!({"status":"active"}),
    ] {
        assert_eq!(
            server
                .handle(request(
                    5,
                    "tools/call",
                    json!({"name":"task_list","arguments":arguments})
                ))
                .unwrap()["result"]["isError"],
            true
        );
    }
    assert_eq!(db.board().unwrap().projects[0].tasks.len(), 1);
    assert_eq!(db.revision().unwrap(), 1);
    assert_eq!(
        server
            .handle(request(6, "tools/call", json!({"name":"not_a_tool"})))
            .unwrap()["error"]["code"],
        -32602
    );
    assert_eq!(
        server
            .handle(request(
                7,
                "tools/call",
                json!({"name":"task_list","arguments":[]})
            ))
            .unwrap()["error"]["code"],
        -32602
    );
}

#[test]
fn older_clients_receive_json_text_without_newer_structured_content() {
    let (_root, db) = fixture();
    let mut server = Server::new(db);
    server.handle(initialize("2024-11-05"));
    let response = server
        .handle(request(2, "tools/call", json!({"name":"task_list"})))
        .unwrap();
    assert!(response["result"].get("structuredContent").is_none());
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(text).unwrap(),
        json!({"items":[],"next_offset":null})
    );
}

#[test]
fn malformed_requests_and_notifications_do_not_mutate_storage_or_corrupt_framing() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    assert_eq!(server.handle(json!([])).unwrap()["error"]["code"], -32600);
    assert_eq!(
        server
            .handle(json!({"jsonrpc":"2.0","id":null,"method":"ping"}))
            .unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(
        server
            .handle(json!({"jsonrpc":"1.0","id":9,"method":"ping"}))
            .unwrap()["error"]["code"],
        -32600
    );
    assert_eq!(
        server
            .handle(json!({"jsonrpc":"2.0","id":10,"method":"ping","params":[]}))
            .unwrap()["error"]["code"],
        -32602
    );
    server.handle(initialize("2025-11-25"));
    assert_eq!(
        server.handle(request(11, "missing", json!({}))).unwrap()["error"]["code"],
        -32601
    );
    assert!(server.handle(json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"task_upsert","arguments":{"project_path":root.path(),"task_key":"notification","title":"should not run","status":"todo","progress":""}}})).is_none());
    assert_eq!(db.revision().unwrap(), 0);
    let input = format!(
        "not json\n{}\n{{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}}\n{}\n",
        initialize("2025-11-25"),
        request(12, "ping", json!({}))
    );
    let mut output = Vec::new();
    serve(db, Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 3);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["id"], 1);
    assert_eq!(replies[2]["id"], 12);
}

#[test]
fn oversized_input_is_drained_and_next_message_is_readable() {
    let (_root, db) = fixture();
    let input = format!(
        "{}\n{}\n",
        "x".repeat(1024 * 1024 + 10),
        request(2, "ping", json!({}))
    );
    let mut output = Vec::new();
    serve(db, Cursor::new(input), &mut output).unwrap();
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(replies.len(), 2);
    assert_eq!(replies[0]["error"]["code"], -32700);
    assert_eq!(replies[1]["result"], json!({}));
}

#[test]
fn v03_human_fields_remain_read_only_and_patch_null_semantics_match_schema() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"human-boundary","title":"用户需求","status":"in_progress","progress":"已接手","agent":"Codex","next_action":"实现交付"});
    let created = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    assert_eq!(created["result"]["isError"], false);
    for (field, value) in [
        ("request", json!("overwrite original")),
        ("user_note", json!("overwrite feedback")),
        ("review_status", json!("accepted")),
        ("agent_updated_at", json!("2099-01-01T00:00:00.000Z")),
    ] {
        let mut forbidden = args.clone();
        forbidden[field] = value;
        let result = server
            .handle(request(
                3,
                "tools/call",
                json!({"name":"task_upsert","arguments":forbidden}),
            ))
            .unwrap();
        assert_eq!(result["result"]["isError"], true);
        assert!(result["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("unknown field"));
    }
    let mut no_change = args.clone();
    no_change["agent"] = Value::Null;
    let result = server
        .handle(request(
            4,
            "tools/call",
            json!({"name":"task_upsert","arguments":no_change}),
        ))
        .unwrap();
    assert_eq!(
        result["result"]["structuredContent"],
        created["result"]["structuredContent"]
    );
    assert_eq!(db.revision().unwrap(), 1);
    for field in [
        "next_action",
        "needs_input",
        "deliverables",
        "expected_updated_at",
    ] {
        let mut invalid = args.clone();
        invalid[field] = Value::Null;
        assert_eq!(
            server
                .handle(request(
                    5,
                    "tools/call",
                    json!({"name":"task_upsert","arguments":invalid})
                ))
                .unwrap()["result"]["isError"],
            true
        );
    }
    let current = db.board().unwrap();
    assert_eq!(current.projects[0].tasks[0].agent.as_deref(), Some("Codex"));
    assert_eq!(current.projects[0].tasks[0].next_action, "实现交付");
    assert_eq!(current.revision, 1);
}

#[test]
fn v03_exact_lookup_and_optimistic_conflicts_are_exposed_as_tool_errors() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"guarded","title":"待验收交付","status":"done","progress":"交付完成","deliverables":[{"label":"报告","uri":"report.html"}]});
    let created = server
        .handle(request(
            2,
            "tools/call",
            json!({"name":"task_upsert","arguments":args}),
        ))
        .unwrap();
    let list = server
        .handle(request(
            3,
            "tools/call",
            json!({"name":"task_list","arguments":{"task_key":"guarded","include_done":true}}),
        ))
        .unwrap();
    let task = &list["result"]["structuredContent"]["items"][0];
    assert_eq!(task["review_status"], "pending");
    assert_eq!(task["deliverables"][0]["uri"], "report.html");
    assert_eq!(
        task["agent_updated_at"],
        created["result"]["structuredContent"]["updated_at"]
    );
    let mut stale = args.clone();
    stale["expected_updated_at"] = json!("2000-01-01T00:00:00.000Z");
    let result = server
        .handle(request(
            4,
            "tools/call",
            json!({"name":"task_upsert","arguments":stale}),
        ))
        .unwrap();
    assert_eq!(result["result"]["isError"], true);
    assert!(result["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    let archived = server.handle(request(5,"tools/call",json!({"name":"task_archive","arguments":{"project_path":root.path(),"task_key":"guarded","expected_updated_at":"2000-01-01T00:00:00.000Z"}}))).unwrap();
    assert_eq!(archived["result"]["isError"], true);
    assert!(archived["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    assert_eq!(db.revision().unwrap(), 1);
    let list = server.handle(request(6,"tools/call",json!({"name":"task_list","arguments":{"task_key":"guarded-prefix","include_done":true}}))).unwrap();
    assert_eq!(list["result"]["structuredContent"]["items"], json!([]));
}

fn call(server: &mut Server, id: i64, name: &str, arguments: Value) -> Value {
    server
        .handle(request(
            id,
            "tools/call",
            json!({"name":name,"arguments":arguments}),
        ))
        .unwrap()["result"]
        .clone()
}

#[test]
fn v04_list_defaults_to_small_summaries_and_exact_keys_return_full_records() {
    let (root, _db) = fixture();
    let mut server = Server::new(_db);
    server.handle(initialize("2025-11-25"));
    for index in 0..7 {
        let steps = json!([{"title":"实现","status":"done","note":"完成"},{"title":"测试","status":"in_progress"}]);
        let result = call(
            &mut server,
            10 + index,
            "task_upsert",
            json!({"project_path":root.path(),"task_key":format!("auto:item-{index}"),"title":"条目","status":"in_progress","progress":"进行中","steps":steps}),
        );
        assert_eq!(result["isError"], false, "{result}");
    }
    let page = call(
        &mut server,
        30,
        "task_list",
        json!({"project_path":root.path()}),
    );
    let page = &page["structuredContent"];
    assert_eq!(page["items"].as_array().unwrap().len(), 5);
    assert_eq!(page["next_offset"], 5);
    let summary = page["items"][0].as_object().unwrap();
    assert_eq!(summary["steps"], "1/2");
    for heavy in [
        "request",
        "user_note",
        "deliverables",
        "next_action",
        "agent_updated_at",
    ] {
        assert!(!summary.contains_key(heavy), "summary leaked {heavy}");
    }
    assert!(!summary.contains_key("has_user_note"));

    let exact = call(
        &mut server,
        31,
        "task_list",
        json!({"project_path":root.path(),"task_key":"auto:item-3"}),
    );
    let full = &exact["structuredContent"]["items"][0];
    assert_eq!(full["steps"][0]["note"], "完成");
    assert!(full["steps"][1].get("note").is_none());
    assert_eq!(full["request"], "");

    let detailed = call(
        &mut server,
        32,
        "task_list",
        json!({"project_path":root.path(),"detail":true,"limit":2}),
    );
    assert_eq!(
        detailed["structuredContent"]["items"][0]["steps"][0]["title"],
        "实现"
    );
    let forced_summary = call(
        &mut server,
        33,
        "task_list",
        json!({"project_path":root.path(),"task_key":"auto:item-3","detail":false}),
    );
    assert_eq!(
        forced_summary["structuredContent"]["items"][0]["steps"],
        "1/2"
    );
    let invalid = call(&mut server, 34, "task_list", json!({"detail":"yes"}));
    assert_eq!(invalid["isError"], true);
    let invalid_step = call(
        &mut server,
        35,
        "task_upsert",
        json!({"project_path":root.path(),"task_key":"auto:bad","title":"x","status":"todo","progress":"","steps":[{"title":"x","status":"started"}]}),
    );
    assert_eq!(invalid_step["isError"], true);
}

#[test]
fn v04_tool_descriptions_carry_tracking_rules_and_stay_small() {
    let (_root, db) = fixture();
    let mut server = Server::new(db);
    let init = server.handle(initialize("2025-11-25")).unwrap();
    assert!(init["result"]["instructions"].as_str().unwrap().len() < 300);
    let tools = server.handle(request(2, "tools/list", json!({}))).unwrap();
    let tools = &tools["result"]["tools"];
    // Clients that ignore server instructions still see these descriptions.
    let list = tools[1]["description"].as_str().unwrap();
    assert!(list.contains("without being asked") && list.contains("modify files"));
    assert!(tools[0]["description"]
        .as_str()
        .unwrap()
        .contains("completed steps"));
    assert!(tools[0]["inputSchema"]["properties"]["acceptance"].is_object());
    assert!(list.contains("has_user_note"));
    // Bound schema growth in bytes; token cost depends on the client/tokenizer.
    assert!(
        tools.to_string().len() < 5600,
        "{} schema bytes",
        tools.to_string().len()
    );
}

#[test]
fn v04_pause_applies_to_running_sessions_without_writing_and_resumes() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let args = json!({"project_path":root.path(),"task_key":"auto:pause","title":"暂停测试","status":"in_progress","progress":"开始"});
    assert_eq!(
        call(&mut server, 2, "task_upsert", args.clone())["isError"],
        false
    );
    let revision = db.revision().unwrap();

    // Paused from the GUI while this session is already initialized.
    db.set_tracking_paused(true).unwrap();
    for (id, name, arguments) in [
        (3, "task_list", json!({"project_path":root.path()})),
        (
            4,
            "task_upsert",
            json!({"project_path":root.path(),"task_key":"auto:other","title":"不应写入","status":"todo","progress":""}),
        ),
        (
            5,
            "task_archive",
            json!({"project_path":root.path(),"task_key":"auto:pause"}),
        ),
    ] {
        let result = call(&mut server, id, name, arguments);
        assert_eq!(result["isError"], false, "a pause is not a tool failure");
        assert_eq!(result["structuredContent"]["paused"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("do not retry or poll"));
    }
    assert_eq!(db.revision().unwrap(), revision);
    assert_eq!(db.board().unwrap().projects[0].tasks.len(), 1);
    assert!(!db.board().unwrap().projects[0].tasks[0].archived);
    // Unknown tools are still protocol errors while paused.
    let unknown = server
        .handle(request(
            6,
            "tools/call",
            json!({"name":"task_delete","arguments":{}}),
        ))
        .unwrap();
    assert_eq!(unknown["error"]["code"], -32602);

    db.set_tracking_paused(false).unwrap();
    let resumed = call(
        &mut server,
        7,
        "task_list",
        json!({"project_path":root.path()}),
    );
    assert_eq!(
        resumed["structuredContent"]["items"][0]["task_key"],
        "auto:pause"
    );
}

#[test]
fn targeted_summaries_flag_hidden_requirements_and_keep_full_text_in_exact_reads() {
    let (root, db) = fixture();
    let captured = db
        .capture(kanban_core::CaptureTask {
            project_path: root.path().to_str().unwrap().into(),
            task_key: "user:keyword".into(),
            title: "用户任务".into(),
            request: "需求中的 50%_\\ 特殊词".into(),
        })
        .unwrap();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let long = "字".repeat(121);
    let write = call(
        &mut server,
        2,
        "task_upsert",
        json!({
            "project_path":root.path(),"task_key":"user:keyword","title":"用户任务",
            "status":"in_progress","progress":long,"expected_updated_at":captured.updated_at,
        }),
    );
    assert_eq!(write["isError"], false);
    let page = call(
        &mut server,
        3,
        "task_list",
        json!({"project_path":root.path(),"query":"%_\\"}),
    );
    let page = &page["structuredContent"];
    assert!(page["project_path"].is_string());
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    let item = &page["items"][0];
    assert!(item.get("project_path").is_none());
    assert_eq!(item["has_request"], true);
    assert_eq!(item["progress_truncated"], true);
    assert_eq!(item["progress"].as_str().unwrap().chars().count(), 120);
    assert!(item["updated_at"].is_string());
    let full = call(
        &mut server,
        4,
        "task_list",
        json!({"project_path":root.path(),"task_key":"user:keyword"}),
    );
    assert_eq!(full["structuredContent"]["items"][0]["progress"], long);
    assert_eq!(
        full["structuredContent"]["items"][0]["request"],
        "需求中的 50%_\\ 特殊词"
    );
    let unscoped = call(&mut server, 5, "task_list", json!({"query":"%_\\"}));
    assert!(unscoped["structuredContent"]["items"][0]["project_path"].is_string());
}

#[test]
fn exact_identity_reads_include_done_and_archived_unless_explicitly_excluded() {
    let (root, db) = fixture();
    let mut server = Server::new(db);
    server.handle(initialize("2025-11-25"));
    let created = call(
        &mut server,
        2,
        "task_upsert",
        json!({"project_path":root.path(),"task_key":"done","title":"完成任务","status":"done","progress":"已完成"}),
    );
    for query in [
        json!({"task_key":"done"}),
        json!({"task_key":"done","include_done":true}),
    ] {
        assert_eq!(
            call(&mut server, 3, "task_list", query)["structuredContent"]["items"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    assert_eq!(
        call(
            &mut server,
            4,
            "task_list",
            json!({"task_key":"done","include_done":false})
        )["structuredContent"]["items"],
        json!([])
    );
    let conflicting_filters = call(
        &mut server,
        9,
        "task_list",
        json!({"task_key":"done","status":"done","include_done":false}),
    );
    assert_eq!(conflicting_filters["structuredContent"]["items"], json!([]));
    assert_eq!(
        conflicting_filters["structuredContent"]["next_offset"],
        Value::Null
    );
    let archived = call(
        &mut server,
        5,
        "task_archive",
        json!({"project_path":root.path(),"task_key":"done","expected_updated_at":created["structuredContent"]["updated_at"]}),
    );
    assert_eq!(archived["isError"], false);
    assert_eq!(
        call(&mut server, 6, "task_list", json!({"task_key":"done"}))["structuredContent"]["items"]
            [0]["archived"],
        true
    );
    assert_eq!(
        call(
            &mut server,
            7,
            "task_list",
            json!({"task_key":"done","include_archived":false})
        )["structuredContent"]["items"],
        json!([])
    );
    assert_eq!(
        call(&mut server, 8, "task_list", json!({}))["structuredContent"]["items"],
        json!([])
    );
}

#[test]
fn mcp_raw_reports_and_partial_step_updates_preserve_input_and_version_boundaries() {
    let (root, db) = fixture();
    let mut server = Server::new(db.clone());
    server.handle(initialize("2025-11-25"));
    let original = json!({"project_path":root.path(),"task_key":"raw","title":"原样上报","status":"in_progress","progress":"开始","agent":null,"steps":[{"title":"一步","status":"todo","note":""}]});
    let created = call(&mut server, 2, "task_upsert", original.clone());
    assert_eq!(created["isError"], false);
    let id = created["structuredContent"]["id"].as_i64().unwrap();
    let mut expected = original.clone();
    expected.as_object_mut().unwrap().remove("project_path");
    expected.as_object_mut().unwrap().remove("task_key");
    assert_eq!(db.reports(id).unwrap()[0].payload, expected);
    let mut patch = original;
    patch.as_object_mut().unwrap().remove("steps");
    patch["step_updates"] = json!([{"index":0,"status":"done"}]);
    let unguarded = call(&mut server, 3, "task_upsert", patch.clone());
    assert_eq!(unguarded["isError"], true);
    assert!(unguarded["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Conflict"));
    patch["expected_updated_at"] = created["structuredContent"]["updated_at"].clone();
    assert_eq!(call(&mut server, 4, "task_upsert", patch)["isError"], false);
    assert_eq!(
        db.board().unwrap().projects[0].tasks[0].steps[0].status,
        kanban_core::StepStatus::Done
    );
    let report = &db.reports(id).unwrap()[0].payload;
    assert!(report.get("steps").is_none());
    assert_eq!(report["step_updates"], json!([{"index":0,"status":"done"}]));
}
