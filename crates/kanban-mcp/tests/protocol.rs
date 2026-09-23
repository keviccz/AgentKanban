use agentkanban_mcp::{serve, Server};
use kanban_core::Database;
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
        .contains("milestones"));
    assert!(tools.to_string().len() < 4300);
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
            .contains("Do not call"));
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
