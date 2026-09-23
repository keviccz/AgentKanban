//! Small newline-delimited JSON-RPC implementation of the local MCP surface.

use kanban_core::{
    ArchiveTask, Database, ListTasks, ListedTask, ReviewStatus, Status, StepStatus, UpsertTask,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_LINE_BYTES: usize = 1024 * 1024;
// Some clients (e.g. Codex CLI 0.156) do not show server instructions to the
// model, so the tracking rules live in the tool descriptions; this only adds the rest.
const INSTRUCTIONS: &str = "AgentKanban tracks file-modifying work automatically; the task_list and task_upsert descriptions say when. On Conflict, re-read with task_key before retrying. Keep board bookkeeping out of replies unless it fails.";
/// Default page for task_list over MCP; summaries keep the resume query cheap.
const LIST_LIMIT: u64 = 5;

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
            "task_list" => list_tasks(&self.db, call.arguments),
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

/// One line per task for discovery. Full records (request, user_note, steps,
/// deliverables) come back only for an exact task_key or detail=true.
#[derive(Serialize)]
struct TaskSummary<'a> {
    id: i64,
    task_key: &'a str,
    title: &'a str,
    status: Status,
    progress: &'a str,
    branch: &'a Option<String>,
    archived: bool,
    review_status: ReviewStatus,
    updated_at: &'a str,
    project_path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    has_user_note: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    needs_input: bool,
}

impl<'a> From<&'a ListedTask> for TaskSummary<'a> {
    fn from(listed: &'a ListedTask) -> Self {
        let task = &listed.task;
        let done = task
            .steps
            .iter()
            .filter(|step| step.status == StepStatus::Done)
            .count();
        Self {
            id: task.id,
            task_key: &task.task_key,
            title: &task.title,
            status: task.status,
            progress: &task.progress,
            branch: &task.branch,
            archived: task.archived,
            review_status: task.review_status,
            updated_at: &task.updated_at,
            project_path: &listed.project_path,
            steps: (!task.steps.is_empty()).then(|| format!("{done}/{}", task.steps.len())),
            has_user_note: !task.user_note.is_empty(),
            needs_input: !task.needs_input.is_empty(),
        }
    }
}

fn list_tasks(db: &Database, mut arguments: Value) -> std::result::Result<Value, String> {
    let object = arguments
        .as_object_mut()
        .ok_or("Invalid tool arguments: expected an object")?;
    let detail = match object.remove("detail") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(detail)) => Some(detail),
        Some(_) => return Err("Invalid tool arguments: detail must be a boolean".into()),
    };
    object.entry("limit").or_insert(json!(LIST_LIMIT));
    let input: ListTasks = serde_json::from_value(arguments)
        .map_err(|error| format!("Invalid tool arguments: {error}"))?;
    let detail = detail.unwrap_or(input.task_key.is_some());
    let page = db.list(input).map_err(|error| error.to_string())?;
    let result = if detail {
        serde_json::to_value(&page)
    } else {
        let items: Vec<TaskSummary> = page.items.iter().map(TaskSummary::from).collect();
        serde_json::to_value(json!({"items": items, "next_offset": page.next_offset}))
    };
    result.map_err(|error| format!("Cannot serialize result: {error}"))
}

fn rpc_error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}})
}

pub fn tool_definitions() -> Vec<Value> {
    let project_path =
        json!({"type":"string","minLength":1,"description":"Absolute project directory."});
    let task_key = json!({"type":"string","minLength":1,"maxLength":160});
    let status = json!({"type":"string","enum":["todo","in_progress","blocked","done"]});
    let expected = json!({"type":"string","minLength":1,"maxLength":64,"description":"updated_at from your last read or receipt."});
    let text = |max: u32, description: &str| json!({"type":"string","maxLength":max,"description":description});
    vec![
        json!({
            "name":"task_upsert",
            "description":"Create or update a tracked task by project_path+task_key. Create with a plan in steps. Afterwards update only at milestones (a step finished, a real blocker, done), never per edit or command or with unchanged state. Pass the last receipt's updated_at as expected_updated_at. done = awaiting human review. title/status/progress replace; omitted/null branch clears; agent/next_action/needs_input/deliverables/steps: omit keeps, send replaces. Single-line text.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"task_key":task_key,
                    "title":{"type":"string","minLength":1,"maxLength":200},
                    "status":status,
                    "progress":text(600, "One-line summary of the latest milestone or blocker."),
                    "branch":{"type":["string","null"],"minLength":1,"maxLength":200},
                    "agent":{"type":["string","null"],"maxLength":100},
                    "next_action":text(600, "Next step."),
                    "needs_input":text(600, "What the user must provide."),
                    "deliverables":{"type":"array","maxItems":5,"items":{"type":"object","additionalProperties":false,"properties":{"label":{"type":"string","minLength":1,"maxLength":100},"uri":{"type":"string","minLength":1,"maxLength":1000,"description":"File path or http(s) URL."}},"required":["label","uri"]}},
                    "steps":{"type":"array","maxItems":12,"description":"Whole plan; resend all steps when one changes.","items":{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","minLength":1,"maxLength":120},"status":status,"note":{"type":"string","maxLength":200}},"required":["title","status"]}},
                    "expected_updated_at":expected
                },
                "required":["project_path","task_key","title","status","progress"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_list",
            "description":"The user's task board. Tracking is automatic: at the start of any task that will modify files (code, config, docs), call this with project_path without being asked; skip Q&A, read-only work, or when the user says not to track. Reuse a matching task_key, else create auto:<short-slug> with task_upsert. Returns 5 unfinished one-line summaries per page (follow next_offset); an exact task_key or detail=true returns full records.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"task_key":task_key,"status":status,
                    "include_done":{"type":"boolean","default":false},
                    "include_archived":{"type":"boolean","default":false},
                    "detail":{"type":"boolean"},
                    "limit":{"type":"integer","minimum":1,"maximum":100,"default":LIST_LIMIT},
                    "offset":{"type":"integer","minimum":0,"maximum":4294967295_u64,"default":0}
                }
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_archive",
            "description":"Hide a task without deleting it, or restore with archived=false.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{"project_path":project_path,"task_key":task_key,"archived":{"type":"boolean","default":true},"expected_updated_at":expected},
                "required":["project_path","task_key"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
    ]
}
