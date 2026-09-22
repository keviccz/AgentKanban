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
