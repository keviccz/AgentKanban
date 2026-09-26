# MCP 接入与任务契约

AgentKanban 提供一个独立的本地 stdio MCP 程序 `agentkanban-mcp.exe`，只有 `task_upsert`、`task_list` 和 `task_archive` 三个工具。它由客户端按需启动，直接写入与浮窗共用的 SQLite；GUI 没有运行时仍可写入，无需单独启动 MCP，也无需让 Coding Agent 随 Windows 登录启动。协议使用 UTF-8 的逐行 JSON-RPC 消息，标准输出只用于协议，诊断写入标准错误。[MCP stdio 规范](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)

## 从浮窗接入

设置 →「Agent 接入」展示 MCP 可执行文件和数据库的实际位置，并可一键接入 Codex、Claude Code、DeepSeek Harness、Cursor、OpenCode、Antigravity、Hermes：写入含实际路径的 MCP 配置和自动记录规则，首次修改的文件先备份为 `.agentkanban.bak`（写入位置见 README）。无法自动合并时展开「手动配置」复制片段。安装位置或免安装包位置变化后，重新点一次接入即可；静态示例中的默认路径不会自动跟随移动。

本地自检用于确认本机 MCP 可执行文件能启动并完成协议响应。**自检通过只证明本地 MCP 可用，不证明 Codex、Claude Code 或 Cursor 已连接。** 完成配置后，仍需到目标客户端检查工具加载，并让真实 Agent 查询或更新一次任务。只读取配置、复制配置或检查语法，都不能代替这一步。

若本地自检失败，先按界面返回的具体错误检查可执行文件与数据目录；若自检通过但客户端不可用，检查客户端使用的绝对路径、MCP 是否已刷新及客户端自身的连接错误。浮窗和客户端若使用不同的 `AGENTKANBAN_DATA_DIR`，会看到不同的看板。

## MCP 断线时继续记录

关闭浮窗不会中断独立的 MCP；若客户端与 MCP 的 stdio 连接确实断开，有本机命令执行能力的 Agent 可使用同一程序的单次调用入口。它直接使用同一数据库和业务校验，不增加常驻进程或第四个 MCP 工具。重开浮窗会读到这些更新；恢复 MCP 后继续原任务。

```powershell
# 路径与环境必须取自原客户端的 MCP 配置，下面只是调用格式。
& $mcpExecutable --call task_list --input-file $queryFile
& $mcpExecutable --call task_upsert --input-file $updateFile
```

`--input-file` 接收 UTF-8 JSON 文件（可有 BOM），内容为该工具的参数对象；也可用 `-` 从标准输入读到 EOF，最多 1 MiB。原配置有 `AGENTKANBAN_DATA_DIR` 时必须保持一致；原配置没有时沿用相同用户环境的默认目录，不猜测其他位置。命令成功向 stdout 输出紧凑 JSON 结果，失败输出 `{"error":"原因"}` 并以非零状态退出。暂停仍返回 `paused:true, recorded:false`，表示没有同步任务。无命令参数时仍是原 stdio MCP 协议。

备用调用仍只在正常里程碑由主 Agent 执行：先 `task_list` 按项目和原 key 精读，处理最新用户意见，再带 `expected_updated_at` 更新。断线前的一次写入可能已提交但回执丢失，因此先读再合并，不能另建任务或盲目重放。MCP 恢复后同样先精读。冲突、参数错误、用户暂停、权限拒绝或明确禁用工具都不能触发备用流程。

该入口不能替客户端重新连接 MCP，也不能在 Agent 无命令权限、程序/数据库不可用或 Agent 本身停止运行时自动记录。两种入口都失败时应明确告知未同步，保留项目、任务 key 和当前进展，恢复后合并最新事实；不补造空档内的历史。没有自动逐条重放队列，也不把久未更新当作断线检测。

升级两个程序后，在设置中重新一键接入以更新客户端同步规则，并让客户端重新加载规则；已经运行的旧会话不会自动学会备用流程。该规则仅增加断线时的条件分支，平时不新增查询或轮询。旧版程序不支持 `--call`。

## 查看同步健康

在设置 → Agent 接入查看「同步健康」，可区分最近一次 MCP 调用、备用 CLI 调用、暂停跳过和失败，并查看最近成功调用与成功保存的时间。打开该页面或手动刷新时读取本机记录，不会额外调用 Agent。成功保存表示一次写入请求被接受，幂等重试也可能显示在此处；任务实质进展仍以任务本身及上报记录为准。

记录从 0.5 的 MCP 开始产生，只保留三类最新事件，不保存参数、任务正文或项目路径，不随工具回复返回给模型，不增加第四个工具。记录诊断失败不改变原工具调用结果；这些记录不是心跳、实时连接检测或待补报队列，不能据此认定尚未收到的进度已经同步。旧 MCP 进程需由客户端重新加载后才会产生新记录。

## 配置客户端

优先复制浮窗生成的实际路径配置。以下文件是合并示例，路径中的 `YOUR_NAME` 必须替换；也可通过 `scripts/write-client-examples.ps1 -Executable <实际绝对路径>` 生成独立示例文件。复制或生成示例不会自动修改客户端配置，不要用示例覆盖已有的完整配置。AgentKanban 无须 API Key。

### Codex

将 [examples/codex.toml](../examples/codex.toml) 中的表合并到 `%USERPROFILE%\.codex\config.toml`，或受信任项目的 `.codex/config.toml`。OpenAI 官方文档要求 stdio 条目提供 `command`，可选 `args`；这里无需参数。重启/刷新客户端 MCP 后，用 `/mcp` 检查连接。[Codex 官方 MCP 文档](https://learn.chatgpt.com/docs/extend/mcp)

```toml
[mcp_servers.agentkanban]
command = 'C:/Users/YOUR_NAME/AppData/Local/AgentKanban/agentkanban-mcp.exe'
args = []
```

TOML 示例使用正斜杠，避免 Windows 反斜杠转义。`command` 是可执行文件本身，不是 PowerShell 命令行，也不包含额外的嵌套引号。

**Codex 还需要一段 AGENTS.md 规则。** 历史实测 Codex CLI 0.156.1（本机配置，同时接入 Playwright 等 MCP）会延迟加载工具说明，因此仅配置 MCP 不足以触发自动入板。设置中的「一键接入」会把 [精简规则](../examples/codex-AGENTS-snippet.md) 写入 `%USERPROFILE%\.codex\AGENTS.md`（Claude Code、DeepSeek Harness 等写入各自全局指令文件）；也可在「手动配置」中复制。实际上下文开销取决于客户端投递方式和 tokenizer，不将字符数当成 token。

### Claude Code

浮窗的一键接入会先备份，再结构化合并用户级 `~/.claude.json`，保留其他服务器和设置；不再先删除已有服务器再尝试添加。以下 CLI 命令仅是手动接入的替代方式。

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

这些步骤属于本地 Windows 客户端配置。远程容器、WSL 和云端 Agent 不能仅靠上述 Windows 路径访问本地看板，当前不提供远程传输。

## 更新规则

自 0.4 起默认自动入板。规则写在 `task_list` 与 `task_upsert` 的工具描述中，随工具列表交给模型；Codex 延迟加载工具说明，需另加上文的一行 AGENTS.md 规则。无需另装 Skill：

- 会修改文件的任务（代码、配置、文档）无需用户提醒即自动入板；普通问答、只读审查和调研不入板；用户说「不用记」时不记。用户从浮窗创建并交给 Agent 的待办已有记录，先查询并接手，不重复新建。
- 每个任务选择一个稳定的 `task_key`：自动入板用 `auto:<简短标识>`，也可沿用 `feature:export-report` 这类已有标识。创建、更新、完成后重新打开都沿用它，避免每轮对话创建新任务。
- 同一任务由一个主 Agent 统一记账，子 Agent 返回结果，不重复入板或并行覆盖；独立任务才使用不同标识。
- 开工写 `goal`（做什么）、`acceptance`（怎么验收）和 `steps`（计划）。之后只在阶段完成、真实阻塞和全部完成时更新；优先用 `step_updates` 局部更新步骤，重排才传整份 `steps`。不为单次修改或命令更新，不复制聊天历史、完整日志或敏感凭据。
- 已知 `task_key` 直接精读；否则以 `project_path` 和 `query` 关键词定向找，避免扫描所有页面。摘要的 `has_request` / `has_user_note` 提示原始需求或用户意见；接手已有记录前精读完整信息。精确 key 默认包含已完成及归档记录，显式过滤参数仍生效。
- 标题描述功能；`progress` 说明目前结果或阻塞。`in_progress` 只表示最近一次上报，不是进程存活检测。
- `blocked` 用于实际阻碍继续推进的输入、依赖或外部问题；单纯耗时或久未更新不构成阻塞。用 `needs_input` 说明需要用户提供什么，解决后显式清空。只有工作与必要验证完成后才标记 `done`，并提供成果位置、如实说明未验证范围；`done` 是执行完成，不是用户验收通过。
- 已有任务的实质更新和归档必须携带最新回执或查询的 `expected_updated_at`；缺失或过期均拒绝。无版本号只允许新建或完全相同的幂等重试。冲突后按原 key 精读并合并，不去掉校验强行覆盖。
- 通过 MCP 工具或上文的同程序断线入口读写任务，不直接编辑 SQLite；只有成功且未暂停，才可声称已同步。

若某个客户端需要更完整的规则，可将 [AGENT_RULES.md](AGENT_RULES.md) 的短中文规则复制进项目 Agent 指令。浮窗复制的同步规则与该文件保持一致。

另有 [可选 Skill 示例](../examples/agentkanban/SKILL.md)，适合在支持 Skill 的 Agent 中按需使用。它只补充记录与同步流程，不启动 MCP、不增加工具，也不能代替上述客户端配置；仓库提供示例，不自动安装或修改全局 Agent 指令。

## 从需求到验收

用户可以在浮窗或通过 `Ctrl+Alt+N` 创建文字待办，选择已有项目或填写绝对目录，并填写标题和详细需求。开工说明包含原任务身份；复制到已接入客户端后，由 Agent 查询并接手。保存待办和复制说明都不会自动启动或派发 Agent。当前不提供语音录入。

状态仍只有 `todo`、`in_progress`、`blocked`、`done`。详情额外保存以下交接信息：

| 字段 | 维护方与含义 |
| --- | --- |
| `request` | 用户在浮窗新建时填写的详细需求，最多 2000 字符，可多行 |
| `agent` | Agent 自报名称或标识，不是认证身份或在线信号 |
| `next_action` / `needs_input` | Agent 的下一步与需要用户提供的内容 |
| `deliverables` | Agent 提供的文件、PR 或其他成果位置，只保存标签与路径/链接 |
| `user_note` | 用户最新一条补充或修改意见，最多 2000 字符，可多行；不是聊天历史 |
| `review_status` | `none` 无验收记录、`pending` 等待验收、`accepted` 已通过、`changes_requested` 需修改 |
| `agent_updated_at` | 最近一次 Agent 实质更新的服务器时间；与整条记录的 `updated_at` 分开 |
| `steps` | Agent 的计划步骤，最多 12 项 `{title,status,note?}`，状态同任务四种状态；浮窗卡片显示完成数，详情显示完整清单 |
| `goal` | 任务要做什么，一两句，详情中显示为「要做什么」 |
| `acceptance` | 用户验收时的检查项，最多 8 条，详情中显示为「验收标准」 |
| `review_withdrawn_at` | Agent 在用户验收前把 `done` 任务改回其他状态的时间；下次完成时清除，浮窗显示「已撤回验收」 |

用户可在浮窗底栏暂停记录：此后三个工具都返回成功结果 `{"paused":true,"recorded":false,"message":...}`，不读写任务。Agent 继续工作，不重试或轮询，在下一个正常里程碑或新任务再尝试一次。恢复在下次正常调用时生效，不会主动唤醒 Agent，也不回补暂停期间的逐条操作。暂停标记每次调用都检查，已有 MCP 进程无需重启。

用户可在浮窗任务详情中归档任务（二次确认，数据保留，不计为 Agent 更新），通过设置中的归档中心搜索并恢复。GUI 恢复要求当前版本，保留任务身份、验收结果、用户意见与上报记录，不刷新 Agent 的活动时间。Agent 也可调用 `task_archive(archived=false)` 恢复；三个工具的接口保持不变。

新任务直接设为 `done`，或从其他状态转为 `done`，会进入 `pending`。在用户验收前 Agent 又把任务改为其他状态时，待验收被取消并记录 `review_withdrawn_at`。用户在浮窗中通过验收后成为 `accepted`；退回修改必须填写意见，任务变为 `todo` 与 `changes_requested`，重新进入默认未完成查询。Agent 继续执行时保留修改请求，下一次完成重新等待验收。已通过的完成任务被 Agent 实质修改后也需重新验收。

`request`、`user_note` 和 `review_status` 不能由 MCP 直接写入，Agent 无法代替用户通过验收。旧版已完成任务迁移后保持 `done` 与 `none`，不强制全部重新验收；重新打开后再完成才进入新验收流程。人工反馈会改变 `updated_at`，但不改变 `agent_updated_at`。

久未更新提示只作用于 `in_progress` 和 `blocked`，默认阈值为 24 小时，可关闭或设为 1、4、8、24、48、168 小时。它依据 Agent 最近一次实质更新展示，不写回任务、不改变状态；Agent 不应为了消除提示而制造更新。

`Ctrl+Alt+K` 显示/隐藏浮窗，`Ctrl+Alt+N` 打开新建，两者可在设置中一并关闭。快捷键及 Windows 登录启动只管理浮窗，MCP 仍由客户端按需启动。

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
| `agent` | string 或 null，可选 | 最多 100 字符；省略或 `null` 保留，空字符串清空 |
| `next_action` | string，可选 | 下一步，最多 600 字符；省略保留，空字符串清空 |
| `needs_input` | string，可选 | 需要用户提供的内容，最多 600 字符；省略保留，空字符串清空 |
| `deliverables` | array，可选 | 最多 5 项 `{label,uri}`；省略保留，空数组清空。`label` 为 1–100 字符，`uri` 为 1–1000 字符的路径或 HTTP(S) 链接 |
| `steps` | array，可选 | 最多 12 项 `{title,status,note?}`；`title` 1–120 字符，`note` 最多 200 字符，均为单行。省略保留，传入时整份替换，空数组清空 |
| `step_updates` | array，可选 | 最多 12 项 `{index,status?,note?}`；索引从 0 起，`status` / `note` 至少提供一个；备注最多 200 字符，省略保留、空字符串清除。不允许重复或越界索引，与 `steps` 互斥，仅用于已有计划 |
| `goal` | string，可选 | 最多 300 字符，单行；省略保留，空字符串清空 |
| `acceptance` | array，可选 | 最多 8 条，每条 1–160 字符，单行；省略保留，传入时整份替换 |
| `expected_updated_at` | string，新建可省略 | 已有任务实质更新必填，取最新回执或查询的 `updated_at`；缺失或过期拒绝且零写入。完全幂等重试可省略 |

四种状态对应待办、进行中、受阻、已完成。此工具的文字字段只接受单行，不接受换行或控制字符。更新时提供完整的必填字段；省略 `branch` 或传 `null` 会清除标签，需要保留时一并传入。新增的 Agent 交接字段使用上述保留/清空规则，不能把整个工具理解为全字段替换。状态变化不会自动清空 `needs_input`，问题解决后应显式传空字符串。已归档任务必须先用 `task_archive` 恢复，再更新。

```json
{
  "project_path": "E:/Projects/Example",
  "task_key": "feature:export-report",
  "title": "导出报告",
  "status": "in_progress",
  "progress": "已完成导出逻辑，正在检查中文文件名。",
  "branch": "feature/export-report",
  "agent": "Codex",
  "next_action": "检查长文件名与空内容导出。",
  "needs_input": "",
  "expected_updated_at": "2026-09-22T08:25:00.000Z"
}
```

上例用于更新已有任务，`expected_updated_at` 必须取实际查询结果。创建新任务时省略它。完成时可附 `"deliverables": [{"label":"导出功能 PR","uri":"https://github.com/OWNER/REPO/pull/42"}]`；交付位置应可定位到实际成果，不以链接存在代替验证。

已有计划推进时可附 `"step_updates":[{"index":0,"status":"done","note":"中文文件名检查通过"}]`，不必重发其他步骤。版本校验保证索引不会作用于另一个 Agent 刚重排的计划；越界、重复索引或同时提供两种计划参数时，整个更新失败，不会部分写入。

成功返回最小结果，不返回整张看板，例如：

```json
{"id":42,"status":"in_progress","updated_at":"2026-09-22T08:30:00.000Z"}
```

### `task_list`

查询任务并分页，默认只返回未完成、未归档任务的摘要，控制 Agent 上下文占用。

| 参数 | 默认值 | 说明 |
| --- | --- | --- |
| `project_path` | 不限项目 | 可选，按项目身份过滤 |
| `task_key` | 不限标识 | 可选，精确匹配；建议同时指定项目。指定时默认返回完整记录，包含已完成和归档任务 |
| `query` | 不搜索 | 可选，1–160 字符的字面子串；搜索 key、标题、目标、原始需求、进展和用户意见，`%` / `_` 不作为通配符 |
| `status` | 不限状态 | 可选，四种状态之一 |
| `include_done` | 普通查询 `false`；精确 key `true` | 是否包含已完成项；显式 `false` 仍排除 |
| `include_archived` | 普通查询 `false`；精确 key `true` | 是否包含已归档项；显式 `false` 仍排除 |
| `detail` | 有 `task_key` 时为 `true`，否则 `false` | 是否返回完整记录 |
| `limit` | `5` | 每页数量，最大 `100` |
| `offset` | `0` | 从第几个结果开始，非负整数 |

返回 `{"items":[...],"next_offset":5}`。继续时用 `next_offset` 作为下一次 `offset`，`null` 表示没有下一页；日常寻找任务应优先加 `query`，不必枚举整个项目。摘要包含身份、标题、状态、更新时间、短进展和步骤完成数；`has_request` / `has_user_note` 提示需要精读的内容。进展超过 120 字时截断并设置 `progress_truncated`，全文仍保留。指定项目时路径只放顶层 `project_path`，跨项目查询才随每项返回。完整记录包括项目路径、`request`、`user_note`、完整计划与交付物等全部字段。

普通查询可用 `include_done:true` 扩展完成项，或用 `status:"done"` 只查完成项；显式 `include_done:false` 优先排除。精确 key 默认跨完成/归档过滤找回原记录，调用者显式指定的排除条件仍保留。

```json
{"project_path":"E:/Projects/Example","query":"导出报告"}
```

找到候选后精读：`{"project_path":"E:/Projects/Example","task_key":"feature:export-report"}`。

分页是当前数据的分段读取，查询期间其他 Agent 更新排序可能使跨页结果变化；重要批量操作前重新查询并按 `id` 去重。

### `task_archive`

隐藏或恢复任务，不删除数据；归档不等于完成。

| 参数 | 类型 | 说明 |
| --- | --- | --- |
| `project_path` | string，必填 | 项目绝对目录路径 |
| `task_key` | string，必填 | 原任务稳定标识 |
| `archived` | boolean，默认 `true` | `true` 归档，`false` 恢复 |
| `expected_updated_at` | string，实质变化必填 | 最近回执或查询的 `updated_at`；缺失或过期拒绝。状态已相同的幂等重试可省略 |

```json
{"project_path":"E:/Projects/Example","task_key":"feature:export-report","archived":false,"expected_updated_at":"2026-09-26T06:00:00.000Z"}
```

示例版本号须替换为实际查询值。成功同样只返回 `{id,status,updated_at}`。归档不存在的任务返回错误。恢复前按 key 精读即可找到归档任务；普通列表需要 `include_archived:true`，完成项还需 `include_done:true`。

## 项目身份、并发与错误

项目显示名称默认取目录名。Git 主仓库及其 worktree 共用项目身份；分支只是标签。非 Git 项目按指定目录识别，不同目录即使同名也保持独立。内容完全相同的重复更新保持原更新时间，不将一次无变化的调用显示为新进展。

多个客户端可启动独立 MCP 进程，共用数据库。不同任务互不覆盖；同一任务的实质修改必须通过 `expected_updated_at` 校验。两个 Agent 同时发现没有任务、以同一 key 创建不同内容时，只会有一个成功，另一个收到冲突；不存在无版本号强行覆盖路径。同一任务指定一个记账负责人可进一步减少冲突和重复上下文。

每个任务保留最近 30 次 Agent 实质上报；MCP 保存实际传入字段（去除项目/key/版本等路由字段），无变化的幂等调用不新增历史。早期版本记录可能经过字段规范化，保留不重写。历史供浮窗按需查看，不随 `task_list` 返回，不将这些记录自动塞入 Agent 上下文。用户意见仍只保存最新一条，不是完整聊天日志。

参数错误、更新时间冲突、不存在的归档目标和数据库写入错误会显式返回，Agent 应说明失败并保留原任务标识。冲突后先重新查询，再根据最新需求和进展更新。只有收到成功结果才可声称看板已同步。锁等待有上限；持久锁定、目录不可写或磁盘错误不会被当作成功。

默认数据库位置为 `%LOCALAPPDATA%\AgentKanban\agentkanban.sqlite3`。若通过 `AGENTKANBAN_DATA_DIR` 覆盖目录，给 GUI 与所有 MCP 进程设置同一值。该变量主要用于隔离测试，不需要加入常规配置。
