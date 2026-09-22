# AgentKanban

由你记录需求、Coding Agent 同步进展的 Windows 桌面任务浮窗。可以在浮窗里快捷新建，也可以说「把这个功能加入看板，后续同步进展」，由 Agent 通过本地 MCP 创建任务。开始、阶段变化、受阻和完成都更新同一条记录。

看板按项目展示待办、进行中、受阻和已完成任务。Agent 可以写明下一步、需要你补充的内容和交付物；完成后由你验收。应用本身不调用模型 API。

## 启动

安装构建产物中的 Windows 安装包，然后从开始菜单打开 AgentKanban。安装目录内同时包含 `agentkanban.exe` 和 `agentkanban-mcp.exe`；后者由客户端按需启动，无须单独双击。

交付文件在 `release/`：`AgentKanban_<版本>_x64-setup.exe` 是安装包，`AgentKanban-<版本>-windows-x64.zip` 是免安装包。免安装包解压后运行 `AgentKanban/agentkanban.exe`，仍使用统一的本地数据目录。

默认浮窗约 380 × 520，支持拖动、缩放、置顶开关和明暗主题。收起后显示进行中与受阻数量。关闭按钮隐藏到托盘；通过托盘菜单重新显示或退出。主题、窗口位置、尺寸和展开状态会保留。

每个项目默认预览三条活跃任务（未完成及待验收），其余点击展开；不需要验收的已完成任务默认折叠。内容超出时在窗口内滚动，任务更新不会强制展开项目。

从旧版本升级时，先退出旧浮窗并停止客户端中的旧 MCP，备份完整数据目录，再替换两个程序并重新连接 MCP。0.3 首次启动会事务迁移数据库到 schema 2，保留旧任务及设置；旧版程序不支持迁移后的数据库。

## 日常查看

- **快捷新建**：点击新建或按 `Ctrl+Alt+N`（窗口内也可用 `Ctrl+N`），选择已有项目或填写实际存在的绝对目录，输入标题和详细需求；`Ctrl+Enter` 保存。保存后得到稳定任务标识；复制开工说明到已接入的 Agent，即可沿用这条任务。创建待办、复制说明都不会自动启动或派发 Agent。
- **项目聚焦与置顶**：聚焦某个项目可只看它的任务，退出聚焦回到所有项目；常用项目可置顶排列。项目置顶与窗口始终置顶是两个独立操作。
- **任务详情与反馈**：查看完整需求、进展、Agent 标识、下一步、所需输入、交付物、项目路径和分支，并可复制。你可以补充一条最新意见供 Agent 继续时读取；该字段不是聊天记录，提交新内容会替换旧意见。
- **人工验收**：Agent 报告完成后，任务显示等待验收。你可以通过验收，或填写修改意见后退回待办；退回的任务会重新出现在 Agent 默认查询结果中。旧版已完成任务保留原状态，不强制重新验收。
- **久未更新提示**：默认对超过 24 小时没有 Agent 实质更新的进行中、受阻任务显示提示；可关闭，或改为 1、4、8、24、48、168 小时。人工补充或验收不会冒充 Agent 的新上报，提示不会自动改变任务状态。
- **全局快捷键**：`Ctrl+Alt+K` 显示/隐藏浮窗，`Ctrl+Alt+N` 打开快捷新建；设置中可一并关闭。快捷键仅在 AgentKanban 运行时有效；退出后需要重新启动应用。
- **登录启动**：可在设置中选择 Windows 登录后启动浮窗，默认关闭。它只启动看板，无需让 Coding Agent 随 Windows 启动。

四种任务状态保持不变。「执行完成」与「用户已验收」分别显示；MCP 不提供验收工具，验收由浮窗中的用户操作完成。Agent 标识来自它的上报，不代表身份认证或实时在线状态。

## 连接 Agent

首次接入可按以下顺序操作：

1. 打开浮窗设置中的接入区域，查看当前程序、MCP 和数据目录的实际路径。
2. 运行本地自检，确认本机 `agentkanban-mcp.exe` 能启动并响应。
3. 为 Codex、Claude Code 或 Cursor 复制含实际路径的配置，按 [MCP 接入说明](docs/MCP.md) 合并到该客户端，再刷新或重启其 MCP 连接。
4. 在客户端确认三个工具可用，再明确要求 Agent 记录一个功能，并检查浮窗中的实际结果。

**本地自检通过不等于客户端已连接。** 自检不能证明客户端已读取配置，也不能代替一次真实 Agent 工具调用。复制配置不会自动修改客户端文件。

仓库另提供 [Codex](examples/codex.toml)、[Claude Code](examples/claude-code.mcp.json) 和 [Cursor](examples/cursor.mcp.json) 静态示例，使用前需替换示例路径。

也可以用 PowerShell 7 生成含本机路径的独立示例文件：

```powershell
pwsh -NoProfile -File .\scripts\write-client-examples.ps1 -Executable "$env:LOCALAPPDATA\AgentKanban\agentkanban-mcp.exe"
```

如选择了其他安装位置，替换 `-Executable`。生成文件位于 `artifacts/client-config/`，仍需合并到需要使用的客户端。

在连接后的 Agent 中明确说：

> 把当前项目的「导出报告」功能加入 AgentKanban，task_key 用 `feature:export-report`，后续在有实质进展时更新同一条任务。

后续会话先按项目查询未完成项，继续原 `task_key`；从开工说明接手时可按 `task_key` 精确查询，读取需求及最新人工意见。普通问答不入板。「进行中」代表 Agent 最后上报的状态；Agent 意外退出不会自动变成完成。

设置中可复制 [简短同步规则](docs/AGENT_RULES.md)，按需放进项目 Agent 指令；仓库还提供 [可选 AgentKanban Skill](examples/agentkanban/SKILL.md)。Skill 补充何时记录与更新的工作规则，不能代替 MCP 配置，也不会随应用自动安装到全局。

MCP 进程由客户端按需启动，GUI 没有运行时仍可写入。需要查看时再打开浮窗即可；无需一直运行浮窗，也无需设置 Coding Agent 开机启动。

## 从源码开发与构建

需要 Windows、Node.js/npm、Rust MSVC 工具链、Visual Studio C++ Build Tools 和 WebView2 Runtime。

```powershell
npm ci
npm run desktop:dev
```

`desktop:dev` 会先准备 MCP 可执行文件，再启动 Tauri 桌面窗口。`npm run dev` 只用于浏览器界面预览，不能验证托盘、置顶和本地持久化等桌面行为。

构建安装包：

```powershell
npm run desktop:build
```

该命令构建 release MCP 与桌面程序，并生成 Windows 安装包。Tauri 原始安装包位于 `target/release/bundle/nsis/`，整理后的交付文件位置以构建命令输出为准。运行安装包是单独动作，构建不会自动安装或修改客户端配置。

验证真实 MCP 进程：

```powershell
node .\scripts\verify-mcp.mjs .\target\release\agentkanban-mcp.exe --report .\artifacts\verification\mcp.json
```

脚本使用系统临时目录的隔离数据库，覆盖协议握手、三个工具、状态流转、重复更新、分页、归档恢复、重启与四进程并发写入。成功后清理自己的测试数据；失败时保留路径和证据。详见 [验收记录与操作步骤](docs/VERIFICATION.md)。

## 数据与边界

```text
Coding Agent → 独立 stdio MCP 进程 → 本地 SQLite ← 桌面浮窗
```

GUI 与 MCP 共用 Rust 数据模块。浮窗退出后 MCP 仍可写入，重新显示时读取最新任务。窗口可见时每秒检查更新，隐藏时暂停刷新。

默认数据目录为 `%LOCALAPPDATA%\AgentKanban`，任务及界面设置保存在 `agentkanban.sqlite3`，WebView 缓存位于其下 `webview/`。SQLite 使用事务、WAL 和 5 秒锁等待处理多个进程写入；最终失败会返回明确错误。[SQLite WAL 说明](https://sqlite.org/wal.html)

`AGENTKANBAN_DATA_DIR` 可指定独立数据目录，适合测试；GUI 和 MCP 必须使用相同设置才能查看同一张看板。备份时先退出 GUI 并关闭客户端 MCP 进程，再复制完整数据目录。不要只复制仍在写入中的 `.sqlite3` 文件而遗漏 WAL。

项目名称默认取目录名，同一 Git 仓库的 worktree 归入同一项目，非 Git 目录按指定目录识别。`task_key` 与项目身份共同定位任务；同名目录和任务标题不用于去重。

应用面向单机本地使用，不包含云同步、远程 Agent、日历提醒和插件市场分发。当前提供文字录入，不包含语音转文字、自动唤起或派发 Agent。久未更新提示只帮助查看已有记录，不提供 Agent 存活检测或后台催办。

[验收记录](docs/VERIFICATION.md) 记录各版本的实测范围并保留历史证据。本地 MCP 自检、客户端连接、Windows 注册项/窗口行为和用户日常体验分别记录；未执行的项目不标作通过。
