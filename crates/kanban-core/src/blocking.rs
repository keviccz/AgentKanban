use crate::{project::resolve_project, Database, Error, Result};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

/// JSON array of project identities the user excluded from AgentKanban.
pub(crate) const BLOCKED_PROJECTS: &str = "blocked_projects";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedProject {
    pub id: i64,
    pub name: String,
    pub path: String,
}

fn parse(value: Option<String>) -> Vec<String> {
    value
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

impl Database {
    fn blocked_identities(&self) -> Result<Vec<String>> {
        Ok(parse(self.get_setting(BLOCKED_PROJECTS)?))
    }

    /// Blocked projects that still have a row, in name order, for Settings.
    pub fn blocked_projects(&self) -> Result<Vec<BlockedProject>> {
        let identities = serde_json::to_string(&self.blocked_identities()?)
            .map_err(|err| Error::InvalidInput(err.to_string()))?;
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT id,name,path FROM projects
             WHERE identity IN (SELECT value FROM json_each(?1))
             ORDER BY name COLLATE NOCASE,id",
        )?;
        let rows = statement.query_map([identities], |row| {
            Ok(BlockedProject {
                id: row.get(0)?,
                name: row.get(1)?,
                path: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Block or unblock by project row. Blocking hides the project from the board and
    /// makes every later Agent call for it a no-op; its tasks stay in the database.
    pub fn set_project_blocked(&self, project_id: i64, blocked: bool) -> Result<()> {
        if project_id <= 0 {
            return Err(Error::InvalidInput(
                "project_id must be a positive project identifier".into(),
            ));
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let identity: String = tx
            .query_row(
                "SELECT identity FROM projects WHERE id=?1",
                [project_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| Error::InvalidInput("project not found".into()))?;
        let mut identities = parse(
            tx.query_row(
                "SELECT value FROM settings WHERE key=?1",
                [BLOCKED_PROJECTS],
                |row| row.get(0),
            )
            .optional()?,
        );
        let present = identities.contains(&identity);
        if present == blocked {
            return Ok(());
        }
        if blocked {
            identities.push(identity);
        } else {
            identities.retain(|item| item != &identity);
        }
        let value = serde_json::to_string(&identities)
            .map_err(|err| Error::InvalidInput(err.to_string()))?;
        tx.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![BLOCKED_PROJECTS, value],
        )?;
        // The board query filters blocked projects, so the view must refresh.
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Whether an Agent call for this path belongs to a blocked project. A path that
    /// cannot be resolved is left to the normal validation error.
    pub fn is_path_blocked(&self, path: &str) -> Result<bool> {
        let identities = self.blocked_identities()?;
        if identities.is_empty() {
            return Ok(false);
        }
        Ok(resolve_project(path).is_ok_and(|project| identities.contains(&project.identity)))
    }
}
