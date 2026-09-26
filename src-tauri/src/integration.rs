use crate::clients::Server;
use kanban_core::Database;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    time::timeout,
};

#[derive(Serialize)]
pub(crate) struct IntegrationInfo {
    app_version: String,
    mcp_path: String,
    mcp_exists: bool,
    database_path: String,
    last_task_update: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct McpCheck {
    pub ok: bool,
    pub message: String,
}

pub(crate) fn mcp_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|err| err.to_string())?;
    let directory = executable.parent().ok_or("无法定位应用所在目录")?;
    Ok(directory.join(if cfg!(windows) {
        "agentkanban-mcp.exe"
    } else {
        "agentkanban-mcp"
    }))
}

pub(crate) fn info(db: &Database, version: &str) -> Result<IntegrationInfo, String> {
    let executable = mcp_path()?;
    Ok(IntegrationInfo {
        app_version: version.to_string(),
        mcp_path: path_text(&executable)?,
        mcp_exists: executable.is_file(),
        database_path: path_text(db.path())?,
        last_task_update: db.last_task_update().map_err(|err| err.to_string())?,
    })
}

/// How clients should start this board's MCP server. A custom data directory
/// (tests, isolated boards) travels with the entry so both sides share one board.
pub(crate) fn server(db: &Database) -> Result<Server, String> {
    let mut env = BTreeMap::new();
    if std::env::var_os("AGENTKANBAN_DATA_DIR").is_some_and(|value| !value.is_empty()) {
        let directory = db.path().parent().ok_or("无法定位数据库目录")?;
        env.insert("AGENTKANBAN_DATA_DIR".to_string(), path_text(directory)?);
    }
    Ok(Server {
        command: path_text(&mcp_path()?)?,
        env,
    })
}

fn path_text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "路径包含无法表示的 Unicode 字符".to_string())
}

pub(crate) async fn check(executable: PathBuf, data_dir: PathBuf) -> McpCheck {
    match check_inner(&executable, &data_dir).await {
        Ok(()) => McpCheck { ok: true, message: "本地 MCP 自检通过：初始化及 3 个工具定义正常。此结果不代表任何 Agent 客户端已连接。".into() },
        Err(message) => McpCheck { ok: false, message },
    }
}

async fn check_inner(executable: &Path, data_dir: &Path) -> Result<(), String> {
    if !executable.is_file() {
        return Err(format!("未找到 MCP 可执行文件：{}", executable.display()));
    }
    let mut command = Command::new(executable);
    command
        .env("AGENTKANBAN_DATA_DIR", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW: self-check never opens a console.
    let mut child = command
        .spawn()
        .map_err(|err| format!("MCP 启动失败：{err}"))?;
    let mut input = child.stdin.take().ok_or("无法打开 MCP 标准输入")?;
    let mut output = BufReader::new(child.stdout.take().ok_or("无法打开 MCP 标准输出")?);
    let result = match timeout(Duration::from_secs(4), handshake(&mut input, &mut output)).await {
        Ok(result) => result,
        Err(_) => Err("MCP 自检超时（4 秒），已终止检查进程".into()),
    };
    drop(input);
    drop(output);
    // Reap on success, protocol failure and timeout. kill_on_drop is a final fallback
    // if the async command itself is cancelled before cleanup completes.
    let _ = child.start_kill();
    match timeout(Duration::from_secs(1), child.wait()).await {
        Ok(Ok(_)) => result,
        Ok(Err(err)) => Err(format!("MCP 检查进程回收失败：{err}")),
        Err(_) => Err("MCP 检查进程未在时限内退出".into()),
    }
}

async fn handshake(
    input: &mut ChildStdin,
    output: &mut BufReader<ChildStdout>,
) -> Result<(), String> {
    send(input, json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"AgentKanban desktop self-check","version":env!("CARGO_PKG_VERSION")}}})).await?;
    let initialized = read_response(output, 1).await?;
    if initialized.get("protocolVersion").and_then(Value::as_str) != Some("2025-11-25")
        || !initialized
            .get("capabilities")
            .and_then(|caps| caps.get("tools"))
            .is_some_and(Value::is_object)
    {
        return Err("MCP 初始化结果缺少预期协议或 tools 能力".into());
    }
    send(
        input,
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
    )
    .await?;
    send(
        input,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    )
    .await?;
    validate_tools(&read_response(output, 2).await?)
}

async fn send(input: &mut ChildStdin, value: Value) -> Result<(), String> {
    let mut encoded = value.to_string();
    encoded.push('\n');
    input
        .write_all(encoded.as_bytes())
        .await
        .map_err(|err| format!("MCP 请求发送失败：{err}"))?;
    input
        .flush()
        .await
        .map_err(|err| format!("MCP 请求刷新失败：{err}"))
}

async fn read_response(
    output: &mut BufReader<ChildStdout>,
    expected_id: i64,
) -> Result<Value, String> {
    let mut line = String::new();
    let count = output
        .read_line(&mut line)
        .await
        .map_err(|err| format!("MCP 响应读取失败：{err}"))?;
    if count == 0 {
        return Err("MCP 在返回响应前退出".into());
    }
    if count > 65_536 {
        return Err("MCP 响应超过自检大小限制".into());
    }
    let response: Value =
        serde_json::from_str(&line).map_err(|err| format!("MCP 返回的内容不是有效 JSON：{err}"))?;
    if response.get("jsonrpc") != Some(&json!("2.0"))
        || response.get("id") != Some(&json!(expected_id))
    {
        return Err("MCP 响应的协议或请求标识不匹配".into());
    }
    if let Some(error) = response.get("error") {
        return Err(format!("MCP 返回错误：{error}"));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| "MCP 响应缺少 result".into())
}

fn validate_tools(result: &Value) -> Result<(), String> {
    let tools = result
        .get("tools")
        .and_then(Value::as_array)
        .ok_or("MCP 工具列表格式无效")?;
    let mut names: Vec<_> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();
    names.sort_unstable();
    if tools.len() != 3
        || names != ["task_archive", "task_list", "task_upsert"]
        || tools.iter().any(|tool| {
            tool.get("inputSchema")
                .and_then(|schema| schema.get("type"))
                .and_then(Value::as_str)
                != Some("object")
        })
        || result
            .get("nextCursor")
            .is_some_and(|cursor| !cursor.is_null())
    {
        return Err(
            "MCP 工具定义不符合预期：应且仅应提供 task_upsert、task_list、task_archive".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_check_rejects_missing_duplicate_extra_and_malformed_tools() {
        let tools = json!([
            {"name":"task_upsert","inputSchema":{"type":"object"}},
            {"name":"task_list","inputSchema":{"type":"object"}},
            {"name":"task_archive","inputSchema":{"type":"object"}}
        ]);
        validate_tools(&json!({"tools":tools})).unwrap();
        assert!(validate_tools(&json!({"tools":[]})).is_err());
        let mut duplicate = tools.clone();
        duplicate[2]["name"] = json!("task_upsert");
        assert!(validate_tools(&json!({"tools":duplicate})).is_err());
        let mut malformed = tools.clone();
        malformed[0]["inputSchema"] = json!(null);
        assert!(validate_tools(&json!({"tools":malformed})).is_err());
        let mut extra = tools.clone();
        extra
            .as_array_mut()
            .unwrap()
            .push(json!({"name":"extra","inputSchema":{"type":"object"}}));
        assert!(validate_tools(&json!({"tools":extra})).is_err());
        assert!(validate_tools(&json!({"tools":tools,"nextCursor":"another-page"})).is_err());
    }
}
