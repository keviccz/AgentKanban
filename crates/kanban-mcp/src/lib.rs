//! Small newline-delimited JSON-RPC implementation of the local MCP surface.

use kanban_core::{
    ArchiveTask, Database, ListTasks, ListedTask, ReviewStatus, Status, StepStatus, SyncOutcome,
    SyncTool, SyncTransport,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;
pub const TOOL_NAMES: &[&str] = &["task_upsert", "task_list", "task_archive"];
static SYNC_HEALTH_WARNING_PRINTED: AtomicBool = AtomicBool::new(false);
// Some clients (e.g. Codex CLI 0.156) do not show server instructions to the
// model, so the tracking rules live in the tool descriptions; this only adds the rest.
const INSTRUCTIONS: &str = "AgentKanban tracks file-modifying work automatically; the task_list and task_upsert descriptions say when. On Conflict, re-read with task_key before retrying. Keep board bookkeeping out of replies unless it fails.";
const PAUSED_MESSAGE: &str = "The user paused AgentKanban tracking. Nothing was read or recorded. Skip this update and continue the task; do not retry or poll. At the next normal milestone or new task, try once so desktop resume can take effect.";
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
            if message.len() + consumed > MAX_INPUT_BYTES {
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
        Ok(match execute_tool(&self.db, &call.name, call.arguments) {
            Ok(value) => {
                let mut result =
                    json!({"content":[{"type":"text","text":value.to_string()}],"isError":false});
                if self.structured_content {
                    result["structuredContent"] = value;
                }
                result
            }
            Err(ToolCallError::InvalidParams(error)) => return Err((-32602, error)),
            Err(ToolCallError::Operation(error)) => {
                json!({"content":[{"type":"text","text":error}],"isError":true})
            }
        })
    }
}

#[derive(Debug)]
pub enum ToolCallError {
    InvalidParams(String),
    Operation(String),
}

impl std::fmt::Display for ToolCallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidParams(message) | Self::Operation(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for ToolCallError {}

/// Shared by MCP and the one-shot local CLI. The transport cannot bypass pause,
/// argument validation, optimistic concurrency, or the original report payload.
pub fn execute_tool(
    db: &Database,
    name: &str,
    arguments: Value,
) -> std::result::Result<Value, ToolCallError> {
    execute_tool_with_transport(db, name, arguments, SyncTransport::Mcp)
}

pub fn execute_tool_with_transport(
    db: &Database,
    name: &str,
    arguments: Value,
    transport: SyncTransport,
) -> std::result::Result<Value, ToolCallError> {
    let result = execute_tool_inner(db, name, arguments);
    // Only a recognized, received tool call is an observation. Initialization,
    // transport closure, malformed frames and unknown tools say no such thing.
    if let Some(tool) = SyncTool::from_name(name) {
        let (outcome, error) = match &result {
            Ok(value) if value.get("paused") == Some(&Value::Bool(true)) => {
                (SyncOutcome::Paused, None)
            }
            Ok(_) => (SyncOutcome::Ok, None),
            Err(error) => (SyncOutcome::Error, Some(sync_error_category(error))),
        };
        if db
            .record_sync_event(transport, tool, outcome, error)
            .is_err()
            && !SYNC_HEALTH_WARNING_PRINTED.swap(true, Ordering::Relaxed)
        {
            // At most one short diagnostic per process; a broken stderr must
            // not panic or replace the successful task result either.
            let _ = writeln!(
                io::stderr().lock(),
                "AgentKanban: sync health could not be recorded"
            );
        }
    }
    result
}

fn sync_error_category(error: &ToolCallError) -> &'static str {
    let ToolCallError::Operation(message) = error else {
        return "Invalid tool arguments";
    };
    if message.starts_with("Conflict:") {
        "Version conflict; re-read the task"
    } else if message.starts_with("Task not found;") {
        "Task not found"
    } else if message.starts_with("Task is archived;") {
        "Task is archived"
    } else if message.starts_with("Invalid tool arguments:")
        || message.starts_with("Invalid input:")
    {
        "Invalid tool arguments"
    } else if message.starts_with("Cannot identify project:") {
        "Project is unavailable"
    } else if message.starts_with("Database error:") {
        "Database operation failed"
    } else if message.starts_with("Filesystem error:") {
        "Filesystem operation failed"
    } else {
        "Tool operation failed"
    }
}

fn execute_tool_inner(
    db: &Database,
    name: &str,
    arguments: Value,
) -> std::result::Result<Value, ToolCallError> {
    if !arguments.is_object() {
        return Err(ToolCallError::InvalidParams(
            "arguments must be an object".into(),
        ));
    }
    if !TOOL_NAMES.contains(&name) {
        return Err(ToolCallError::InvalidParams(format!(
            "Unknown tool: {name}"
        )));
    }
    // Pausing is successful but never reads or changes a task. Both transports
    // check the same persisted setting on every normal call.
    let paused = db
        .tracking_paused()
        .map_err(|error| ToolCallError::Operation(error.to_string()))?;
    if paused {
        return Ok(json!({"paused":true,"recorded":false,"message":PAUSED_MESSAGE}));
    }
    run_tool(db, name, arguments).map_err(ToolCallError::Operation)
}

fn run_tool(db: &Database, name: &str, arguments: Value) -> std::result::Result<Value, String> {
    match name {
        "task_upsert" => {
            let receipt = db
                .upsert_from_json(arguments)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(receipt)
                .map_err(|error| format!("Cannot serialize result: {error}"))
        }
        "task_list" => list_tasks(db, arguments),
        "task_archive" => parse_and_run::<ArchiveTask, _>(arguments, |input| db.archive(input)),
        _ => Err(format!("Unknown tool: {name}")),
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
    progress: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    progress_truncated: bool,
    branch: &'a Option<String>,
    archived: bool,
    review_status: ReviewStatus,
    updated_at: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    project_path: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    steps: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    has_user_note: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    has_request: bool,
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
            progress: task.progress.chars().take(120).collect(),
            progress_truncated: task.progress.chars().count() > 120,
            branch: &task.branch,
            archived: task.archived,
            review_status: task.review_status,
            updated_at: &task.updated_at,
            project_path: Some(&listed.project_path),
            steps: (!task.steps.is_empty()).then(|| format!("{done}/{}", task.steps.len())),
            has_user_note: !task.user_note.is_empty(),
            has_request: !task.request.is_empty(),
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
    let excludes_requested_done = object.get("include_done") == Some(&json!(false))
        && object.get("status") == Some(&json!("done"));
    if object.contains_key("task_key") {
        // Exact identity reads are used to resume work and recover from conflicts.
        // Explicit false values still narrow the caller's requested scope.
        object.entry("include_done").or_insert(json!(true));
        object.entry("include_archived").or_insert(json!(true));
    }
    object.entry("limit").or_insert(json!(LIST_LIMIT));
    let input: ListTasks = serde_json::from_value(arguments)
        .map_err(|error| format!("Invalid tool arguments: {error}"))?;
    let detail = detail.unwrap_or(input.task_key.is_some());
    let scoped_path = input.project_path.clone();
    let mut page = db.list(input).map_err(|error| error.to_string())?;
    // Unlike an omitted flag, explicit false is a filter even alongside status=done.
    if excludes_requested_done {
        page.items.clear();
        page.next_offset = None;
    }
    let result = if detail {
        serde_json::to_value(&page)
    } else {
        let items: Vec<TaskSummary> = page
            .items
            .iter()
            .map(|listed| {
                let mut summary = TaskSummary::from(listed);
                if scoped_path.is_some() {
                    summary.project_path = None;
                }
                summary
            })
            .collect();
        let mut result = json!({"items": items, "next_offset": page.next_offset});
        if let Some(path) = scoped_path {
            result["project_path"] = json!(page
                .items
                .first()
                .map(|item| item.project_path.as_str())
                .unwrap_or(&path));
        }
        Ok(result)
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
            "description":"One designated Agent records each task by project_path+task_key; delegates report to it. Create with goal, acceptance and steps. Update only at completed steps, real blockers or done, never per command. Existing changes require expected_updated_at from the last receipt/read; on Conflict re-read the key. Use step_updates for changed steps. done awaits human review. title/status/progress replace; omitted/null branch clears; other optional fields omit keeps. Single-line text.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"task_key":task_key,
                    "title":{"type":"string","minLength":1,"maxLength":200},
                    "status":status,
                    "progress":text(600, "One-line summary of the latest milestone or blocker."),
                    "branch":{"type":["string","null"],"minLength":1,"maxLength":200},
                    "agent":{"type":["string","null"],"maxLength":100,"description":"Your client name, e.g. Codex or Claude Code."},
                    "next_action":text(600, "Next step."),
                    "needs_input":text(600, "What the user must provide."),
                    "deliverables":{"type":"array","maxItems":5,"items":{"type":"object","additionalProperties":false,"properties":{"label":{"type":"string","minLength":1,"maxLength":100},"uri":{"type":"string","minLength":1,"maxLength":1000,"description":"File path or http(s) URL."}},"required":["label","uri"]}},
                    "steps":{"type":"array","maxItems":12,"description":"Whole plan for creation or reordering; mutually exclusive with step_updates.","items":{"type":"object","additionalProperties":false,"properties":{"title":{"type":"string","minLength":1,"maxLength":120},"status":status,"note":{"type":"string","maxLength":200}},"required":["title","status"]}},
                    "step_updates":{"type":"array","maxItems":12,"description":"Patch existing steps by zero-based index; status/note omit keeps, empty note clears. Requires status or note per item; unique valid indices.","items":{"type":"object","additionalProperties":false,"properties":{"index":{"type":"integer","minimum":0,"maximum":11},"status":status,"note":{"type":"string","maxLength":200}},"required":["index"],"anyOf":[{"required":["status"]},{"required":["note"]}]}},
                    "goal":text(300, "What this task delivers and why, one or two sentences."),
                    "acceptance":{"type":"array","maxItems":8,"description":"Checks the user can do to accept the result.","items":{"type":"string","minLength":1,"maxLength":160}},
                    "expected_updated_at":expected
                },
                "required":["project_path","task_key","title","status","progress"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_list",
            "description":"For work that will modify files, one designated Agent calls this with project_path+query without being asked; skip Q&A, read-only work or user opt-out. Search a relevant keyword, do not scan all pages by default. Reuse a matching task_key, else create auto:<short-slug>. Returns 5 unfinished summaries; has_request/has_user_note or truncated progress: read the matching key before acting. Exact task_key returns full records including done/archived unless explicitly excluded.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{
                    "project_path":project_path,"task_key":task_key,"status":status,
                    "query":{"type":"string","minLength":1,"maxLength":160,"description":"Literal keyword in key, title, goal, request, progress or user note."},
                    "include_done":{"type":"boolean","description":"Default true with exact task_key, otherwise false."},
                    "include_archived":{"type":"boolean","description":"Default true with exact task_key, otherwise false."},
                    "detail":{"type":"boolean"},
                    "limit":{"type":"integer","minimum":1,"maximum":100,"default":LIST_LIMIT},
                    "offset":{"type":"integer","minimum":0,"maximum":4294967295_u64,"default":0}
                }
            },
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
        json!({
            "name":"task_archive",
            "description":"Hide a task without deleting it, or restore with archived=false. Changes require expected_updated_at from the latest read/receipt; identical retries may omit it.",
            "inputSchema":{
                "type":"object","additionalProperties":false,
                "properties":{"project_path":project_path,"task_key":task_key,"archived":{"type":"boolean","default":true},"expected_updated_at":expected},
                "required":["project_path","task_key"]
            },
            "annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}
        }),
    ]
}
