use crate::{
    changed_at, check_expected, default_limit, optional_non_null, read_task, validate_task_id,
    validate_text, ArchiveById, Database, Error, ListedTask, Result, TaskPage, TaskReceipt,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// GUI-only archived-task search. It never changes the default MCP list scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveQuery {
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub query: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

impl Default for ArchiveQuery {
    fn default() -> Self {
        Self {
            query: None,
            limit: default_limit(),
            offset: 0,
        }
    }
}

impl Database {
    /// All archived statuses, newest change first. Restart pagination after a restore
    /// or refresh: offset pagination describes the current database, not a saved snapshot.
    pub fn list_archived(&self, input: ArchiveQuery) -> Result<TaskPage> {
        if !(1..=100).contains(&input.limit) {
            return Err(Error::InvalidInput(
                "limit must be between 1 and 100".into(),
            ));
        }
        let query = input
            .query
            .as_deref()
            .map(|query| {
                validate_text("query", query, 1, 160)?;
                Ok::<_, Error>(format!(
                    "%{}%",
                    query
                        .trim()
                        .replace('\\', "\\\\")
                        .replace('%', "\\%")
                        .replace('_', "\\_")
                ))
            })
            .transpose()?;
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT t.*,p.name AS project_name,p.path AS project_path
             FROM tasks t JOIN projects p ON t.project_id=p.id
             WHERE t.archived=1
               AND (?1 IS NULL OR t.title LIKE ?1 ESCAPE '\\'
                    OR p.name LIKE ?1 ESCAPE '\\' OR p.path LIKE ?1 ESCAPE '\\'
                    OR t.task_key LIKE ?1 ESCAPE '\\' OR t.goal LIKE ?1 ESCAPE '\\'
                    OR t.progress LIKE ?1 ESCAPE '\\' OR t.request LIKE ?1 ESCAPE '\\'
                    OR t.user_note LIKE ?1 ESCAPE '\\')
             ORDER BY t.updated_at DESC,t.id DESC
             LIMIT ?2 OFFSET ?3",
        )?;
        let mut items = statement
            .query_map(params![query, input.limit + 1, input.offset], |row| {
                Ok(ListedTask {
                    task: read_task(row)?,
                    project_name: row.get("project_name")?,
                    project_path: row.get("project_path")?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let has_more = items.len() > input.limit as usize;
        items.truncate(input.limit as usize);
        Ok(TaskPage {
            items,
            next_offset: if has_more {
                input.offset.checked_add(input.limit)
            } else {
                None
            },
        })
    }

    /// Restore the existing row as a human action. Project identity, task contents,
    /// reports, review state, and Agent activity remain unchanged. A fresh updated_at
    /// also restarts the inactivity period for accepted tasks' automatic archiving.
    pub fn restore_by_id(&self, input: ArchiveById) -> Result<TaskReceipt> {
        validate_task_id(input.id)?;
        validate_text("expected_updated_at", &input.expected_updated_at, 1, 64)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx
            .query_row("SELECT * FROM tasks WHERE id=?1", [input.id], read_task)
            .optional()?
            .ok_or(Error::TaskNotFound)?;
        check_expected(Some(&input.expected_updated_at), Some(&task))?;
        if !task.archived {
            return Ok(TaskReceipt::from(&task));
        }
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET archived=0,updated_at=?1 WHERE id=?2",
            params![updated_at, task.id],
        )?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt {
            id: task.id,
            status: task.status,
            updated_at,
        })
    }
}
