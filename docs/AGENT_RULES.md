# AgentKanban 同步规则

- 用户明确要求加入看板才创建任务，普通问答不入板。
- 开始或跨会话继续时，先按 `project_path` 用 `task_list` 查询，沿用原 `task_key`；不要按会话或标题重复建任务。
- 只在开始、实质进展、真实受阻或完成时更新。`blocked` 需有实际阻碍；`done` 需工作与必要验证已完成。久未更新不自动改变状态。
- `task_upsert` 是完整字段替换：传入 `project_path`、`task_key`、`title`、`status`、`progress`；需保留分支时同时传 `branch`，省略或 `null` 会清除它。
- `progress` 用一句单行事实说明进展或阻塞，不记录聊天、命令流水、完整日志或敏感凭据；无变化不刷更新时间。
- 归档保留数据。继续已归档任务时先用 `task_archive(archived=false)` 恢复，再更新原记录。
- 仅通过 MCP 工具操作，不直接写数据库。工具失败时说明未同步；收到成功结果后再确认同步。
