//! One-click client setup: registers the MCP server and adds the short
//! tracking rule to the client's global instructions. Edits keep the rest of
//! each file, and the first edit of an existing file leaves `<name>.agentkanban.bak`.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const RULE: &str = include_str!("../../examples/codex-AGENTS-snippet.md");
/// Earlier rule texts pasted without markers; setup replaces them in place.
const LEGACY_RULES: [&str; 4] = [
    include_str!("legacy-rule-v05.md"),
    include_str!("legacy-rule-v04.md"),
    "## AgentKanban\n会修改文件的任务无需用户要求即自动记录到 AgentKanban（MCP 工具 task_list / task_upsert）：开工先 task_list(project_path) 查摘要，沿用相符 task_key，否则 task_upsert 新建 auto:<简短标识>，agent 填自己的名称，写 goal（做什么）、acceptance（用户怎么验收）和 steps 计划（更新时保留已完成的步骤）；摘要带 has_user_note 时先按 task_key 读取用户补充并照做；之后只在某步完成、真实受阻或全部完成时更新，不为单次修改或命令更新。问答、只读审查、用户说不用记时不记。回复里不必提看板操作。",
    "## AgentKanban\n会修改文件的任务无需用户要求即自动记录到 AgentKanban（MCP 工具 task_list / task_upsert）：开工先 task_list(project_path) 查摘要，沿用相符 task_key，否则 task_upsert 新建 auto:<简短标识>，写 goal（做什么）、acceptance（用户怎么验收）和 steps 计划；摘要带 has_user_note 时先按 task_key 读取用户补充并照做；之后只在某步完成、真实受阻或全部完成时更新，不为单次修改或命令更新。问答、只读审查、用户说不用记时不记。回复里不必提看板操作。",
];
const RULE_START: &str = "<!-- agentkanban:start -->";
const RULE_END: &str = "<!-- agentkanban:end -->";
const YAML_START: &str = "# >>> agentkanban (managed by AgentKanban)";
const YAML_END: &str = "# <<< agentkanban";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Codex,
    Claude,
    Cursor,
    OpenCode,
    Antigravity,
    Hermes,
    Dsh,
}

const CLIENTS: [(&str, &str, Kind); 7] = [
    ("codex", "Codex", Kind::Codex),
    ("claude", "Claude Code", Kind::Claude),
    ("cursor", "Cursor", Kind::Cursor),
    ("opencode", "OpenCode", Kind::OpenCode),
    ("antigravity", "Antigravity", Kind::Antigravity),
    ("hermes", "Hermes", Kind::Hermes),
    ("dsh", "DeepSeek Harness", Kind::Dsh),
];

/// What the client needs to start this board's MCP server.
pub(crate) struct Server {
    pub command: String,
    /// Only set for a non-default data directory (tests and isolated boards).
    pub env: BTreeMap<String, String>,
}

#[derive(Serialize)]
pub(crate) struct ClientStatus {
    id: &'static str,
    name: &'static str,
    detected: bool,
    /// "ok", "outdated" (launch settings differ), "missing" or "unreadable".
    mcp: &'static str,
    /// None when the client has no global instruction file.
    rules: Option<bool>,
    config_path: String,
    rules_path: Option<String>,
    /// The same entry for manual merging.
    manual: String,
}

struct Paths {
    detect: PathBuf,
    configs: Vec<PathBuf>,
    rules: Option<PathBuf>,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn env_dir(name: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}

fn paths(kind: Kind, home: &Path) -> Paths {
    match kind {
        Kind::Codex => {
            let dir = env_dir("CODEX_HOME", home.join(".codex"));
            Paths {
                configs: vec![dir.join("config.toml")],
                rules: Some(dir.join("AGENTS.md")),
                detect: dir,
            }
        }
        Kind::Claude => Paths {
            detect: home.join(".claude"),
            configs: vec![home.join(".claude.json")],
            rules: Some(home.join(".claude").join("CLAUDE.md")),
        },
        Kind::Cursor => Paths {
            detect: home.join(".cursor"),
            configs: vec![home.join(".cursor").join("mcp.json")],
            rules: None,
        },
        Kind::OpenCode => {
            let dir = home.join(".config").join("opencode");
            let jsonc = dir.join("opencode.jsonc");
            let json = dir.join("opencode.json");
            Paths {
                configs: vec![if !json.exists() && jsonc.exists() {
                    jsonc
                } else {
                    json
                }],
                rules: Some(dir.join("AGENTS.md")),
                detect: dir,
            }
        }
        Kind::Antigravity => {
            let gemini = home.join(".gemini");
            // Current releases share ~/.gemini/config; older ones read the antigravity folder.
            let mut configs = vec![gemini.join("config").join("mcp_config.json")];
            if gemini.join("antigravity").is_dir() {
                configs.push(gemini.join("antigravity").join("mcp_config.json"));
            }
            Paths {
                detect: gemini.join("antigravity"),
                configs,
                rules: Some(gemini.join("GEMINI.md")),
            }
        }
        Kind::Hermes => {
            let dir = env_dir("HERMES_HOME", home.join(".hermes"));
            Paths {
                detect: dir.join("config.yaml"),
                configs: vec![dir.join("config.yaml")],
                rules: None,
            }
        }
        Kind::Dsh => {
            let dir = env_dir("DSH_HOME", home.join(".dsh"));
            Paths {
                configs: vec![dir.join("cordis.patch.yml")],
                rules: Some(dir.join("AGENTS.md")),
                detect: dir,
            }
        }
    }
}

/// Windows paths match regardless of slash direction and letter case.
fn same_path(a: &str, b: &str) -> bool {
    let normalize = |path: &str| path.replace('/', "\\").to_lowercase();
    normalize(a) == normalize(b)
}

fn find(id: &str) -> Result<(&'static str, &'static str, Kind), String> {
    CLIENTS
        .into_iter()
        .find(|(client, _, _)| *client == id)
        .ok_or_else(|| format!("未知客户端：{id}"))
}

pub(crate) fn statuses(server: &Server) -> Vec<ClientStatus> {
    let home = home();
    CLIENTS
        .into_iter()
        .map(|(id, name, kind)| status(id, name, kind, server, &home))
        .collect()
}

fn status(
    id: &'static str,
    name: &'static str,
    kind: Kind,
    server: &Server,
    home: &Path,
) -> ClientStatus {
    let paths = paths(kind, home);
    status_files(id, name, kind, server, &paths)
}

fn status_files(
    id: &'static str,
    name: &'static str,
    kind: Kind,
    server: &Server,
    paths: &Paths,
) -> ClientStatus {
    // Every config file of a client must point at this board.
    let mut mcp = "ok";
    for path in &paths.configs {
        let state = match fs::read_to_string(path) {
            Ok(text) => match configured_server(kind, &text) {
                Ok(Some(entry)) if entry.matches(server) => "ok",
                Ok(Some(_)) => "outdated",
                Ok(None) => "missing",
                Err(_) => "unreadable",
            },
            Err(_) => "missing",
        };
        if state != "ok" {
            mcp = state;
            break;
        }
    }
    let rules = paths.rules.as_ref().map(|path| {
        fs::read_to_string(path)
            .is_ok_and(|text| normalize_newlines(&text).contains(normalize_newlines(RULE).trim()))
    });
    ClientStatus {
        id,
        name,
        detected: paths.detect.exists() || paths.configs.iter().any(|path| path.exists()),
        mcp,
        rules,
        config_path: paths.configs[0].display().to_string(),
        rules_path: paths.rules.as_ref().map(|path| path.display().to_string()),
        manual: manual(kind, server),
    }
}

/// Writes the MCP entry and, where the client has one, the global rule.
pub(crate) fn setup(id: &str, server: &Server) -> Result<ClientStatus, String> {
    let (id, name, kind) = find(id)?;
    let home = home();
    let paths = paths(kind, &home);
    setup_files(kind, server, &paths)?;
    Ok(status_files(id, name, kind, server, &paths))
}

fn setup_files(kind: Kind, server: &Server, paths: &Paths) -> Result<(), String> {
    // Prepare every edit before touching a file. Unsupported configuration must
    // not leave half an integration behind. Claude uses the same backed-up JSON
    // merge as the other clients; there is no destructive remove/add CLI gap.
    let mut edits = Vec::new();
    for path in &paths.configs {
        let current = read_optional(path)?;
        let next = edit_config(kind, &current, server)
            .map_err(|err| format!("{}：{err}，请展开「手动配置」自行合并", path.display()))?;
        edits.push((path, current, next));
    }
    if let Some(path) = &paths.rules {
        let current = read_optional(path)?;
        let next = with_rule(&current);
        edits.push((path, current, next));
    }
    for (path, current, next) in edits {
        write_with_backup(path, &current, &next)?;
    }
    Ok(())
}

fn read_optional(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(format!("无法读取 {}：{err}", path.display())),
    }
}

fn write_with_backup(path: &Path, current: &str, next: &str) -> Result<(), String> {
    if current == next {
        return Ok(());
    }
    let fail = |err: std::io::Error| format!("无法写入 {}：{err}", path.display());
    let unchanged = || -> Result<(), String> {
        if read_optional(path)? != current {
            return Err(format!("{} 已被其他程序修改，请重试接入", path.display()));
        }
        Ok(())
    };
    unchanged()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    if path.exists() {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".agentkanban.bak");
        let backup = PathBuf::from(backup);
        // Keep the very first original; later runs only change our own entry.
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)
        {
            Ok(mut file) => {
                if let Err(err) = file.write_all(current.as_bytes()) {
                    drop(file);
                    let _ = fs::remove_file(&backup);
                    return Err(fail(err));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && backup.is_file() => {}
            Err(err) => return Err(fail(err)),
        }
    }
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(
        ".agentkanban-{}-{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|err| err.to_string())?
            .as_nanos()
    ));
    let temp = PathBuf::from(temp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(fail)?;
    if let Err(err) = file.write_all(next.as_bytes()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(fail(err));
    }
    drop(file);
    if let Err(err) = unchanged() {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }
    fs::rename(&temp, path).map_err(|err| {
        let _ = fs::remove_file(&temp);
        fail(err)
    })
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn with_rule(text: &str) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let block = format!(
        "{RULE_START}\n{}\n{RULE_END}",
        normalize_newlines(RULE).trim()
    )
    .replace('\n', newline);
    if let (Some(start), Some(end)) = (text.find(RULE_START), text.find(RULE_END)) {
        if start < end {
            return format!("{}{block}{}", &text[..start], &text[end + RULE_END.len()..]);
        }
    }
    for known in std::iter::once(RULE).chain(LEGACY_RULES) {
        let known = normalize_newlines(known);
        for variant in [known.trim().to_string(), known.trim().replace('\n', "\r\n")] {
            if text.contains(&variant) {
                return text.replacen(&variant, &block, 1);
            }
        }
    }
    let body = text.trim_end();
    if body.is_empty() {
        format!("{block}{newline}")
    } else {
        format!("{body}{newline}{newline}{block}{newline}")
    }
}

#[derive(Debug)]
struct ConfiguredServer {
    command: String,
    args: Vec<String>,
    env: BTreeMap<String, String>,
    enabled: bool,
}

impl ConfiguredServer {
    fn matches(&self, server: &Server) -> bool {
        self.enabled
            && same_path(&self.command, &server.command)
            && self.args.is_empty()
            && self.env.len() == server.env.len()
            && server.env.iter().all(|(key, expected)| {
                self.env.get(key).is_some_and(|actual| {
                    if key == "AGENTKANBAN_DATA_DIR" {
                        same_path(actual, expected)
                    } else {
                        actual == expected
                    }
                })
            })
    }
}

fn configured_server(kind: Kind, text: &str) -> Result<Option<ConfiguredServer>, String> {
    let entry = match kind {
        Kind::Codex => {
            let value: toml::Value = toml::from_str(text).map_err(|err| err.to_string())?;
            value
                .get("mcp_servers")
                .and_then(|servers| servers.get("agentkanban"))
                .map(serde_json::to_value)
                .transpose()
                .map_err(|err| err.to_string())?
        }
        Kind::Hermes => {
            hermes_servers(text)?.and_then(|section| section.value.get("agentkanban").cloned())
        }
        Kind::Dsh => dsh_server(text)?,
        Kind::OpenCode => {
            let value = parse_json(text)?;
            value.pointer("/mcp/agentkanban").cloned()
        }
        Kind::Claude | Kind::Cursor | Kind::Antigravity => {
            let value = parse_json(text)?;
            value.pointer("/mcpServers/agentkanban").cloned()
        }
    };
    entry.map(|entry| parse_server(kind, &entry)).transpose()
}

fn string_array(value: &Value) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or("启动参数必须是数组")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_string)
                .ok_or_else(|| "启动参数必须是字符串".into())
        })
        .collect()
}

fn parse_server(kind: Kind, entry: &Value) -> Result<ConfiguredServer, String> {
    let entry = entry.as_object().ok_or("MCP 配置必须是对象")?;
    let (command, args) = if kind == Kind::OpenCode {
        let mut command = string_array(entry.get("command").ok_or("缺少 command")?)?.into_iter();
        (command.next().ok_or("command 不能为空")?, command.collect())
    } else {
        let command = entry
            .get("command")
            .and_then(Value::as_str)
            .ok_or("command 必须是字符串")?;
        let args = entry
            .get("args")
            .map(string_array)
            .transpose()?
            .unwrap_or_default();
        (command.to_string(), args)
    };
    let env_key = if kind == Kind::OpenCode {
        "environment"
    } else {
        "env"
    };
    let mut env = BTreeMap::new();
    if let Some(value) = entry.get(env_key) {
        for (key, value) in value.as_object().ok_or("环境变量必须是对象")? {
            env.insert(
                key.clone(),
                value.as_str().ok_or("环境变量必须是字符串")?.to_string(),
            );
        }
    }
    let boolean = |key: &str, default: bool| -> Result<bool, String> {
        entry
            .get(key)
            .map(|value| value.as_bool().ok_or_else(|| format!("{key} 必须是布尔值")))
            .unwrap_or(Ok(default))
    };
    let expected_type = if kind == Kind::OpenCode {
        "local"
    } else {
        "stdio"
    };
    let transport = if kind == Kind::Dsh {
        "transport"
    } else {
        "type"
    };
    let correct_transport = entry
        .get(transport)
        .is_none_or(|value| value.as_str() == Some(expected_type));
    Ok(ConfiguredServer {
        command,
        args,
        env,
        enabled: boolean("enabled", true)? && !boolean("disabled", false)? && correct_transport,
    })
}

fn edit_config(kind: Kind, text: &str, server: &Server) -> Result<String, String> {
    let next = match kind {
        Kind::Codex => edit_codex(text, &codex_block(server)?),
        Kind::Hermes => edit_hermes(text, server)?,
        Kind::Dsh => edit_dsh(text, &dsh_block_text(server))?,
        Kind::OpenCode => edit_json(text, "mcp", opencode_entry(server))?,
        Kind::Claude | Kind::Cursor | Kind::Antigravity => {
            edit_json(text, "mcpServers", json_entry(kind, server))?
        }
    };
    // Never leave a file we cannot read back as configured.
    match configured_server(kind, &next) {
        Ok(Some(entry)) if entry.matches(server) => Ok(next),
        _ => Err("自动合并后的配置校验失败".into()),
    }
}

fn manual(kind: Kind, server: &Server) -> String {
    match kind {
        Kind::Codex => codex_block(server).unwrap_or_default(),
        Kind::Hermes => format!("mcp_servers:\n{}", hermes_entry("  ", server)),
        Kind::Dsh => dsh_block_text(server),
        Kind::OpenCode => pretty(&json!({"mcp": {"agentkanban": opencode_entry(server)}})),
        Kind::Claude | Kind::Cursor | Kind::Antigravity => {
            pretty(&json!({"mcpServers": {"agentkanban": json_entry(kind, server)}}))
        }
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

fn json_entry(kind: Kind, server: &Server) -> Value {
    let mut entry = Map::new();
    if kind != Kind::Antigravity {
        entry.insert("type".into(), json!("stdio"));
    }
    entry.insert("command".into(), json!(server.command));
    entry.insert("args".into(), json!([]));
    if !server.env.is_empty() {
        entry.insert("env".into(), json!(server.env));
    }
    Value::Object(entry)
}

fn opencode_entry(server: &Server) -> Value {
    let mut entry = json!({"type": "local", "command": [server.command], "enabled": true});
    if !server.env.is_empty() {
        entry["environment"] = json!(server.env);
    }
    entry
}

fn parse_json(text: &str) -> Result<Value, String> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text).map_err(|err| format!("无法解析 JSON（可能含注释）：{err}"))
}

fn edit_json(text: &str, section: &str, entry: Value) -> Result<String, String> {
    let mut value = parse_json(text)?;
    let root = value.as_object_mut().ok_or("配置文件顶层不是对象")?;
    let servers = root
        .entry(section)
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{section} 不是对象"))?;
    servers.insert("agentkanban".into(), entry);
    Ok(format!("{}\n", pretty(&value)))
}

fn codex_block(server: &Server) -> Result<String, String> {
    #[derive(Serialize)]
    struct Entry<'a> {
        command: &'a str,
        args: [&'a str; 0],
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        env: &'a BTreeMap<String, String>,
    }
    let entry = Entry {
        command: &server.command,
        args: [],
        env: &server.env,
    };
    toml::to_string_pretty(&BTreeMap::from([(
        "mcp_servers",
        BTreeMap::from([("agentkanban", entry)]),
    )]))
    .map_err(|err| err.to_string())
}

/// Replaces our own tables in place and leaves comments and other tables untouched.
fn edit_codex(text: &str, block: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let ours = |line: &str| {
        let line = line.trim();
        [
            "[mcp_servers.agentkanban]",
            "[mcp_servers.\"agentkanban\"]",
            "[mcp_servers.'agentkanban']",
        ]
        .contains(&line)
            || line.starts_with("[mcp_servers.agentkanban.")
    };
    let Some(start) = lines.iter().position(|line| ours(line)) else {
        let body = text.trim_end();
        return if body.is_empty() {
            block.to_string()
        } else {
            format!("{body}\n\n{block}")
        };
    };
    let end = lines[start + 1..]
        .iter()
        .position(|line| line.trim_start().starts_with('[') && !ours(line))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let mut out: Vec<String> = lines[..start].iter().map(|line| line.to_string()).collect();
    out.extend(block.trim_end().lines().map(str::to_owned));
    if end < lines.len() {
        out.push(String::new());
        out.extend(lines[end..].iter().map(|line| line.to_string()));
    }
    format!("{}\n", out.join("\n"))
}

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

// Deliberately limited YAML reader: block maps, scalar lists, single-line
// scalars and JSON-compatible flow values. Refuse aliases and ambiguous YAML rather
// than guessing and overwriting a client's configuration.
fn yaml_value(value: &str) -> Result<Value, String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')) {
        let mut chars = inner.chars();
        while let Some(ch) = chars.next() {
            if ch == '\'' && chars.next() != Some('\'') {
                return Err("不支持的 YAML 引号写法".into());
            }
        }
        Ok(json!(inner.replace("''", "'")))
    } else if value.starts_with('"') || value.starts_with('[') || value.starts_with('{') {
        serde_json::from_str(value).map_err(|_| "复杂 YAML 需手动合并".into())
    } else {
        match value {
            "true" => Ok(json!(true)),
            "false" => Ok(json!(false)),
            "" | "null" | "~" => Ok(Value::Null),
            _ if value.starts_with(['\'', '&', '*', '!', '|', '>', '%', '@', '`'])
                || value.contains(": ") =>
            {
                Err("复杂 YAML 需手动合并".into())
            }
            _ => Ok(json!(value)),
        }
    }
}

#[derive(Clone)]
struct YamlLine {
    line: usize,
    depth: usize,
    body: String,
}

fn yaml_lines(text: &str) -> Result<Vec<YamlLine>, String> {
    let mut out = Vec::new();
    for (line, raw) in text.lines().enumerate() {
        let body = raw.trim_start();
        if body.is_empty() || body.starts_with('#') {
            continue;
        }
        let indent = &raw[..raw.len() - body.len()];
        if !indent.chars().all(|ch| ch == ' ')
            || matches!(body, "---" | "...")
            || body.starts_with('%')
        {
            return Err("不支持的 YAML 缩进或文档标记，请手动合并".into());
        }
        let mut quote = None;
        let mut escaped = false;
        let mut end = body.len();
        for (offset, ch) in body.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if quote == Some('"') && ch == '\\' {
                escaped = true;
                continue;
            }
            if Some(ch) == quote {
                quote = None;
            } else if quote.is_none() && matches!(ch, '\'' | '"') {
                quote = Some(ch);
            } else if quote.is_none()
                && ch == '#'
                && (offset == 0 || body[..offset].ends_with(char::is_whitespace))
            {
                end = offset;
                break;
            }
        }
        if quote.is_some() {
            return Err("多行 YAML 引号需手动合并".into());
        }
        out.push(YamlLine {
            line,
            depth: indent.len(),
            body: body[..end].trim_end().to_string(),
        });
    }
    Ok(out)
}

fn yaml_pair(body: &str) -> Result<(String, &str), String> {
    let (key, value) = body.split_once(':').ok_or("YAML 不是块映射，请手动合并")?;
    if !value.is_empty() && !value.starts_with(char::is_whitespace) {
        return Err("YAML 键值缺少分隔空格".into());
    }
    let key = yaml_value(key.trim())?
        .as_str()
        .ok_or("YAML 键必须是字符串")?
        .to_string();
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err("复杂 YAML 键需手动合并".into());
    }
    Ok((key, value.trim()))
}

fn yaml_block(lines: &[YamlLine]) -> Result<Value, String> {
    if lines.is_empty() {
        return Ok(Value::Null);
    }
    let depth = lines[0].depth;
    let sequence = lines[0].body.starts_with("- ") || lines[0].body == "-";
    let mut map = Map::new();
    let mut array = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        if line.depth != depth {
            return Err("YAML 缩进不一致，请手动合并".into());
        }
        let end = (index + 1..lines.len())
            .find(|&i| lines[i].depth <= depth)
            .unwrap_or(lines.len());
        let (key, value) = if sequence {
            let value = line
                .body
                .strip_prefix("- ")
                .or_else(|| (line.body == "-").then_some(""))
                .ok_or("YAML 列表与映射混用")?;
            (String::new(), value)
        } else {
            yaml_pair(&line.body)?
        };
        let nested = &lines[index + 1..end];
        let value = if sequence && yaml_pair(value).is_ok() {
            // A sequence item may begin a block map, e.g. DSH's `- insert:`.
            let mut item = vec![YamlLine {
                line: line.line,
                depth: depth + 2,
                body: value.to_string(),
            }];
            item.extend_from_slice(nested);
            yaml_block(&item)?
        } else if value.is_empty() {
            yaml_block(nested)?
        } else {
            if !nested.is_empty() {
                return Err("YAML 标量下出现额外缩进，请手动合并".into());
            }
            yaml_value(value)?
        };
        if sequence {
            array.push(value);
        } else if map.insert(key, value).is_some() {
            return Err("YAML 存在重复键，请手动合并".into());
        }
        index = end;
    }
    Ok(if sequence {
        Value::Array(array)
    } else {
        Value::Object(map)
    })
}

fn yaml_env(indent: &str, server: &Server) -> String {
    if server.env.is_empty() {
        return String::new();
    }
    let mut out = format!("{indent}env:\n");
    for (key, value) in &server.env {
        out.push_str(&format!("{indent}  {key}: {}\n", yaml_quote(value)));
    }
    out
}

fn hermes_entry(indent: &str, server: &Server) -> String {
    let inner = format!("{indent}  ");
    format!(
        "{indent}agentkanban:\n{inner}command: {}\n{inner}args: []\n{}",
        yaml_quote(&server.command),
        yaml_env(&inner, server)
    )
}

struct HermesSection {
    key: usize,
    depth: usize,
    child: Option<(usize, usize)>,
    value: Value,
}

fn hermes_servers(text: &str) -> Result<Option<HermesSection>, String> {
    let lines = yaml_lines(text)?;
    let mut found = None;
    for (index, line) in lines.iter().enumerate().filter(|(_, line)| line.depth == 0) {
        let (key, value) = yaml_pair(&line.body)?;
        if key != "mcp_servers" {
            continue;
        }
        if found.is_some() {
            return Err("重复的 mcp_servers，请手动合并".into());
        }
        let end = (index + 1..lines.len())
            .find(|&i| lines[i].depth == 0)
            .unwrap_or(lines.len());
        let children = &lines[index + 1..end];
        if !["", "{}", "null", "~"].contains(&value) || (!value.is_empty() && !children.is_empty())
        {
            return Err("mcp_servers 使用了无法自动合并的写法".into());
        }
        let value = if children.is_empty() {
            json!({})
        } else {
            yaml_block(children)?
        };
        let servers = value.as_object().ok_or("mcp_servers 必须是块映射")?;
        if servers.values().any(|server| !server.is_object()) {
            return Err("MCP 服务器必须是块映射，请手动合并".into());
        }
        let depth = children.first().map_or(2, |line| line.depth);
        // Only a direct child can be our server. env.agentkanban belongs to its
        // parent server and is preserved along with every other nested field.
        let child = children
            .iter()
            .enumerate()
            .find(|(_, line)| {
                line.depth == depth
                    && yaml_pair(&line.body).is_ok_and(|(key, _)| key == "agentkanban")
            })
            .map(|(start, line)| {
                let child_end = children[start + 1..]
                    .iter()
                    .find(|next| next.depth == depth)
                    .map(|next| next.line)
                    .unwrap_or_else(|| {
                        lines
                            .get(end)
                            .map_or(text.lines().count(), |next| next.line)
                    });
                (line.line, child_end)
            });
        found = Some(HermesSection {
            key: line.line,
            depth,
            child,
            value,
        });
    }
    if lines.first().is_some_and(|line| line.depth != 0) {
        return Err("YAML 顶层缩进需手动合并".into());
    }
    Ok(found)
}

fn edit_hermes(text: &str, server: &Server) -> Result<String, String> {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let Some(section) = hermes_servers(text)? else {
        let body = text.trim_end();
        let entry = format!("mcp_servers:\n{}", hermes_entry("  ", server));
        return Ok(if body.is_empty() {
            entry
        } else {
            format!("{body}\n\n{entry}")
        });
    };
    lines[section.key] = "mcp_servers:".into();
    let indent = " ".repeat(section.depth);
    let entry: Vec<String> = hermes_entry(&indent, server)
        .lines()
        .map(str::to_owned)
        .collect();
    match section.child {
        Some((start, child_end)) => {
            lines.splice(start..child_end, entry);
        }
        None => {
            lines.splice(section.key + 1..section.key + 1, entry);
        }
    }
    Ok(format!("{}\n", lines.join("\n")))
}

fn dsh_block_text(server: &Server) -> String {
    format!(
        "{YAML_START}\n- insert:\n    - id: agentkanban-mcp\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: agentkanban\n        transport: stdio\n        command: {}\n        args: []\n{}{YAML_END}\n",
        yaml_quote(&server.command),
        yaml_env("        ", server)
    )
}

fn dsh_block(text: &str) -> Result<Option<(usize, usize)>, String> {
    let starts: Vec<_> = text
        .match_indices(YAML_START)
        .map(|(offset, _)| offset)
        .collect();
    let ends: Vec<_> = text
        .match_indices(YAML_END)
        .map(|(offset, _)| offset)
        .collect();
    if starts.is_empty() && ends.is_empty() {
        return Ok(None);
    }
    if starts.len() != 1
        || ends.len() != 1
        || starts[0] >= ends[0]
        || [starts[0], ends[0]]
            .iter()
            .any(|&offset| offset > 0 && !text[..offset].ends_with('\n'))
    {
        return Err("AgentKanban YAML 标记不完整或重复，请手动合并".into());
    }
    Ok(Some((starts[0], ends[0] + YAML_END.len())))
}

fn check_unmanaged_dsh(text: &str, block: Option<(usize, usize)>) -> Result<(), String> {
    let remainder = block.map_or_else(
        || text.to_string(),
        |(start, end)| format!("{}{}", &text[..start], &text[end..]),
    );
    let lines = yaml_lines(&remainder)?;
    if lines.is_empty() {
        return Ok(());
    }
    let value = if lines.len() == 1 && lines[0].depth == 0 && lines[0].body.starts_with('[') {
        yaml_value(&lines[0].body)?
    } else {
        yaml_block(&lines)?
    };
    if lines[0].depth != 0
        || !value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_object))
    {
        return Err("cordis.patch.yml 不是补丁对象列表，请手动合并".into());
    }
    fn has_server(value: &Value) -> bool {
        match value {
            Value::Object(map) => {
                map.get("id").and_then(Value::as_str) == Some("agentkanban-mcp")
                    || map.get("serverName").and_then(Value::as_str) == Some("agentkanban")
                    || map.values().any(has_server)
            }
            Value::Array(items) => items.iter().any(has_server),
            _ => false,
        }
    }
    if has_server(&value) {
        return Err("已存在未标记的 AgentKanban 配置，请手动合并已有条目，避免重复注册".into());
    }
    Ok(())
}

fn dsh_server(text: &str) -> Result<Option<Value>, String> {
    let block = dsh_block(text)?;
    check_unmanaged_dsh(text, block)?;
    let Some((start, end)) = block else {
        return Ok(None);
    };
    let lines = yaml_lines(&text[start..end])?;
    let expected = [
        (0, "- insert:"),
        (4, "- id: agentkanban-mcp"),
        (6, "name: '@deepseek-ai/dsh-mcp-client'"),
        (6, "config:"),
    ];
    if lines.len() < 5
        || expected
            .iter()
            .zip(&lines)
            .any(|((depth, body), line)| line.depth != *depth || line.body != *body)
        || lines[4].depth != 8
    {
        return Err("AgentKanban 补丁结构已变化，请手动检查".into());
    }
    Ok(Some(yaml_block(&lines[4..])?))
}

/// The patch file is a YAML list of patches; ours is one marked item.
fn edit_dsh(text: &str, block: &str) -> Result<String, String> {
    let existing = dsh_block(text)?;
    check_unmanaged_dsh(text, existing)?;
    if let Some((start, end)) = existing {
        let tail = text[end..]
            .strip_prefix("\r\n")
            .or_else(|| text[end..].strip_prefix('\n'))
            .unwrap_or(&text[end..]);
        return Ok(format!("{}{block}{tail}", &text[..start]));
    }
    let body = text.trim_end();
    let first = body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'));
    match first {
        None => Ok(if body.is_empty() {
            block.to_string()
        } else {
            format!("{body}\n{block}")
        }),
        Some("[]") => Ok(block.to_string()),
        Some(line) if line.starts_with('-') => Ok(format!("{body}\n\n{block}")),
        Some(_) => Err("cordis.patch.yml 不是补丁列表".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            command: "C:\\中文 目录\\O'Brien\\agentkanban-mcp.exe".into(),
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn every_format_round_trips_quotes_unicode_and_the_data_override() {
        let mut server = server();
        server
            .env
            .insert("AGENTKANBAN_DATA_DIR".into(), "C:\\数据\\a'b\"c".into());
        for (_, _, kind) in CLIENTS {
            let written = edit_config(kind, "", &server).unwrap();
            assert!(configured_server(kind, &written)
                .unwrap()
                .unwrap()
                .matches(&server));
            assert!(manual(kind, &server).contains("agentkanban"));
        }
        let codex: toml::Value = toml::from_str(&codex_block(&server).unwrap()).unwrap();
        assert_eq!(
            codex["mcp_servers"]["agentkanban"]["env"]["AGENTKANBAN_DATA_DIR"].as_str(),
            Some("C:\\数据\\a'b\"c")
        );
        let cursor: Value =
            serde_json::from_str(&edit_config(Kind::Cursor, "", &server).unwrap()).unwrap();
        assert_eq!(
            cursor["mcpServers"]["agentkanban"]["env"]["AGENTKANBAN_DATA_DIR"],
            "C:\\数据\\a'b\"c"
        );
    }

    #[test]
    fn edits_keep_other_settings_and_replace_only_our_entry() {
        let server = server();
        let codex = "# mine\nmodel = \"x\"\n\n[mcp_servers.agentkanban]\ncommand = 'old.exe'\nargs = []\n\n[mcp_servers.agentkanban.env]\nA = '1'\n\n[mcp_servers.other]\ncommand = 'other'\n";
        let edited = edit_config(Kind::Codex, codex, &server).unwrap();
        assert!(edited.starts_with("# mine\nmodel = \"x\"\n"));
        assert!(edited.contains("[mcp_servers.other]\ncommand = 'other'"));
        assert!(!edited.contains("old.exe") && !edited.contains("A = '1'"));
        assert_eq!(edit_config(Kind::Codex, &edited, &server).unwrap(), edited);

        let hermes = "model:\n  default: x\nmcp_servers:\n    time:\n        command: uvx\n    agentkanban:\n        command: 'old.exe'\n        args: []\nterminal:\n  backend: local\n";
        let edited = edit_config(Kind::Hermes, hermes, &server).unwrap();
        assert!(edited.contains("    time:\n        command: uvx\n"));
        assert!(edited.contains(
            "    agentkanban:\n      command: 'C:\\中文 目录\\O''Brien\\agentkanban-mcp.exe'"
        ));
        assert!(!edited.contains("old.exe"));
        assert!(edited.ends_with("terminal:\n  backend: local\n"));
        let appended = edit_config(Kind::Hermes, "model:\n  default: x\n", &server).unwrap();
        assert!(appended.starts_with("model:\n  default: x\n\nmcp_servers:\n  agentkanban:\n"));
        assert!(edit_config(Kind::Hermes, "mcp_servers: {a: 1}\n", &server).is_err());

        let dsh = "- id: llm\n  config: {}\n";
        let edited = edit_config(Kind::Dsh, dsh, &server).unwrap();
        assert!(edited.starts_with(dsh));
        assert_eq!(edit_config(Kind::Dsh, &edited, &server).unwrap(), edited);
        assert!(edit_config(Kind::Dsh, "key: value\n", &server).is_err());

        let json = "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {\"other\": {\"type\": \"remote\"}}\n}";
        let edited: Value =
            serde_json::from_str(&edit_config(Kind::OpenCode, json, &server).unwrap()).unwrap();
        assert_eq!(edited["$schema"], "https://opencode.ai/config.json");
        assert_eq!(edited["mcp"]["other"]["type"], "remote");
        assert!(edit_config(Kind::Cursor, "// comment\n{}", &server).is_err());
    }

    #[test]
    fn rules_are_added_once_and_legacy_copies_are_adopted() {
        let added = with_rule("# Mine\n");
        assert!(added.starts_with("# Mine\n\n<!-- agentkanban:start -->"));
        assert_eq!(with_rule(&added), added);
        let legacy = format!("# Mine\n\n{}\n\n# After\n", RULE.trim());
        assert!(same_path(
            "C:/Users/A/agentkanban-mcp.exe",
            r"c:\users\a\AgentKanban-MCP.exe"
        ));
        let adopted = with_rule(&legacy);
        assert_eq!(adopted.matches(RULE.trim()).count(), 1);
        assert!(adopted.contains(RULE_START) && adopted.ends_with("# After\n"));
        let old = format!("# Mine\n\n{}\n", LEGACY_RULES[0].trim());
        let upgraded = with_rule(&old);
        assert!(!upgraded.contains(LEGACY_RULES[0].trim()) && upgraded.contains(RULE.trim()));
        let stale = format!("{RULE_START}\n{}\n{RULE_END}\n", LEGACY_RULES[0].trim());
        assert_eq!(with_rule(&stale).matches(RULE_START).count(), 1);
        assert!(with_rule(&stale).contains(RULE.trim()));
        for known in LEGACY_RULES {
            let old = format!(
                "# Mine\r\n\r\n{}\r\n\r\n# After\r\n",
                normalize_newlines(known).trim().replace('\n', "\r\n")
            );
            let upgraded = with_rule(&old);
            assert_eq!(upgraded.matches("## AgentKanban").count(), 1);
            assert!(
                upgraded.starts_with("# Mine\r\n\r\n") && upgraded.ends_with("\r\n\r\n# After\r\n")
            );
            assert_eq!(with_rule(&upgraded), upgraded);
        }
    }

    fn temporary_home(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "agentkanban-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_paths(root: &Path) -> Paths {
        Paths {
            detect: root.to_path_buf(),
            configs: vec![root.join("config")],
            rules: Some(root.join("RULES.md")),
        }
    }

    #[test]
    fn setup_writes_each_client_under_a_fake_home_and_backs_up_originals() {
        let home = temporary_home("clients");
        let server = server();
        for (id, name, kind) in CLIENTS {
            let paths = test_paths(&home.join(id));
            setup_files(kind, &server, &paths).unwrap();
            let status = status_files(id, name, kind, &server, &paths);
            assert_eq!(status.mcp, "ok", "{id}");
            assert_ne!(status.rules, Some(false), "{id}");
            assert!(status.detected, "{id}");
            let original = fs::read_to_string(&paths.configs[0]).unwrap();
            let changed = Server {
                command: "C:\\new.exe".into(),
                env: BTreeMap::new(),
            };
            setup_files(kind, &changed, &paths).unwrap();
            assert_eq!(
                fs::read_to_string(home.join(id).join("config.agentkanban.bak")).unwrap(),
                original
            );
            setup_files(kind, &server, &paths).unwrap();
            assert_eq!(
                fs::read_to_string(home.join(id).join("config.agentkanban.bak")).unwrap(),
                original
            );
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn nested_hermes_keys_are_preserved_and_ambiguous_yaml_is_rejected_before_writing() {
        let original = "mcp_servers:\n  other:\n    command: uvx\n    env:\n      agentkanban: secret-for-other\n      REST: preserve\n";
        let next = edit_config(Kind::Hermes, original, &server()).unwrap();
        let parsed = hermes_servers(&next).unwrap().unwrap().value;
        assert_eq!(
            parsed["other"]["env"],
            json!({"agentkanban":"secret-for-other","REST":"preserve"})
        );
        assert!(configured_server(Kind::Hermes, &next)
            .unwrap()
            .unwrap()
            .matches(&server()));
        assert_eq!(edit_config(Kind::Hermes, &next, &server()).unwrap(), next);
        for bad in [
            "mcp_servers: &servers\n  other:\n    command: uvx\n",
            "mcp_servers:\n  agentkanban: *server\n",
            "mcp_servers:\n  other:\n    command: uvx\n   agentkanban:\n    command: old\n",
            "mcp_servers: {}\nmcp_servers: {}\n",
            "mcp_servers:\n  agentkanban:\n    command: old\n    command: other\n",
        ] {
            let home = temporary_home("hermes-refuse");
            let paths = test_paths(&home);
            fs::write(&paths.configs[0], bad).unwrap();
            assert!(setup_files(Kind::Hermes, &server(), &paths).is_err());
            assert_eq!(fs::read_to_string(&paths.configs[0]).unwrap(), bad);
            assert!(
                !home.join("config.agentkanban.bak").exists()
                    && !paths.rules.as_ref().unwrap().exists()
            );
            fs::remove_dir_all(home).unwrap();
        }
    }

    #[test]
    fn launch_diagnostics_include_arguments_environment_and_enabled_state() {
        let server = server();
        for (_, _, kind) in CLIENTS {
            let base = if kind == Kind::OpenCode {
                opencode_entry(&server)
            } else {
                json_entry(kind, &server)
            };
            assert!(parse_server(kind, &base).unwrap().matches(&server));
            for disabled in ["enabled", "disabled"] {
                let mut changed = base.clone();
                changed[disabled] = json!(disabled == "disabled");
                assert!(!parse_server(kind, &changed).unwrap().matches(&server));
            }
            let mut changed = base.clone();
            if kind == Kind::OpenCode {
                changed["command"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("--version"));
            } else {
                changed["args"] = json!(["--version"]);
            }
            assert!(!parse_server(kind, &changed).unwrap().matches(&server));
            let mut changed = base;
            changed[if kind == Kind::OpenCode {
                "environment"
            } else {
                "env"
            }] = json!({"AGENTKANBAN_DATA_DIR":"C:\\other-board"});
            assert!(!parse_server(kind, &changed).unwrap().matches(&server));
        }
        let root = temporary_home("diagnostics");
        let mut paths = test_paths(&root);
        paths.rules = None;
        fs::write(&paths.configs[0], pretty(&json!({"mcpServers":{"agentkanban":{
            "command":server.command,"args":["--version"],"disabled":true,"env":{"AGENTKANBAN_DATA_DIR":"C:\\other-board"}
        }}}))).unwrap();
        assert_eq!(
            status_files("cursor", "Cursor", Kind::Cursor, &server, &paths).mcp,
            "outdated"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unmarked_or_duplicate_dsh_entries_are_not_added_again() {
        let old = "- insert:\n    - id: agentkanban-mcp\n      name: '@deepseek-ai/dsh-mcp-client'\n      config:\n        serverName: agentkanban\n        command: old.exe\n";
        assert!(edit_config(Kind::Dsh, old, &server()).is_err());
        assert!(edit_config(Kind::Dsh, "- id: \"agentkanban\\u002dmcp\"\n", &server()).is_err());
        assert!(edit_config(
            Kind::Dsh,
            "- id: other\n  config: {}\n    invalid: nested\n",
            &server()
        )
        .is_err());
        let managed = dsh_block_text(&server());
        assert!(edit_config(Kind::Dsh, &format!("{old}\n{managed}"), &server()).is_err());
        assert!(edit_config(Kind::Dsh, &format!("{managed}{managed}"), &server()).is_err());
        assert_eq!(
            edit_config(Kind::Dsh, &managed.replace('\n', "\r\n"), &server())
                .unwrap()
                .matches("id: agentkanban-mcp")
                .count(),
            1
        );
    }

    #[test]
    fn claude_merge_keeps_other_fields_and_bad_input_never_loses_the_old_entry() {
        let root = temporary_home("claude-safe");
        let paths = test_paths(&root);
        let original = r#"{"theme":"dark","mcpServers":{"other":{"command":"keep.exe"},"agentkanban":{"command":"old.exe"}}}"#;
        fs::write(&paths.configs[0], original).unwrap();
        setup_files(Kind::Claude, &server(), &paths).unwrap();
        let merged = parse_json(&fs::read_to_string(&paths.configs[0]).unwrap()).unwrap();
        assert_eq!(merged["theme"], "dark");
        assert_eq!(merged["mcpServers"]["other"]["command"], "keep.exe");
        assert_eq!(
            fs::read_to_string(root.join("config.agentkanban.bak")).unwrap(),
            original
        );
        fs::write(&paths.configs[0], "invalid JSON with old config").unwrap();
        assert!(setup_files(Kind::Claude, &server(), &paths).is_err());
        assert_eq!(
            fs::read_to_string(&paths.configs[0]).unwrap(),
            "invalid JSON with old config"
        );
        // A running client editing after our read must not have its update replaced.
        assert!(write_with_backup(&paths.configs[0], original, "replacement").is_err());
        assert_eq!(
            fs::read_to_string(&paths.configs[0]).unwrap(),
            "invalid JSON with old config"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_secondary_configuration_leaves_all_files_untouched() {
        let root = temporary_home("multi-file-preflight");
        let mut paths = test_paths(&root);
        paths.configs.push(root.join("second-config"));
        fs::write(&paths.configs[0], "{\"other\":true}").unwrap();
        fs::write(&paths.configs[1], "broken JSON").unwrap();
        assert!(setup_files(Kind::Antigravity, &server(), &paths).is_err());
        assert_eq!(
            fs::read_to_string(&paths.configs[0]).unwrap(),
            "{\"other\":true}"
        );
        assert_eq!(
            fs::read_to_string(&paths.configs[1]).unwrap(),
            "broken JSON"
        );
        assert!(!root.join("config.agentkanban.bak").exists());
        assert!(!paths.rules.as_ref().unwrap().exists());
        fs::remove_dir_all(root).unwrap();
    }
}
