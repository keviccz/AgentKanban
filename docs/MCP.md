# MCP 接入与任务契约

AgentKanban 提供一个独立的本地 stdio MCP 程序 `agentkanban-mcp.exe`，只有 `task_upsert`、`task_list` 和 `task_archive` 三个工具。它直接写入与浮窗共用的 SQLite；窗口不必保持运行。协议使用 UTF-8 的逐行 JSON-RPC 消息，标准输出只用于协议，诊断写入标准错误。[MCP stdio 规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

## 配置客户端

以下文件是合并示例，路径中的 `YOUR_NAME` 必须替换。推荐通过 `scripts/write-client-examples.ps1 -Executable <实际绝对路径>` 生成含本机路径的文件。不要用示例覆盖已有的完整配置。AgentKanban 无须 API Key。

### Codex

将 [examples/codex.toml](../examples/codex.toml) 中的表合并到 `%USERPROFILE%\.codex\config.toml`，或受信任项目的 `.codex/config.toml`。OpenAI 官方文档要求 stdio 条目提供 `command`，可选 `args`；这里无需参数。重启/刷新客户端 MCP 后，用 `/mcp` 检查连接。[Codex 官方 MCP 文档](https://learn.chatgpt.com/docs/extend/mcp)

```toml
[mcp_servers.agentkanban]
command = 'C:/Users/YOUR_NAME/AppData/Local/AgentKanban/agentkanban-mcp.exe'
args = []
```

TOML 示例使用正斜杠，避免 Windows 反斜杠转义。`command` 是可执行文件本身，不是 PowerShell 命令行，也不包含额外的嵌套引号。

### Claude Code

在 PowerShell 中使用实际安装路径添加用户范围服务器：

```powershell
claude mcp add --transport stdio --scope user agentkanban -- "$env:LOCALAPPDATA\AgentKanban\agentkanban-mcp.exe"
```

此命令会修改 Claude Code 用户配置，使工具跨项目可用。项目范围可改为 `--scope project`；对应 JSON 结构见 [examples/claude-code.mcp.json](../examples/claude-code.mcp.json)。使用 `/mcp` 查看连接状态；`claude mcp get agentkanban` 用于查看配置。[Claude Code 官方 MCP 文档](https://code.claude.com/docs/en/mcp)

### Cursor

将 [examples/cursor.mcp.json](../examples/cursor.mcp.json) 中 `mcpServers.agentkanban` 合并到 `%USERPROFILE%\.cursor\mcp.json`；仅当前项目使用时放到 `.cursor/mcp.json`。STDIO 配置包括 `type: "stdio"`、可执行文件的 `command` 和空的 `args`。在 Cursor 的 MCP 设置中检查服务器已启用且工具加载成功。[Cursor 官方 MCP 文档](https://cursor.com/docs/mcp)

```json
{
  "mcpServers": {
    "agentkanban": {
      "type": "stdio",
      "command": "C:/Users/YOUR_NAME/AppData/Local/AgentKanban/agentkanban-mcp.exe",
      "args": []
    }
  }
}
```

这些步骤属于本地 Windows 客户端配置。远程容器、WSL 和云端 Agent 不能仅靠上述 Windows 路径访问本地看板，v0.1 不提供远程传输。

## 更新规则

- 只有用户明确要求记录的功能任务才入板，普通问答和逐条命令不入板。
- 每个任务选择一个稳定的 `task_key`，例如 `feature:export-report`。创建、更新、完成后重新打开都沿用它，避免每轮对话创建新任务。
- 开始执行、阶段变化、遇到阻塞和完成时，用一句话概括实质进展。无变化时不重复写入，不复制聊天历史、完整日志或敏感凭据。
- 后续会话先 `task_list` 查询当前项目未完成项，找到原记录后继续更新。若可能已完成或已归档，显式扩展查询范围。
- 标题描述功能；`progress` 说明目前结果或阻塞。`in_progress` 只表示最近一次上报，不是进程存活检测。

可将以下短说明按需放进项目 Agent 指令：

> 用户明确要求跟踪某功能时使用 AgentKanban。先按项目查询，使用稳定 task_key；只在开始、实质进展、受阻或完成时更新原任务。普通问答不入板，不记录命令流水和聊天历史。后续会话查询未完成项继续原任务。

## 三个工具

### `task_upsert`

按项目和 `task_key` 创建或更新任务。重复调用同一身份更新原记录，服务器生成更新时间。

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `project_path` | string，必填 | 项目绝对目录路径；使用当前实际工作目录所属的项目 |
| `task_key` | string，必填 | 项目内稳定标识，1–160 字符，不以标题或会话 ID 替代 |
| `title` | string，必填 | 简短功能标题，1–200 字符 |
| `status` | enum，必填 | `todo` / `in_progress` / `blocked` / `done` |
| `progress` | string，必填 | 一句话进展或阻塞原因，最多 600 字符，可为空 |
| `branch` | string 或 null，可选 | 展示用分支标签，1–200 字符，不参与任务身份 |

四种状态对应待办、进行中、受阻、已完成。文字字段只接受单行，不接受换行或控制字符。更新时提供完整的必填字段；省略 `branch` 或传 `null` 会清除标签，需要保留时一并传入。已归档任务必须先用 `task_archive` 恢复，再更新。

```json
{
  "project_path": "E:/Projects/Example",
  "task_key": "feature:export-report",
  "title": "导出报告",
  "status": "in_progress",
  "progress": "已完成导出逻辑，正在检查中文文件名。",
  "branch": "feature/export-report"
}
```

成功返回最小结果，不返回整张看板，例如：

```json
{"id":42,"status":"in_progress","updated_at":"2026-09-22T08:30:00.000Z"}
```

### `task_list`

查询任务并分页，默认只返回未完成、未归档任务。

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `project_path` | 不限项目 | 可选，按项目身份过滤 |
| `status` | 不限状态 | 可选，四种状态之一 |
| `include_done` | `false` | 是否包含已完成项 |
| `include_archived` | `false` | 是否包含已归档项 |
| `limit` | `20` | 每页数量，最大 `100` |
| `offset` | `0` | 从第几个结果开始，非负整数 |

返回 `{"items":[...],"next_offset":20}`。继续时将返回的 `next_offset` 作为下一次 `offset`；`null` 表示没有下一页。每条记录包含任务身份、项目、标题、状态、进展、可选分支、归档标记与更新时间。查询范围包含完成项时用 `include_done: true`；只查询完成项时可直接传 `status: "done"`。

```json
{"project_path":"E:/Projects/Example","limit":20,"offset":0}
```

分页是当前数据的分段读取，查询期间其他 Agent 更新排序可能使跨页结果变化；重要批量操作前重新查询并按 `id` 去重。

### `task_archive`

隐藏或恢复任务，不删除数据；归档不等于完成。

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `project_path` | string，必填 | 项目绝对目录路径 |
| `task_key` | string，必填 | 原任务稳定标识 |
| `archived` | boolean，默认 `true` | `true` 归档，`false` 恢复 |

```json
{"project_path":"E:/Projects/Example","task_key":"feature:export-report","archived":false}
```

成功同样只返回 `{id,status,updated_at}`。归档不存在的任务会返回错误。归档后需要查询时传 `include_archived: true`，若任务也已完成同时传 `include_done: true`。

## 项目身份、并发与错误

项目显示名称默认取目录名。Git 主仓库及其 worktree 共用项目身份；分支只是标签。非 Git 项目按指定目录识别，不同目录即使同名也保持独立。内容完全相同的重复更新保持原更新时间，不将一次无变化的调用显示为新进展。

多个客户端可各自启动独立 MCP 进程，共用数据库。对不同任务的更新互不覆盖；同一任务的并发更新以最后成功写入为准，v0.1 不保存每次更新的事件历史。

参数错误、不存在的归档目标和数据库写入错误会显式返回，Agent 应说明失败并保留原任务标识。只有收到成功结果才可声称看板已同步。锁等待有上限；持久锁定、目录不可写或磁盘错误不会被当作成功。

默认数据库位置为 `%LOCALAPPDATA%\AgentKanban\agentkanban.sqlite3`。若通过 `AGENTKANBAN_DATA_DIR` 覆盖目录，给 GUI 与所有 MCP 进程设置同一值。该变量主要用于隔离测试，不需要加入常规配置。
