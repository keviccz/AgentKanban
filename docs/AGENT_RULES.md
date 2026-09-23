# AgentKanban 同步规则

- 会修改文件的任务（代码、配置、文档）无需用户提醒，自动入板；普通问答、只读审查和调研不入板；用户说「不用记」时不记。从浮窗接手已有任务时不重复创建。
- 开始时先按 `project_path` 用 `task_list` 查询摘要，找到相符的任务就沿用原 `task_key`；没有再新建 `auto:<简短标识>`。需要 `request`、`user_note`、`steps` 等全文时按 `task_key` 精确查询。
- 开工时用 `steps` 写下计划步骤（最多 12 步），之后整份替换更新。
- 只在阶段完成（某个步骤完成）、真实受阻或全部完成时更新；不为单次修改、单条命令或无变化的状态更新。`blocked` 需有实际阻碍；用 `needs_input` 写需要用户补充什么，解决后显式清空。
- `task_upsert` 每次传 `project_path`、`task_key`、`title`、`status`、`progress`；保留分支需同时传 `branch`，省略或 `null` 会清空。`agent`、`next_action`、`needs_input`、`deliverables`、`steps` 省略则保留；清空用空字符串或空数组。
- 更新时带上次回执或查询的 `updated_at` 作为 `expected_updated_at`；冲突后按 `task_key` 重新查询并合并新反馈，不去掉校验强行覆盖。
- `progress` 用一句单行事实，不记录聊天、命令流水、完整日志或敏感凭据。回复用户时不必复述看板操作，失败时再说明。
- 工作与必要验证完成后才标 `done`，用 `deliverables` 提供可定位的成果。`done` 等待用户验收；不能通过 MCP 修改用户需求、意见或验收结论。
- 归档保留数据；继续已归档任务时先用 `task_archive(archived=false)` 恢复。仅通过 MCP 工具操作，不直接写数据库。
