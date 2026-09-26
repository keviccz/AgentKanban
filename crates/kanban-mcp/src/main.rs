use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
};

enum Mode {
    Stdio,
    Help,
    Version,
    Call { name: String, input: OsString },
}

fn main() {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    let one_shot = !arguments.is_empty();
    if let Err(error) = run(arguments) {
        if one_shot {
            // One-shot callers receive one JSON value, including on failure.
            let _ = write_json(&json!({"error":error.to_string()}));
        }
        eprintln!("AgentKanban MCP: {error}");
        std::process::exit(1);
    }
}

fn parse_mode(arguments: Vec<OsString>) -> Result<Mode, String> {
    if arguments.is_empty() {
        return Ok(Mode::Stdio);
    }
    if arguments == ["--version"] {
        return Ok(Mode::Version);
    }
    if arguments == ["--help"] || arguments == ["-h"] {
        return Ok(Mode::Help);
    }
    let mut name = None;
    let mut input = None;
    let mut arguments = arguments.into_iter();
    while let Some(flag) = arguments.next() {
        match flag.to_str() {
            Some("--call") => {
                if name.is_some() {
                    return Err("duplicate --call flag".into());
                }
                let value = arguments.next().ok_or("--call requires a tool name")?;
                let value = value
                    .into_string()
                    .map_err(|_| "tool name must be valid Unicode")?;
                if !agentkanban_mcp::TOOL_NAMES.contains(&value.as_str()) {
                    return Err(format!("Unknown tool: {value}"));
                }
                name = Some(value);
            }
            Some("--input-file") => {
                if input.is_some() {
                    return Err("duplicate --input-file flag".into());
                }
                let value = arguments
                    .next()
                    .ok_or("--input-file requires a path or -")?;
                if value.is_empty() {
                    return Err("--input-file requires a nonempty path or -".into());
                }
                input = Some(value);
            }
            _ => {
                return Err(format!(
                    "unknown argument: {}; use --help",
                    flag.to_string_lossy()
                ))
            }
        }
    }
    Ok(Mode::Call {
        name: name.ok_or("--call is required for one-shot mode")?,
        input: input.ok_or("--input-file is required for one-shot mode")?,
    })
}

fn read_arguments(input: impl Read) -> Result<Value, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    input
        .take((agentkanban_mcp::MAX_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > agentkanban_mcp::MAX_INPUT_BYTES {
        return Err("JSON input exceeds the 1 MiB limit".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    let value = serde_json::from_slice(bytes)
        .map_err(|error| format!("Invalid JSON input; UTF-8 is required: {error}"))?;
    Ok(value)
}

fn write_json(value: &Value) -> io::Result<()> {
    let mut output = io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn run(arguments: Vec<OsString>) -> Result<(), Box<dyn std::error::Error>> {
    match parse_mode(arguments)? {
        Mode::Version => {
            println!("agentkanban-mcp {}", env!("CARGO_PKG_VERSION"));
        }
        Mode::Help => {
            println!("AgentKanban MCP {}\n\nUsage:\n  agentkanban-mcp\n  agentkanban-mcp --call <task_list|task_upsert|task_archive> --input-file <JSON-file|->\n  agentkanban-mcp --help\n  agentkanban-mcp --version\n\nWithout arguments: local stdio MCP server.\n--call: one tool call, with its arguments object from a UTF-8 file or stdin (-).\nInput limit: 1 MiB; a UTF-8 BOM is accepted. Success prints the tool result JSON;\nfailure prints an error JSON and exits nonzero. Pause and version checks apply.\nData: %LOCALAPPDATA%\\AgentKanban\\agentkanban.sqlite3\nOverride the data directory with AGENTKANBAN_DATA_DIR.", env!("CARGO_PKG_VERSION"));
        }
        Mode::Stdio => {
            let db = kanban_core::Database::open_default()?;
            agentkanban_mcp::serve(db, io::stdin().lock(), io::stdout().lock())?;
        }
        Mode::Call { name, input } => {
            let arguments = if input == "-" {
                read_arguments(io::stdin().lock())?
            } else {
                read_arguments(File::open(PathBuf::from(input))?)?
            };
            let db = kanban_core::Database::open_default()?;
            let result = agentkanban_mcp::execute_tool_with_transport(
                &db,
                &name,
                arguments,
                kanban_core::SyncTransport::Cli,
            )?;
            write_json(&result)?;
        }
    }
    Ok(())
}
