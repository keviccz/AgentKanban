---
name: agentkanban
description: 用户明确要求将功能任务加入 AgentKanban、同步进展，或继续已授权跟踪的任务时使用。通过已配置的 AgentKanban MCP 查询和维护同一条记录，不用于普通问答或自动收集全部工作。
---

# AgentKanban

这是可选的任务同步规则，补充已配置的 AgentKanban MCP。它不替代 MCP 配置、不启动服务器、不增加工具；仅存在于此仓库不会自动安装到客户端或全局目录。

## 记录与继续

1. 用户明确要求把某个功能加入看板时才创建。此前已授权跟踪的同一任务，后续会话继续同步；普通问答不入板。
2. 先用 `task_list` 查询当前项目的绝对 `project_path`，查找原任务。默认每页 20 条未完成、未归档记录，按返回的 `next_offset` 继续；可能已完成或已归档时，按需使用 `include_done` / `include_archived`。
3. 沿用原 `task_key`；新任务选择简短、稳定的功能标识，例如 `feature:export-report`。不要以新会话、分支名或修改后的标题新建同一任务。同一仓库的 worktree 由 MCP 归入同一项目。

## 更新同一条任务

只在开始执行、有实质进展、实际受阻和完成时调用 `task_upsert`。状态使用 `todo`、`in_progress`、`blocked`、`done`。`blocked` 必须有阻碍继续工作的真实原因；耗时较久或久未更新本身不是阻塞。工作与必要验证完成后才标 `done`，不要把未运行的验证写成通过。

`task_upsert` 完整替换可见字段，每次都传 `project_path`、`task_key`、`title`、`status`、`progress`。需要保留分支时同时传 `branch`；省略或传 `null` 会清除分支。`progress` 用一句单行事实概括结果、下一阶段或阻塞，不复制聊天历史、命令流水、完整日志或敏感凭据。内容没有变化时不重复更新，不为消除久未更新提示制造进展。

完成后有新工作仍可用原标识重新设为 `in_progress`。归档保留数据；需要继续已归档任务时，先调用 `task_archive` 并传 `archived: false`，再更新。只在用户要求整理归档时归档任务。

## 失败与边界

仅使用 `task_upsert`、`task_list`、`task_archive` 操作任务，不直接读取或写入数据库，不绕过 MCP。收到工具成功结果后才确认已同步；工具不可用或失败时，简短说明未同步及具体原因，保留原任务标识。

MCP 由客户端按需启动，GUI 不运行仍可写入，无需让 Coding Agent 随 Windows 启动。本地自检通过只说明本机 MCP 能响应，不能据此宣称当前客户端已连接；连接应以该客户端加载并成功调用工具的结果为准。
