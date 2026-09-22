//! Shared local storage for the desktop app and independent MCP processes.

mod project;

use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub use project::{resolve_project, ProjectIdentity};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Task not found; check project_path and task_key")]
    TaskNotFound,
    #[error("Task is archived; restore it with task_archive(archived=false) before updating")]
    TaskArchived,
    #[error("Cannot identify project: {0}")]
    ProjectIdentity(String),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cannot find a local application data directory; set AGENTKANBAN_DATA_DIR")]
    DataDirectoryUnavailable,
    #[error("Database schema version {0} is newer than this application supports")]
    NewerSchema(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    Blocked,
    Done,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::Done => "done",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    pub task_key: String,
    pub title: String,
    pub status: Status,
    pub progress: String,
    pub branch: Option<String>,
    pub updated_at: String,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectBoard {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub revision: i64,
    pub projects: Vec<ProjectBoard>,
}

/// Full replacement of the visible task fields; omitted/null branch clears it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpsertTask {
    pub project_path: String,
    pub task_key: String,
    pub title: String,
    pub status: Status,
    pub progress: String,
    #[serde(default)]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTasks {
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub project_path: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<Status>,
    #[serde(default)]
    pub include_done: bool,
    #[serde(default)]
    pub include_archived: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

const fn default_limit() -> u32 {
    20
}

fn optional_non_null<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl Default for ListTasks {
    fn default() -> Self {
        Self {
            project_path: None,
            status: None,
            include_done: false,
            include_archived: false,
            limit: default_limit(),
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveTask {
    pub project_path: String,
    pub task_key: String,
    #[serde(default = "default_archived")]
    pub archived: bool,
}

const fn default_archived() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskReceipt {
    pub id: i64,
    pub status: Status,
    pub updated_at: String,
}

impl From<&Task> for TaskReceipt {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id,
            status: task.status,
            updated_at: task.updated_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListedTask {
    #[serde(flatten)]
    pub task: Task,
    pub project_name: String,
    pub project_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPage {
    pub items: Vec<ListedTask>,
    pub next_offset: Option<u32>,
}

/// A path, not a shared connection: each call opens its own bounded-wait connection.
/// This makes clones safe across GUI threads and independent MCP processes.
#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn default_data_dir() -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("AGENTKANBAN_DATA_DIR").filter(|s| !s.is_empty()) {
            return Ok(PathBuf::from(path));
        }
        if let Some(path) = std::env::var_os("LOCALAPPDATA").filter(|s| !s.is_empty()) {
            return Ok(PathBuf::from(path).join("AgentKanban"));
        }
        #[cfg(not(windows))]
        {
            if let Some(path) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
                return Ok(PathBuf::from(path).join("AgentKanban"));
            }
            if let Some(path) = std::env::var_os("HOME").filter(|s| !s.is_empty()) {
                return Ok(PathBuf::from(path).join(".local/share/AgentKanban"));
            }
        }
        Err(Error::DataDirectoryUnavailable)
    }

    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_data_dir()?.join("agentkanban.sqlite3"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(Error::InvalidInput("database must be a file path".into()));
        }
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Self { path };
        let mut conn = db.connect()?;
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::InvalidInput(
                "database filesystem does not support SQLite WAL".into(),
            ));
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 1 {
            return Err(Error::NewerSchema(version));
        }
        if version == 0 {
            tx.execute_batch(
                "CREATE TABLE projects (
                    id INTEGER PRIMARY KEY,
                    identity TEXT NOT NULL UNIQUE,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL
                );
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id),
                    task_key TEXT NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('todo','in_progress','blocked','done')),
                    progress TEXT NOT NULL,
                    branch TEXT,
                    updated_at TEXT NOT NULL,
                    archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0,1)),
                    UNIQUE(project_id,task_key)
                );
                CREATE INDEX tasks_project_state ON tasks(project_id,archived,status);
                CREATE TABLE metadata (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                INSERT INTO metadata(key,value) VALUES ('revision',0);
                CREATE TABLE settings (key TEXT PRIMARY KEY,value TEXT NOT NULL);
                PRAGMA user_version=1;",
            )?;
        }
        tx.commit()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // FULL keeps committed progress durable through power loss as well as process exit.
        conn.pragma_update(None, "synchronous", "FULL")?;
        Ok(conn)
    }

    pub fn revision(&self) -> Result<i64> {
        Ok(self.connect()?.query_row(
            "SELECT value FROM metadata WHERE key='revision'",
            [],
            |row| row.get(0),
        )?)
    }

    /// The latest reported task change, including archived tasks; not a live-agent heartbeat.
    pub fn last_task_update(&self) -> Result<Option<String>> {
        Ok(self
            .connect()?
            .query_row("SELECT MAX(updated_at) FROM tasks", [], |row| row.get(0))?)
    }

    pub fn board(&self) -> Result<BoardSnapshot> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let revision = tx.query_row(
            "SELECT value FROM metadata WHERE key='revision'",
            [],
            |row| row.get(0),
        )?;
        let mut projects = Vec::new();
        {
            let mut statement = tx.prepare(
                "SELECT id,name,path FROM projects p
                 WHERE EXISTS (SELECT 1 FROM tasks t WHERE t.project_id=p.id AND t.archived=0)
                 ORDER BY name COLLATE NOCASE,id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(ProjectBoard {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    tasks: Vec::new(),
                })
            })?;
            for row in rows {
                projects.push(row?);
            }
        }
        {
            let mut statement = tx.prepare(
                "SELECT id,project_id,task_key,title,status,progress,branch,updated_at,archived
                 FROM tasks WHERE project_id=?1 AND archived=0
                 ORDER BY CASE status WHEN 'in_progress' THEN 0 WHEN 'blocked' THEN 1 WHEN 'todo' THEN 2 ELSE 3 END,
                          updated_at DESC,id DESC"
            )?;
            for project in &mut projects {
                project.tasks = statement
                    .query_map([project.id], read_task)?
                    .collect::<std::result::Result<_, _>>()?;
            }
        }
        tx.commit()?;
        Ok(BoardSnapshot { revision, projects })
    }

    pub fn upsert(&self, input: UpsertTask) -> Result<TaskReceipt> {
        validate_text("task_key", &input.task_key, 1, 160)?;
        validate_text("title", &input.title, 1, 200)?;
        validate_text("progress", &input.progress, 0, 600)?;
        if let Some(branch) = &input.branch {
            validate_text("branch", branch, 1, 200)?;
        }
        let project = resolve_project(&input.project_path)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO projects(identity,name,path) VALUES (?1,?2,?3) ON CONFLICT(identity) DO NOTHING",
            params![project.identity, project.name, project.path])?;
        let project_id: i64 = tx.query_row(
            "SELECT id FROM projects WHERE identity=?1",
            [&project.identity],
            |row| row.get(0),
        )?;
        let existing = tx.query_row(
            "SELECT id,project_id,task_key,title,status,progress,branch,updated_at,archived FROM tasks WHERE project_id=?1 AND task_key=?2",
            params![project_id, input.task_key], read_task
        ).optional()?;
        if let Some(ref task) = existing {
            if task.archived {
                return Err(Error::TaskArchived);
            }
            if task.title == input.title
                && task.status == input.status
                && task.progress == input.progress
                && task.branch == input.branch
            {
                return Ok(TaskReceipt::from(task));
            }
        }
        let updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let id = if let Some(task) = existing {
            tx.execute(
                "UPDATE tasks SET title=?1,status=?2,progress=?3,branch=?4,updated_at=?5 WHERE id=?6",
                params![input.title, input.status.as_str(), input.progress, input.branch, updated_at, task.id]
            )?;
            task.id
        } else {
            tx.execute(
                "INSERT INTO tasks(project_id,task_key,title,status,progress,branch,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![project_id, input.task_key, input.title, input.status.as_str(), input.progress, input.branch, updated_at]
            )?;
            tx.last_insert_rowid()
        };
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt {
            id,
            status: input.status,
            updated_at,
        })
    }

    pub fn list(&self, input: ListTasks) -> Result<TaskPage> {
        if !(1..=100).contains(&input.limit) {
            return Err(Error::InvalidInput(
                "limit must be between 1 and 100".into(),
            ));
        }
        let identity = input
            .project_path
            .as_deref()
            .map(resolve_project)
            .transpose()?
            .map(|project| project.identity);
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT t.id,t.project_id,t.task_key,t.title,t.status,t.progress,t.branch,t.updated_at,t.archived,p.name,p.path
             FROM tasks t JOIN projects p ON t.project_id=p.id
             WHERE (?1 IS NULL OR p.identity=?1)
               AND (?2 IS NULL OR t.status=?2)
               AND (?3 OR t.status!='done')
               AND (?4 OR t.archived=0)
             ORDER BY CASE t.status WHEN 'in_progress' THEN 0 WHEN 'blocked' THEN 1 WHEN 'todo' THEN 2 ELSE 3 END,
                      t.updated_at DESC,t.id DESC
             LIMIT ?5 OFFSET ?6"
        )?;
        // An explicit done filter must work without also requiring include_done=true.
        let include_done = input.include_done || input.status == Some(Status::Done);
        let mut items: Vec<ListedTask> = statement
            .query_map(
                params![
                    identity,
                    input.status.map(Status::as_str),
                    include_done,
                    input.include_archived,
                    input.limit + 1,
                    input.offset
                ],
                |row| {
                    Ok(ListedTask {
                        task: read_task(row)?,
                        project_name: row.get(9)?,
                        project_path: row.get(10)?,
                    })
                },
            )?
            .collect::<std::result::Result<_, _>>()?;
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

    pub fn archive(&self, input: ArchiveTask) -> Result<TaskReceipt> {
        validate_text("task_key", &input.task_key, 1, 160)?;
        let project = resolve_project(&input.project_path)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx.query_row(
            "SELECT t.id,t.project_id,t.task_key,t.title,t.status,t.progress,t.branch,t.updated_at,t.archived
             FROM tasks t JOIN projects p ON t.project_id=p.id WHERE p.identity=?1 AND t.task_key=?2",
            params![project.identity, input.task_key], read_task
        ).optional()?.ok_or(Error::TaskNotFound)?;
        if task.archived == input.archived {
            return Ok(TaskReceipt::from(&task));
        }
        let updated_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        tx.execute(
            "UPDATE tasks SET archived=?1,updated_at=?2 WHERE id=?3",
            params![input.archived, updated_at, task.id],
        )?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt {
            id: task.id,
            status: task.status,
            updated_at,
        })
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        validate_text("setting key", key, 1, 128)?;
        Ok(self
            .connect()?
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        validate_text("setting key", key, 1, 128)?;
        if value.len() > 65_536 {
            return Err(Error::InvalidInput("setting value exceeds 64 KiB".into()));
        }
        self.connect()?.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value!=excluded.value",
            params![key, value]
        )?;
        Ok(())
    }
}

fn validate_text(name: &str, value: &str, min: usize, max: usize) -> Result<()> {
    let length = value.chars().count();
    if length < min || length > max || (min > 0 && value.trim().is_empty()) {
        return Err(Error::InvalidInput(format!(
            "{name} must contain {min} to {max} characters"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::InvalidInput(format!(
            "{name} must be a single line without control characters"
        )));
    }
    Ok(())
}

fn read_task(row: &Row<'_>) -> rusqlite::Result<Task> {
    let raw: String = row.get(4)?;
    let status = match raw.as_str() {
        "todo" => Status::Todo,
        "in_progress" => Status::InProgress,
        "blocked" => Status::Blocked,
        "done" => Status::Done,
        _ => {
            return Err(rusqlite::Error::InvalidColumnType(
                4,
                "status".into(),
                rusqlite::types::Type::Text,
            ))
        }
    };
    Ok(Task {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_key: row.get(2)?,
        title: row.get(3)?,
        status,
        progress: row.get(5)?,
        branch: row.get(6)?,
        updated_at: row.get(7)?,
        archived: row.get(8)?,
    })
}
