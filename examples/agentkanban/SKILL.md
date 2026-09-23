---
name: agentkanban
description: 在已配置 AgentKanban MCP 的环境中执行会修改文件的任务、通过开工说明交接已有待办或继续已跟踪的任务时使用。自动入板并在阶段完成时维护同一条记录；不用于普通问答、只读审查或用户明确说不用记的任务。
---

# AgentKanban

这是可选的任务同步规则，补充已配置的 AgentKanban MCP。它不替代 MCP 配置、不启动服务器、不增加工具；仅存在于此仓库不会自动安装到客户端或全局目录。

## 记录与继续

1. 会修改文件的任务（代码、配置、文档）无需用户提醒即自动入板；普通问答、只读审查和调研不入板；用户说「不用记」时不记。用户从浮窗复制的开工说明已有项目与任务身份，应接手原记录。
2. 先用 `task_list` 查询当前项目的绝对 `project_path`，默认返回每页 5 条未完成、未归档任务的摘要，按 `next_offset` 继续。需要 `request`、`user_note`、`steps` 与 `review_status` 全文时，按 `task_key` 精确查询。可能已完成或已归档时，按需使用 `include_done` / `include_archived`，精确过滤不会绕过这些范围限制。
3. 找到相符的任务就沿用原 `task_key`；新任务用 `auto:<简短标识>` 或 `feature:export-report` 这类稳定标识。不要以新会话、分支名或修改后的标题新建同一任务。同一仓库的 worktree 由 MCP 归入同一项目。

## 更新同一条任务

创建时用 `steps` 写下计划（最多 12 步），之后只在阶段完成（某个步骤完成）、实际受阻和全部完成时调用 `task_upsert`，每次整份替换 `steps`；不为单次修改或命令更新。状态使用 `todo`、`in_progress`、`blocked`、`done`。`blocked` 必须有阻碍继续工作的真实原因；耗时较久或久未更新本身不是阻塞。工作与必要验证完成后才标 `done`，不要把未运行的验证写成通过。

`task_upsert` 每次都传 `project_path`、`task_key`、`title`、`status`、`progress`。需要保留分支时同时传 `branch`；省略或传 `null` 会清除分支。`agent`、`next_action`、`needs_input`、`deliverables`、`steps` 省略则保留，清空字符串字段用 `""`、清空数组用 `[]`；`agent: null` 仍保留。不要把工具理解为所有字段的完整替换。

`progress` 用一句单行事实概括进展；`agent` 如实填写执行方，`next_action` 说明下一步，`needs_input` 明确需要用户提供什么。阻塞解除后显式清空 `needs_input`，状态变化不会自动清空它。不复制聊天历史、命令流水、完整日志或敏感凭据。内容没有变化时不重复更新，不为消除久未更新提示制造进展。

更新已有任务或归档时，建议把最新查询的 `updated_at` 作为 `expected_updated_at` 传入。冲突表示查询后有其他变化，重新读取需求和人工意见，再合并更新；不要通过省略校验覆盖它。

## 完成与验收

完成时用 `deliverables` 提供最多 5 个 `{label,uri}`，指向实际文件、PR 或其他可定位成果；只报告实际完成的验证。`done` 表示 Agent 执行完成，任务随后等待用户验收。`request`、`user_note`、`review_status` 不能由 MCP 直接写入，不代替用户作出验收结论。

用户退回修改时，任务成为 `todo` 与 `changes_requested`，修改意见在 `user_note`；继续原任务，完成后重新等待验收。用户补充字段仅保留最新一条，不把它当成完整聊天历史。完成后有新工作仍可用原标识重新设为 `in_progress`。

归档保留数据；需要继续已归档任务时，先调用 `task_archive` 并传 `archived: false`，再更新。只在用户要求整理归档时归档任务；归档不等于完成或验收通过。

## 失败与边界

仅使用 `task_upsert`、`task_list`、`task_archive` 操作任务，不直接读取或写入数据库，不绕过 MCP。收到工具成功结果后才确认已同步；工具不可用或失败时，简短说明未同步及具体原因，保留原任务标识。

MCP 由客户端按需启动，GUI 不运行仍可写入，无需让 Coding Agent 随 Windows 启动。创建待办、复制开工说明或此 Skill 都不会自动派发或唤起 Agent。本地自检通过只说明本机 MCP 能响应，不能据此宣称当前客户端已连接；连接应以该客户端加载并成功调用工具的结果为准。
