---
name: agentkanban
description: 在已配置 AgentKanban MCP 的环境中执行会修改文件的任务、通过开工说明交接已有待办或继续已跟踪的任务时使用。自动入板并在阶段完成时维护同一条记录；不用于普通问答、只读审查或用户明确说不用记的任务。
---

# AgentKanban

这是可选的任务同步规则，补充已配置的 AgentKanban MCP。它不替代 MCP 配置、不启动服务器、不增加工具；仅存在于此仓库不会自动安装到客户端或全局目录。

## 记录与继续

1. 会修改文件的任务（代码、配置、文档）无需用户提醒即自动入板；普通问答、只读审查和调研不入板；用户说「不用记」时不记。用户从浮窗复制的开工说明已有项目与任务身份，应接手原记录。
2. 已知 `task_key` 时直接按项目和 key 精读；未知时用 `task_list(project_path, query=任务关键词)` 定向搜索，不逐页扫描整张看板。摘要带 `has_request` / `has_user_note` 时有原始需求或用户意见；接手已有任务前必须精读。精确 key 默认包括完成和归档任务，显式 `include_done:false` / `include_archived:false` 才排除。
3. 找到相符的任务就沿用原 `task_key`；新任务用 `auto:<简短标识>` 或 `feature:export-report` 这类稳定标识。不要以新会话、分支名或修改后的标题新建同一任务。同一仓库的 worktree 由 MCP 归入同一项目。
4. 同一任务由一个主 Agent 统一记账，子 Agent 返回结果、不重复入板。独立任务可并行，但各用独立标识。

## 更新同一条任务

创建时写 `goal`（做什么）、`acceptance`（用户怎么验收，最多 8 条），用 `steps` 写计划（最多 12 步）。只在阶段完成、实际受阻和全部完成时更新；推进已有计划用 `step_updates:[{index:0,status:"done",note:"结果"}]` 只发变化项，索引从 0 起，备注省略保留、空字符串清除。初始化或重排才整份传 `steps`，不能和 `step_updates` 同时发送。不为单次修改或命令更新。状态仍使用 `todo`、`in_progress`、`blocked`、`done`；仅真实障碍标受阻，工作与必要验证完成后才标完成。

`task_upsert` 每次都传 `project_path`、`task_key`、`title`、`status`、`progress`。需要保留分支时同时传 `branch`；省略或传 `null` 会清除分支。`agent`、`next_action`、`needs_input`、`deliverables`、`steps`、`goal`、`acceptance` 省略则保留，清空字符串字段用 `""`、清空数组用 `[]`；`agent: null` 仍保留。不要把工具理解为所有字段的完整替换。

`progress` 用一句单行事实概括进展；`agent` 如实填写执行方，`next_action` 说明下一步，`needs_input` 明确需要用户提供什么。阻塞解除后显式清空 `needs_input`，状态变化不会自动清空它。不复制聊天历史、命令流水、完整日志或敏感凭据。内容没有变化时不重复更新，不为消除久未更新提示制造进展。

已有任务实质更新或归档时，必须把最新回执或查询的 `updated_at` 作为 `expected_updated_at`。无版本号只允许新建或完全幂等的重试；并行创建撞名会返回冲突，不会覆盖另一方。冲突后按原 key 精读需求和人工意见再合并，不省略校验。

暂停期间继续工作，不重试或轮询；下个正常里程碑或新任务再尝试一次。用户恢复后在下一次正常调用生效，无需回补暂停期间的逐条操作。

## 完成与验收

完成时用 `deliverables` 提供最多 5 个 `{label,uri}`，指向实际文件、PR 或其他可定位成果；只报告实际完成的验证。`done` 表示 Agent 执行完成，任务随后等待用户验收。`request`、`user_note`、`review_status` 不能由 MCP 直接写入，不代替用户作出验收结论。

用户退回修改时，任务成为 `todo` 与 `changes_requested`，修改意见在 `user_note`；继续原任务，完成后重新等待验收。用户补充字段仅保留最新一条，不把它当成完整聊天历史。完成后有新工作仍可用原标识重新设为 `in_progress`。

归档保留数据；需要继续已归档任务时，先调用 `task_archive` 并传 `archived: false`，再更新。只在用户要求整理归档时归档任务；归档不等于完成或验收通过。

## 失败与边界

通常使用 `task_upsert`、`task_list`、`task_archive` 操作任务，不直接读取或写入数据库。仅 MCP 传输连接中断时，可按原客户端配置的同一可执行程序和环境（尤其 `AGENTKANBAN_DATA_DIR`），运行 `agentkanban-mcp --call <原工具名> --input-file <UTF-8 JSON 参数文件>`。参数与 MCP 相同；`--input-file -` 从标准输入读取。先按项目和原 `task_key` 精读，包括用户意见，再携带最新 `expected_updated_at` 合并当前进度。仍由原主 Agent 在正常里程碑记录，不创建替代任务、不轮询、不回放逐条操作。

备用入口和 MCP 共用校验、暂停、版本检查及历史记录。冲突、非法参数、暂停、权限拒绝或用户明确禁用工具不是传输断线，不能改走备用入口规避。不要猜测另一个程序或数据目录。两种入口都失败时，简短说明未同步及具体原因，在交接中保留项目、原 key 和最后可确认的进展；恢复后先精读最新状态，合并当前事实，不补造历史。只有成功结果且没有 `paused:true` / `recorded:false`，才可确认本次已同步。

MCP 由客户端按需启动，GUI 不运行仍可写入，无需让 Coding Agent 随 Windows 启动。创建待办、复制开工说明或此 Skill 都不会自动派发或唤起 Agent。本地自检通过只说明本机 MCP 能响应，不能据此宣称当前客户端已连接；连接应以该客户端加载并成功调用工具的结果为准。
