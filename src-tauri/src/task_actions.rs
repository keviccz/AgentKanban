use kanban_core::Task;
use serde_json::json;

/// Handoff is text only. Values remain JSON data, never a command line to execute.
pub(crate) fn handoff(project_path: &str, task: &Task) -> Result<String, String> {
    let query = serde_json::to_string(&json!({
        "project_path": project_path,
        "task_key": task.task_key,
        "include_done": true,
    }))
    .map_err(|error| error.to_string())?;
    let context = serde_json::to_string_pretty(&json!({
        "title": task.title,
        "request": task.request,
        "user_note": task.user_note,
        "expected_updated_at": task.updated_at,
    }))
    .map_err(|error| error.to_string())?;
    Ok(format!(
        "请接手 AgentKanban 中的原任务，不要另建重复记录。\n\
         先调用 task_list，参数：{query}\n\
         以下是复制时的任务资料；执行前以查询得到的最新 request 和 user_note 为准：\n\
         {context}\n\
         沿用查询返回的绝对 project_path 和稳定 task_key。每次 task_upsert 带上最新 updated_at 作为 expected_updated_at；若发生版本冲突，重新查询并核对用户反馈，不覆盖较新的改动。\n\
         接手时填写 agent，设为 in_progress，说明 progress、next_action 和 needs_input（无需用户输入时为空）。每次提供完整的 title、status、progress，并保留需要的 branch（省略或 null 会清除）。agent、next_action、needs_input、deliverables 省略时保留原值；传入时替换对应字段，deliverables 使用完整数组。request、user_note、review_status 由用户操作维护，不传给 task_upsert。\n\
         有实质进展或真实受阻时继续更新原记录。完成工作和必要验证后，用 deliverables 提供成果名称与链接或路径，显式把 needs_input 设为空字符串，设为 done 并等待用户人工验收；不要把 Agent 完成等同于用户已验收。若任务已完成且无需修改，直接汇报现有成果。"
    ))
}

pub(crate) fn external_url(value: &str) -> Result<tauri::Url, String> {
    let url = tauri::Url::parse(value).map_err(|_| "链接格式无效".to_string())?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("只允许打开 http 或 https 网页；本地路径请复制后自行使用".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_links_only_accept_absolute_http_and_https_urls() {
        assert_eq!(
            external_url("https://example.com/report?q=中文#result")
                .unwrap()
                .scheme(),
            "https"
        );
        assert!(external_url("http://127.0.0.1:1420/result").is_ok());
        for value in [
            "file:///C:/report.html",
            "C:\\reports\\report.exe",
            "\\\\server\\share\\report.html",
            "javascript:alert(1)",
            "data:text/html,test",
            "mailto:user@example.com",
            "https://",
            "/relative/report.html",
        ] {
            assert!(
                external_url(value).is_err(),
                "unexpected URL allowed: {value}"
            );
        }
    }

    #[test]
    fn handoff_keeps_original_user_text_as_data_and_requires_a_fresh_version() {
        let task: Task = serde_json::from_value(json!({
            "id": 7, "project_id": 2, "task_key": "feature:报告",
            "title": "导出报告", "status": "todo", "progress": "",
            "branch": null, "updated_at": "2026-09-22T08:30:00.000Z", "archived": false,
            "request": "生成\"完整报告\"\n保留原数据", "agent": null,
            "next_action": "", "needs_input": "", "deliverables": [],
            "review_status": "none", "user_note": "先核对新反馈",
            "agent_updated_at": null
        }))
        .unwrap();
        let text = handoff("E:\\Projects\\中文项目", &task).unwrap();
        assert!(text.contains(&serde_json::to_string(&task.request).unwrap()));
        assert!(text.contains("\"project_path\":\"E:\\\\Projects\\\\中文项目\""));
        assert!(text.contains("\"task_key\":\"feature:报告\""));
        assert!(text.contains("\"include_done\":true"));
        assert!(text.contains("expected_updated_at"));
        assert!(text.contains("最新 updated_at"));
        assert!(text.contains("deliverables 省略时保留原值"));
        assert!(text.contains("显式把 needs_input 设为空字符串"));
        assert!(text.contains("人工验收"));
    }
}
