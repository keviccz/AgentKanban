//! Small newline-delimited JSON-RPC implementation of the local MCP surface.

use kanban_core::{ArchiveTask, Database, ListTasks, UpsertTask};
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_LINE_BYTES: usize = 1024 * 1024;
const INSTRUCTIONS: &str = "Track only work the user explicitly asks to put on the board. Reuse project_path + a stable task_key. Query unfinished tasks when resuming; upsert only meaningful start/progress/block/completion changes. Use done for completion. Do not log chats or commands. Status is the agent's last report, not a live activity signal.";

pub fn serve(db: Database, mut input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    let mut server = Server::new(db);
    loop {
        let Some(line) = read_message(&mut input)? else {
            return Ok(());
        };
        let response = match line {
            MessageLine::TooLarge => Some(rpc_error(
                Value::Null,
                -32700,
                "Message exceeds the 1 MiB limit",
            )),
            MessageLine::Data(bytes) => match serde_json::from_slice::<Value>(&bytes) {
                Ok(value) => server.handle(value),
                Err(_) => Some(rpc_error(
                    Value::Null,
                    -32700,
                    "Parse error: expected one UTF-8 JSON message per line",
                )),
            },
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
}

enum MessageLine {
    Data(Vec<u8>),
    TooLarge,
}

// Bound allocation even for a client that never sends a newline; drain oversized frames.
fn read_message(input: &mut impl BufRead) -> io::Result<Option<MessageLine>> {
    let mut message = Vec::new();
    let mut too_large = false;
    let mut received = false;
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return Ok(if !received {
                None
            } else if too_large {
                Some(MessageLine::TooLarge)
            } else {
                Some(MessageLine::Data(message))
            });
        }
        received = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        if !too_large {
            if message.len() + consumed > MAX_LINE_BYTES {
                too_large = true;
                message.clear();
            } else {
                message.extend_from_slice(&available[..consumed]);
            }
        }
        input.consume(consumed);
        if newline.is_some() {
            return Ok(Some(if too_large {
                MessageLine::TooLarge
            } else {
                MessageLine::Data(message)
            }));
        }
    }
}

pub struct Server {
    db: Database,
    initialized: bool,
    structured_content: bool,
}

impl Server {
    pub fn new(db: Database) -> Self {
        Self {
            db,
            initialized: false,
            structured_content: true,
        }
    }

    pub fn handle(&mut self, request: Value) -> Option<Value> {
        let Some(object) = request.as_object() else {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "Invalid Request: expected an object",
            ));
        };
        let id = object.get("id");
        if id.is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64()) {
            return Some(rpc_error(
                Value::Null,
                -32600,
                "Invalid Request: id must be a string or integer",
            ));
        }
        if object.get("jsonrpc") != Some(&json!("2.0")) {
            return Some(rpc_error(
                id.cloned().unwrap_or(Value::Null),
                -32600,
                "Invalid Request: jsonrpc must be 2.0",
            ));
        }
        let Some(method) = object.get("method").and_then(Value::as_str) else {
            // We issue no server-to-client requests, so unsolicited valid responses are ignored.
            if id.is_some() && (object.contains_key("result") || object.contains_key("error")) {
                return None;
            }
            return Some(rpc_error(
                id.cloned().unwrap_or(Value::Null),
                -32600,
                "Invalid Request: missing method",
            ));
        };
        // Notifications, including initialized/cancelled, never receive a reply or mutate tasks.
        let id = id?.clone();
        let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
        if !params.is_object() {
            return Some(rpc_error(id, -32602, "Invalid params: expected an object"));
        }
        let result = match method {
            "initialize" => self.initialize(params),
            "ping" => Ok(json!({})),
            _ if !self.initialized => Err((-32002, "Server not initialized".to_string())),
            "tools/list" => {
                if params
                    .get("cursor")
                    .is_some_and(|value| !value.is_null() && value != "")
                {
                    Err((
                        -32602,
                        "Invalid cursor; all three tools fit in one page".into(),
                    ))
                } else {
                    Ok(json!({"tools": tool_definitions()}))
                }
            }
            "tools/call" => self.call_tool(params),
            _ => Err((-32601, format!("Method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
            Err((code, message)) => rpc_error(id, code, &message),
        })
    }

    fn initialize(&mut self, params: Value) -> std::result::Result<Value, (i32, String)> {
        if self.initialized {
            return Err((-32600, "Server is already initialized".into()));
        }
        let version = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| (-32602, "protocolVersion is required".into()))?;
        if !params.get("capabilities").is_some_and(Value::is_object)
            || !params.get("clientInfo").is_some_and(|info| {
                info.get("name").is_some_and(Value::is_string)
                    && info.get("version").is_some_and(Value::is_string)
            })
        {
            return Err((
                -32602,
                "initialize requires capabilities and clientInfo.name/version".into(),
            ));
        }
        let negotiated = if SUPPORTED_VERSIONS.contains(&version) {
            version
        } else {
            PROTOCOL_VERSION
        };
        self.initialized = true;
        self.structured_content = negotiated >= "2025-06-18";
        Ok(json!({
            "protocolVersion": negotiated,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "AgentKanban", "version": env!("CARGO_PKG_VERSION")},
            "instructions": INSTRUCTIONS
        }))
    }

    fn call_tool(&self, params: Value) -> std::result::Result<Value, (i32, String)> {
        #[derive(Deserialize)]
        struct CallParams {
            name: String,
            #[serde(default = "empty_object")]
            arguments: Value,
        }
        fn empty_object() -> Value {
            json!({})
        }
        let call: CallParams = serde_json::from_value(params)
            .map_err(|error| (-32602, format!("Invalid tools/call params: {error}")))?;
        if !call.arguments.is_object() {
            return Err((-32602, "arguments must be an object".into()));
        }
        let result = match call.name.as_str() {
            "task_upsert" => {
                parse_and_run::<UpsertTask, _>(call.arguments, |input| self.db.upsert(input))
            }
            "task_list" => {
                parse_and_run::<ListTasks, _>(call.arguments, |input| self.db.list(input))
            }
            "task_archive" => {
                parse_and_run::<ArchiveTask, _>(call.arguments, |input| self.db.archive(input))
            }
            _ => return Err((-32602, format!("Unknown tool: {}", call.name))),
        };
        Ok(match result {
            Ok(value) => {
                let mut result =
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false});
                if self.structured_content {
                    result["structuredContent"] = value;
                }
                result
            }
            Err(error) => json!({"content":[{"type":"text","text":error}],"isError":true}),
        })
    }
}

fn parse_and_run<T: serde::de::DeserializeOwned, R: serde::Serialize>(
    arguments: Value,
    operation: impl FnOnce(T) -> kanban_core::Result<R>,
) -> std::result::Result<Value, String> {
    let input = serde_json::from_value(arguments)
        .map_err(|error| format!("Invalid tool arguments: {error}"))?;
    let result = operation(input).map_err(|error| error.to_string())?;
    serde_json::to_value(result).map_err(|error| format!("Cannot serialize result: {error}"))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn tool_definitions() -> Vec<Value> {
    let project_path = json!({"type":"string","minLength":1,"description":"Absolute existing project directory. Git worktrees share their repository."});
    let task_key = json!({"type":"string","minLength":1,"maxLength":160,"description":"Stable feature key reused across sessions."});
    let status = json!({"type":"string","enum":["todo","in_progress","blocked","done"]});
    vec![
        json!({
            "name":"task_upsert",
            "description":"Create or update one explicitly tracked task. Same project + task_key is idempotent. Supply all visible fields; omitted/null branch clears it. Restore archived tasks first. Returns only id, status, updated_at.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"task_key":task_key,
                    "title":{"type":"string","minLength":1,"maxLength":200},
                    "status":status,
                    "progress":{"type":"string","maxLength":600,"description":"One short line about meaningful progress or the blocker."},
                    "branch":{"type":["string","null"],"minLength":1,"maxLength":200}
                },
                "required":["project_path","task_key","title","status","progress"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_list",
            "description":"Resume tracked work. Defaults to 20 unfinished, unarchived tasks; optional project/status filters. Follow next_offset for more. Explicit status=done includes completed tasks.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"status":status,
                    "include_done":{"type":"boolean","default":false},
                    "include_archived":{"type":"boolean","default":false},
                    "limit":{"type":"integer","minimum":1,"maximum":100,"default":20},
                    "offset":{"type":"integer","minimum":0,"maximum":4294967295_u64,"default":0}
                }
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_archive",
            "description":"Hide a task without deleting data, or restore with archived=false. Returns only id, status, updated_at.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{"project_path":project_path,"task_key":task_key,"archived":{"type":"boolean","default":true}},
                "required":["project_path","task_key"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
    ]
}
