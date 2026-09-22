# 验证与验收

## v0.2 实测结果（2026-09-22）

本轮新增接入诊断、实际路径配置复制、本地 MCP 自检、久未更新提示、项目聚焦与置顶、只读详情、可选登录启动及全局快捷键。保留下方 v0.1 原始结果。

| 检查 | 结果与范围 |
| --- | --- |
| Rust 自动检查 | PASS：25 项测试（12 数据、8 MCP、5 桌面）；桌面和数据模块 Clippy `--all-targets -- -D warnings`、全工作区格式检查通过 |
| 前端生产构建 | PASS：TypeScript 类型检查与 Vite 构建 |
| MCP debug / release 真实进程 | PASS：各 12/12；协议仍只有 3 个工具；不依赖运行中的 GUI |
| 真实桌面 WebView 主流程 | PASS：11 项。旧偏好兼容、项目置顶排序与聚焦、状态筛选、更新不重置视图、提示阈值开关、详情全文与复制、Escape 焦点恢复、诊断与配置复制、真实 MCP 自检、隐藏恢复、重启保留偏好；无 WebView 页面错误 |
| 边界行为 | PASS：4 项。聚焦项目全部归档后明确显示空项目；恢复任务不重开详情；快照读取失败保留已读偏好；偏好读取失败时禁用修改，重试后恢复。最后一项检测还确认可见时有实际读取、隐藏时读取停止、恢复立即读取 |
| 登录启动 | PASS（注册项开关）：通过设置页实际启用并关闭，读取系统实际状态确认，最终恢复测试前关闭状态；没有注销或重启 Windows，实际登录启动 NOT_RUN |
| 全局快捷键 | PASS（原生注册）：实际停用/注册 `Ctrl+Alt+K` 并检查持久化。物理按键触发 NOT_RUN：Windows Computer Use 重置重试仍报 `native pipe unavailable` |
| 布局 | PASS：浅色/深色、长中文标题、长进展及路径；320×360、380×520、480×640 视口无横向溢出。当前系统实际缩放 150%；其他缩放为 CDP 模拟 |
| 安装包及免安装包 | PASS：0.2.0 NSIS x64 与 ZIP 构建；包含两程序、同步规则及可选 Skill。实际运行免安装 GUI：版本 0.2.0、窗口响应、数据库及其下 WebView 缓存创建正常、stderr 为空 |
| 客户端及用户验收 | v0.2 未重新调用模型执行 Agent 演示；v0.1 Codex 实测保留在下方。Claude / Cursor、安装卸载向导、物理托盘与拖动、Windows 重新登录、持续日常使用仍未验证 |

测试使用独立临时数据目录；没有更改已有客户端配置、安装全局 Skill，或在日常数据库创建验收样例。Browser plugin 未提供，使用 Playwright 通过调试版 WebView2 的 CDP 检查真实 Tauri 窗口，release 仅做启动检查。

本机证据保存在 `%TEMP%\agentkanban-v02-20260922\`：`results.json`（11 项主流程）、`edge-results.json`（4 项边界检查）、`mcp-debug.json`、`mcp-release.json`、`release-startup.json` 与 PNG 截图。原始失败结果保留为 `*-before-*.json`；其中包括已修复的模态框焦点问题，以及测试脚本对异步复选框、TOML 末尾换行和只读内部接口的错误假设。

异常读取通过测试专用网络拦截在前端 bridge 注入失败，不改变发布代码。最终的隐藏读取检查在 bridge 中计数，并先证明可见时计数增长，避免把未生效的内部接口替换误判为通过。MCP 自检检查前后数据库 revision 相同，没有创建或修改任务。

## v0.1 验证与验收

记录必须区分实现、自动验证、客户端连接、原生窗口行为和用户验收。脚本通过只证明其覆盖的接口行为；浏览器截图不能证明原生窗口置顶或托盘功能。

## 自动验证

在仓库根目录运行构建脚本，得到真实 MCP 可执行文件后执行：

```powershell
node .\scripts\verify-mcp.mjs .\target\release\agentkanban-mcp.exe --report .\artifacts\verification\mcp.json
```

验证脚本不加载聊天历史、不调用模型、不修改客户端配置。它在系统临时目录下创建独立项目与数据目录，通过 `AGENTKANBAN_DATA_DIR` 传给每个子进程。

| 检查 | 判定依据 |
| --- | --- |
| MCP 协议 | `initialize`、`notifications/initialized`、`ping`、`tools/list` 和工具定义 |
| 状态流转 | 创建 → 进行中 → 受阻 → 完成 → 重新打开，身份保持不变 |
| 幂等性与中文 | 重复更新只有一条记录，中文标题、进展和分支往返一致 |
| 归档恢复 | 默认隐藏归档项；扩展查询可见；恢复后保持原身份 |
| 分页与过滤 | 默认最多 20 项、显式分页、项目过滤和已完成过滤 |
| 持久化 | 关闭 MCP 后新进程可读到原记录，全过程不启动 GUI |
| 并发 | 四个独立 MCP 进程写入 32 条不同任务，无遗漏或重复身份 |
| 错误 | 无效参数、未知方法/工具、不存在任务、已连接后的数据库写入失败和不可用数据目录明确失败 |
| 兼容与输出 | 较早协议握手；标准输出保持 JSON-RPC，不混入普通日志 |

成功后清理脚本创建的临时数据，传 `--keep` 可保留；失败时自动保留数据与 `verification.json`，控制台给出路径。`--report` 另外保存机器可读结果。缺少可执行文件时先构建，不应以模拟服务代替真实服务。

## Windows 窗口现场检查

使用独立数据目录启动 GUI 与 MCP，避免把验收样例混入日常看板。逐项记录实际观察结果：

1. 首次浅色、约 380 × 520；窗口可以拖动和缩放，显示缩放 100% / 125% / 150% 下文字与按钮不裁切。
2. 置顶开关分别开启和关闭，在另一应用窗口前后检查实际层级。
3. 切换深色主题；关闭并重启程序后，主题、位置、尺寸及展开状态恢复。
4. 收成窄条后显示进行中/受阻数量；再展开恢复正常窗口尺寸。
5. 关闭按钮只隐藏窗口，托盘仍存在；托盘恢复显示，托盘退出结束 GUI。
6. 隐藏或退出 GUI 后从 MCP 更新任务；重新显示或启动 GUI，立即看到最新结果。
7. 多项目、长中文标题和大量任务下，内容在窗口内滚动；每项目默认三条未完成项，已完成默认折叠；任务更新不强制展开。
8. 浮窗只提供查看、筛选和折叠等操作，没有任务编辑输入框。

## 客户端与真实 Agent 流程

对每个实际可用的客户端分别记录版本、配置路径范围、连接结果和工具调用结果。读取配置或执行 JSON/TOML 语法检查属于配置检查，不能标作客户端连接通过。

1. 用独立测试配置连接真实 `agentkanban-mcp.exe`，确认三个工具可用。
2. 对 Agent 说「把验收演示功能加入看板，后续同步进展」，确认它创建稳定 `task_key`。
3. 让 Agent 报告一个真实阶段变化，确认同一条记录更新为进行中。
4. 完成演示工作后由 Agent 标成已完成，确认它进入已完成区且默认未完成查询不再返回它。
5. 新会话查询该项目并确认能定位同一条任务；需要复验时重新打开原任务。

只有真实 Agent 使用已连接工具完成上述过程，才能标作 Agent 流程通过。手工 JSON-RPC 脚本只证明协议和数据行为。未安装、未登录或未执行的客户端保留 `NOT_RUN` 并说明具体原因。

## 本次结果

2026-09-22 在 Windows 上实际执行。所有样例使用临时数据目录，没有把验收任务写入日常数据库，也没有更改已有客户端配置。

| 项目 | 状态 | 证据/缺口 |
| --- | --- | --- |
| Rust 数据模块与 MCP 自动检查 | PASS | `cargo test -p kanban-core -p agentkanban-mcp`：19 项通过（11 数据、5 协议、3 子进程）；含 worktree 归并、锁等待/超时、4 进程 48 任务 96 次写入。两个模块及桌面 `clippy --all-targets -- -D warnings` 均通过 |
| 前端类型检查与生产构建 | PASS | `npm run build`：TypeScript 与 Vite 生产构建通过 |
| MCP 真实进程黑盒检查 | PASS | [debug 12/12](evidence/mcp-debug.json)、[release 12/12](evidence/mcp-release.json) |
| Windows 安装包构建与文件完整性 | PASS | NSIS x64 安装包与 ZIP 已生成；生成的 installer.nsi 与 ZIP 内容均包含 GUI、MCP、README、docs、examples 和配置生成脚本。未在用户系统执行安装/卸载向导 |
| release 桌面启动 | PASS（启动检查） | [进程与窗口记录](evidence/release-startup.json)：实际启动免安装目录中的 GUI，窗口响应、SQLite 与 WebView 数据目录创建正常；不等同于 release DOM 自动验收 |
| 真实 Tauri WebView 与原生 API 检查 | PASS（已列范围） | [8 项交互](evidence/ui-results.json)、[重启恢复](evidence/persistence-results.json)、[新增 5 项目 80 任务布局](evidence/many-results.json)。主题、原生置顶 getter、48px 窄条、筛选/折叠、隐藏停止查询、MCP 独立写入、第二次启动恢复、重启持久化通过 |
| 托盘菜单点击、鼠标拖动/边缘缩放、系统缩放切换 | NOT_RUN | Windows Computer Use 重试并重置后仍报 native pipe unavailable。已验证关闭按钮确实隐藏，第二次启动可恢复；没有宣称实际点过托盘。系统实际为 125%，100%/150% 仅 CDP 模拟 |
| Codex 真实连接与 Agent 流程 | PASS | Codex CLI 0.155.1，ChatGPT 登录，临时 CLI 配置；[首会话](evidence/codex-demo.jsonl) todo → in_progress → blocked，[后续会话](evidence/codex-resume.jsonl) 查询原记录 → in_progress → done，始终 id=1 |
| Claude Code 真实连接与 Agent 流程 | 连接 NOT_RUN；Agent 流程 NOT_RUN | 2.1.278 隔离配置检查通过；user scope 注册被自动审批拒绝，安全改用临时 project scope 后显示 Pending approval，尚未连接。[实际记录](evidence/claude-connection.txt) |
| Cursor 真实连接与 Agent 流程 | NOT_RUN | PATH 未找到 `cursor` / `cursor-agent`；未运行连接与 Agent 流程 |
| 用户日常使用体验验收 | NOT_RUN | 交付后由用户体验确认 |

Codex 首次尝试只读查看报告时，CLI 执行策略返回 `blocked by policy`。实际 Agent 将其上报为受阻。主 Agent 随后把已经生成的报告原文作为下一会话输入，避免额外文件访问；新会话核对 12 项结果并通过 MCP 完成原任务。此处不是手工写数据库或脚本模拟 Agent 工具调用。

视觉方法、概念比较、视口和明确差异见 [视觉核对](design/REVIEW.md)。已修复 Vite 监听 Rust 构建目录导致的 Windows 文件锁退出、窄条切换重复加锁风险、窗口几何设置并发落盘顺序问题，以及 WebView 缓存目录未跟随数据目录的问题。最后一项修复后的实际 release 启动记录确认 `webview_under_data_dir: true`，旧观察保留在 `release-startup-before-cache-fix.json`。
