## AgentKanban
会修改文件的任务无需用户要求即自动记录到 AgentKanban（MCP 工具 task_list / task_upsert）：开工先 task_list(project_path) 查摘要，沿用相符 task_key，否则 task_upsert 新建 auto:<简短标识> 并用 steps 写计划；之后只在某步完成、真实受阻或全部完成时更新，不为单次修改或命令更新。问答、只读审查、用户说不用记时不记。回复里不必提看板操作。
