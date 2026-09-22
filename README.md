# AgentKanban

由 Coding Agent 维护的 Windows 桌面任务浮窗。你说「把这个功能加入看板，后续同步进展」，Agent 通过本地 MCP 创建任务，并在开始、阶段变化、受阻和完成时更新同一条记录。

看板按项目展示待办、进行中、受阻和已完成任务。文字录入交给 Agent；浮窗负责查看、筛选和折叠。应用本身不调用模型 API。

## 启动

安装构建产物中的 Windows 安装包，然后从开始菜单打开 AgentKanban。安装目录内同时包含 `agentkanban.exe` 和 `agentkanban-mcp.exe`；后者由客户端按需启动，无须单独双击。

交付文件在 `release/`：`AgentKanban_0.1.0_x64-setup.exe` 是安装包，`AgentKanban-0.1.0-windows-x64.zip` 是免安装包。免安装包解压后运行 `AgentKanban/agentkanban.exe`，仍使用统一的本地数据目录。

默认浮窗约 380 × 520，支持拖动、缩放、置顶开关和明暗主题。收起后显示进行中与受阻数量。关闭按钮隐藏到托盘；通过托盘菜单重新显示或退出。主题、窗口位置、尺寸和展开状态会保留。

每个项目默认预览三条未完成任务，其余点击展开；已完成默认折叠。内容超出时在窗口内滚动，任务更新不会强制展开项目。

## 连接 Agent

客户端连接的是本机的 `agentkanban-mcp.exe`。先确定可执行文件的实际绝对路径，再按 [MCP 接入说明](docs/MCP.md) 合并配置。仓库提供 [Codex](examples/codex.toml)、[Claude Code](examples/claude-code.mcp.json) 和 [Cursor](examples/cursor.mcp.json) 示例，不会自动修改任何客户端配置。

也可以用 PowerShell 7 生成含本机路径的独立示例文件：

```powershell
pwsh -NoProfile -File .\scripts\write-client-examples.ps1 -Executable "$env:LOCALAPPDATA\AgentKanban\agentkanban-mcp.exe"
```

如选择了其他安装位置，替换 `-Executable`。生成文件位于 `artifacts/client-config/`，仍需合并到需要使用的客户端。

在连接后的 Agent 中明确说：

> 把当前项目的「导出报告」功能加入 AgentKanban，task_key 用 `feature:export-report`，后续在有实质进展时更新同一条任务。

后续会话先按项目查询未完成项，继续原 `task_key`。普通问答不入板。「进行中」代表 Agent 最后上报的状态；Agent 意外退出不会自动变成完成，更新时间用于判断记录的新鲜程度。

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

v0.1 面向单机本地使用，不包含云同步、远程 Agent、日历提醒和插件市场分发。自动检查、客户端现场连接、Windows 窗口行为及用户体验验收分别记录，未执行的项目不会标作已通过。
