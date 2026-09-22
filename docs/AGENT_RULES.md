# AgentKanban 同步规则

- 用户明确要求加入看板才创建任务，普通问答不入板；从浮窗接手已有任务时不重复创建。复制开工说明不会自动启动 Agent。
- 开始或跨会话继续时，先按 `project_path` 用 `task_list` 查询，已知 `task_key` 时精确过滤，读取 `request`、`user_note` 和 `review_status`。沿用原 `task_key`；必要时包含已完成或已归档项。
- 只在开始、实质进展、真实受阻或完成时更新。`blocked` 需有实际阻碍；用 `next_action` 写下一步、`needs_input` 写需要用户补充什么，解决后显式清空。久未更新不自动改变状态。
- `task_upsert` 每次传 `project_path`、`task_key`、`title`、`status`、`progress`；保留分支需同时传 `branch`，省略或 `null` 会清空。`agent`、`next_action`、`needs_input`、`deliverables` 省略则保留；清空用空字符串或空数组，`agent: null` 仍保留。
- 更新和归档已有任务时建议带最近读取的 `expected_updated_at`；冲突后重新查询并合并新反馈，不去掉校验强行覆盖。
- `progress` 用一句单行事实，不记录聊天、命令流水、完整日志或敏感凭据；无变化不刷更新时间。`agent` 如实填写执行方，不能据此宣称仍在线。
- 工作与必要验证完成后才标 `done`，用 `deliverables` 提供可定位的成果，并说明未验证范围。执行完成后等待用户验收；不能通过 MCP 修改用户需求、意见或验收结论。退回修改沿用原任务继续。
- 归档保留数据。继续已归档任务时先用 `task_archive(archived=false)` 恢复，再更新原记录；归档不等于完成或验收通过。
- 仅通过 MCP 工具操作，不直接写数据库。工具失败时说明未同步；收到成功结果后再确认同步。
