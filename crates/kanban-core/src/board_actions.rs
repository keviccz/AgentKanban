//! Desktop-only actions: undoing quick board actions, the finished-work summary and
//! display names for projects. None of these count as Agent activity.
use crate::{
    changed_at, read_task, validate_task_id, validate_text, Database, Error, ListedTask,
    ReviewStatus, Result, Status, TaskReceipt,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};

impl Database {
    /// Put a one-click acceptance back to "not reviewed". Only an accepted, visible
    /// done task can go back; anything else changed since and is left alone.
    pub fn undo_accept(&self, id: i64, expected_updated_at: &str) -> Result<TaskReceipt> {
        validate_task_id(id)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx
            .query_row("SELECT * FROM tasks WHERE id=?1", [id], read_task)
            .optional()?
            .ok_or(Error::TaskNotFound)?;
        crate::check_expected(Some(expected_updated_at), Some(&task))?;
        if task.archived || task.status != Status::Done || task.review_status != ReviewStatus::Accepted {
            return Err(Error::NotReviewable);
        }
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET review_status='pending',updated_at=?1 WHERE id=?2",
            params![updated_at, id],
        )?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt { id, status: task.status, updated_at })
    }

    /// Restore tasks archived moments ago (undo of a project archive). Rows that were
    /// restored or changed in between are skipped.
    pub fn restore_many(&self, ids: &[i64]) -> Result<usize> {
        if ids.len() > 10_000 || ids.iter().any(|id| *id <= 0) {
            return Err(Error::InvalidInput("ids must be positive task identifiers".into()));
        }
        let list = serde_json::to_string(ids).map_err(|err| Error::InvalidInput(err.to_string()))?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let restored = tx.execute(
            "UPDATE tasks SET archived=0,updated_at=?1
             WHERE archived=1 AND id IN (SELECT value FROM json_each(?2))",
            params![changed_at(None), list],
        )?;
        if restored > 0 {
            tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        }
        tx.commit()?;
        Ok(restored)
    }

    /// Finished work whose last Agent report is at or after `since`, archived or not,
    /// for the summary panel. Newest first within each project.
    pub fn finished_since(&self, since: &str) -> Result<Vec<ListedTask>> {
        validate_text("since", since, 1, 64)?;
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT t.*,p.name AS project_name,p.path AS project_path
             FROM tasks t JOIN projects p ON t.project_id=p.id
             WHERE t.status='done' AND COALESCE(t.agent_updated_at,t.updated_at)>=?1
             ORDER BY p.name COLLATE NOCASE,p.id,COALESCE(t.agent_updated_at,t.updated_at) DESC,t.id DESC
             LIMIT 500",
        )?;
        let rows = statement.query_map([since], |row| {
            Ok(ListedTask {
                task: read_task(row)?,
                project_name: row.get("project_name")?,
                project_path: row.get("project_path")?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Change a project's display name. Identity and path stay the same, so Agents
    /// keep reaching the same project; later reports do not rename it back.
    pub fn rename_project(&self, project_id: i64, name: &str) -> Result<()> {
        if project_id <= 0 {
            return Err(Error::InvalidInput("project_id must be a positive project identifier".into()));
        }
        let name = name.trim();
        validate_text("name", name, 1, 80)?;
        if name.chars().any(char::is_control) {
            return Err(Error::InvalidInput("name must be a single line".into()));
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE projects SET name=?1 WHERE id=?2 AND name!=?1",
            params![name, project_id],
        )?;
        if changed == 0 {
            let exists: Option<i64> = tx
                .query_row("SELECT id FROM projects WHERE id=?1", [project_id], |row| row.get(0))
                .optional()?;
            if exists.is_none() {
                return Err(Error::InvalidInput("project not found".into()));
            }
            return Ok(());
        }
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(())
    }
}
