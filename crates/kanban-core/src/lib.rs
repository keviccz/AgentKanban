//! Shared local storage for the desktop app and independent MCP processes.

mod archive_center;
mod blocking;
mod onboarding;
mod project;
mod sync_health;

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub use archive_center::ArchiveQuery;
pub use blocking::BlockedProject;
pub use project::{resolve_project, ProjectIdentity};
pub use sync_health::{SyncEvent, SyncHealth, SyncOutcome, SyncTool, SyncTransport};

pub type Result<T> = std::result::Result<T, Error>;

const TRACKING_PAUSED: &str = "tracking_paused";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Task not found; check project_path and task_key")]
    TaskNotFound,
    #[error("Task is archived; restore it with task_archive(archived=false) before updating")]
    TaskArchived,
    #[error("Conflict: existing changes require a current expected_updated_at; re-read the task before retrying")]
    Conflict,
    #[error("Only an unarchived done task with pending review can be reviewed")]
    NotReviewable,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStatus {
    #[default]
    None,
    Pending,
    Accepted,
    ChangesRequested,
}

impl ReviewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::ChangesRequested => "changes_requested",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Todo,
    InProgress,
    Blocked,
    Done,
}

/// One plan step. The Agent replaces the whole list; the GUI only displays it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub title: String,
    pub status: StepStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// A versioned change to one existing plan step. Omitted fields are preserved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepUpdate {
    pub index: u32,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub status: Option<StepStatus>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deliverable {
    pub label: String,
    pub uri: String,
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
    #[serde(default)]
    pub request: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub next_action: String,
    #[serde(default)]
    pub needs_input: String,
    #[serde(default)]
    pub deliverables: Vec<Deliverable>,
    #[serde(default)]
    pub review_status: ReviewStatus,
    #[serde(default)]
    pub user_note: String,
    #[serde(default)]
    pub agent_updated_at: Option<String>,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// Set when the Agent reopened a done task before the user reviewed it.
    #[serde(default)]
    pub review_withdrawn_at: Option<String>,
    /// What the task delivers, in the Agent's words.
    #[serde(default)]
    pub goal: String,
    /// How the user can check the result; shown next to the review buttons.
    #[serde(default)]
    pub acceptance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectBoard {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub tasks: Vec<Task>,
    /// Archived tasks of this project; the GUI loads them on demand.
    #[serde(default)]
    pub archived_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub revision: i64,
    pub projects: Vec<ProjectBoard>,
}

/// Legacy fields are replacements; omitted/null branch clears it.
/// New Agent fields are patches: omission preserves the value. Null agent is
/// omission too; an empty agent string clears the attribution.
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub next_action: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub needs_input: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub deliverables: Option<Vec<Deliverable>>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub steps: Option<Vec<Step>>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub step_updates: Option<Vec<StepUpdate>>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub goal: Option<String>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub acceptance: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureTask {
    pub project_path: String,
    pub task_key: String,
    pub title: String,
    pub request: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTask {
    pub id: i64,
    pub expected_updated_at: String,
    pub accepted: bool,
    pub note: String,
}

/// GUI archive: a human action, so it never counts as Agent activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchiveById {
    pub id: i64,
    pub expected_updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedbackTask {
    pub id: i64,
    pub expected_updated_at: String,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTasks {
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub query: Option<String>,
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
    pub task_key: Option<String>,
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
            query: None,
            project_path: None,
            task_key: None,
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
    #[serde(
        default,
        deserialize_with = "optional_non_null",
        skip_serializing_if = "Option::is_none"
    )]
    pub expected_updated_at: Option<String>,
}

const fn default_archived() -> bool {
    true
}

/// One Agent report as it arrived over MCP, kept for the user to inspect.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskReport {
    pub reported_at: String,
    /// The task_upsert fields the Agent sent, without routing and version fields.
    pub payload: serde_json::Value,
}

/// Reports kept per task; older ones are dropped.
pub const REPORTS_KEPT: i64 = 30;

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
        // Switching a new database to WAL can report SQLITE_BUSY without consulting
        // the busy handler when several clients start MCP at once. Retry briefly.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match db.prepare_schema() {
                Err(Error::Database(rusqlite::Error::SqliteFailure(failure, _)))
                    if matches!(
                        failure.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                result => return result.map(|()| db),
            }
        }
    }

    fn prepare_schema(&self) -> Result<()> {
        let mut conn = self.connect()?;
        let mode: String = conn.query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::InvalidInput(
                "database filesystem does not support SQLite WAL".into(),
            ));
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 5 {
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
        if version <= 1 {
            tx.execute_batch(
                "ALTER TABLE tasks ADD COLUMN request TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN agent TEXT;
                 ALTER TABLE tasks ADD COLUMN next_action TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN needs_input TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN deliverables TEXT NOT NULL DEFAULT '[]';
                 ALTER TABLE tasks ADD COLUMN review_status TEXT NOT NULL DEFAULT 'none'
                   CHECK(review_status IN ('none','pending','accepted','changes_requested'));
                 ALTER TABLE tasks ADD COLUMN user_note TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN agent_updated_at TEXT;
                 UPDATE tasks SET agent_updated_at=updated_at;
                 PRAGMA user_version=2;",
            )?;
        }
        if version <= 2 {
            tx.execute_batch(
                "ALTER TABLE tasks ADD COLUMN steps TEXT NOT NULL DEFAULT '[]';
                 ALTER TABLE tasks ADD COLUMN review_withdrawn_at TEXT;
                 PRAGMA user_version=3;",
            )?;
        }
        if version <= 3 {
            tx.execute_batch(
                "ALTER TABLE tasks ADD COLUMN goal TEXT NOT NULL DEFAULT '';
                 ALTER TABLE tasks ADD COLUMN acceptance TEXT NOT NULL DEFAULT '[]';
                 PRAGMA user_version=4;",
            )?;
        }
        if version <= 4 {
            tx.execute_batch(
                "CREATE TABLE task_reports (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL REFERENCES tasks(id),
                    reported_at TEXT NOT NULL,
                    payload TEXT NOT NULL
                 );
                 CREATE INDEX task_reports_task ON task_reports(task_id,id);
                 PRAGMA user_version=5;",
            )?;
        }
        tx.commit()?;
        Ok(())
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
    /// Newest first.
    pub fn reports(&self, task_id: i64) -> Result<Vec<TaskReport>> {
        validate_task_id(task_id)?;
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT reported_at,payload FROM task_reports WHERE task_id=?1 ORDER BY id DESC",
        )?;
        let rows = statement.query_map([task_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (reported_at, payload) = row?;
            Ok(TaskReport {
                reported_at,
                payload: serde_json::from_str(&payload)
                    .map_err(|err| Error::InvalidInput(err.to_string()))?,
            })
        })
        .collect()
    }

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
                "SELECT id,name,path,
                   (SELECT COUNT(*) FROM tasks a WHERE a.project_id=p.id AND a.archived=1)
                 FROM projects p
                 WHERE EXISTS (SELECT 1 FROM tasks t WHERE t.project_id=p.id AND t.archived=0)
                   AND p.identity NOT IN (SELECT value FROM json_each(
                     COALESCE((SELECT value FROM settings WHERE key='blocked_projects'),'[]')))
                 ORDER BY name COLLATE NOCASE,id",
            )?;
            let rows = statement.query_map([], |row| {
                Ok(ProjectBoard {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    path: row.get(2)?,
                    tasks: Vec::new(),
                    archived_count: row.get(3)?,
                })
            })?;
            for row in rows {
                projects.push(row?);
            }
        }
        {
            let mut statement = tx.prepare(
                "SELECT * FROM tasks WHERE project_id=?1 AND archived=0
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
        let report =
            serde_json::to_value(&input).map_err(|err| Error::InvalidInput(err.to_string()))?;
        self.upsert_inner(input, report_payload(report))
    }

    /// Preserve the MCP fields exactly as supplied, including omission and null.
    /// Routing/version fields are excluded from the stored report.
    pub fn upsert_from_json(&self, value: serde_json::Value) -> Result<TaskReceipt> {
        let input: UpsertTask = serde_json::from_value(value.clone())
            .map_err(|err| Error::InvalidInput(err.to_string()))?;
        self.upsert_inner(input, report_payload(value))
    }

    fn upsert_inner(&self, input: UpsertTask, report: String) -> Result<TaskReceipt> {
        validate_text("task_key", &input.task_key, 1, 160)?;
        validate_text("title", &input.title, 1, 200)?;
        validate_text("progress", &input.progress, 0, 600)?;
        if let Some(branch) = &input.branch {
            validate_text("branch", branch, 1, 200)?;
        }
        if let Some(agent) = &input.agent {
            if !agent.is_empty() {
                validate_text("agent", agent, 1, 100)?;
            }
        }
        if let Some(action) = &input.next_action {
            validate_text("next_action", action, 0, 600)?;
        }
        if let Some(needed) = &input.needs_input {
            validate_text("needs_input", needed, 0, 600)?;
        }
        if let Some(deliverables) = &input.deliverables {
            validate_deliverables(deliverables)?;
        }
        if let Some(steps) = &input.steps {
            validate_steps(steps)?;
        }
        if input.steps.is_some() && input.step_updates.is_some() {
            return Err(Error::InvalidInput(
                "steps and step_updates are mutually exclusive".into(),
            ));
        }
        if let Some(updates) = &input.step_updates {
            validate_step_updates(updates)?;
        }
        if let Some(goal) = &input.goal {
            validate_text("goal", goal, 0, 300)?;
        }
        if let Some(acceptance) = &input.acceptance {
            validate_acceptance(acceptance)?;
        }
        if let Some(expected) = &input.expected_updated_at {
            validate_text("expected_updated_at", expected, 1, 64)?;
        }
        let project = self.resolve_task_project(&input.project_path)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO projects(identity,name,path) VALUES (?1,?2,?3) ON CONFLICT(identity) DO NOTHING",
            params![project.identity, project.name, project.path])?;
        let project_id: i64 = tx.query_row(
            "SELECT id FROM projects WHERE identity=?1",
            [&project.identity],
            |row| row.get(0),
        )?;
        let existing = tx
            .query_row(
                "SELECT * FROM tasks WHERE project_id=?1 AND task_key=?2",
                params![project_id, input.task_key],
                read_task,
            )
            .optional()?;
        check_expected(input.expected_updated_at.as_deref(), existing.as_ref())?;
        let agent = match &input.agent {
            Some(agent) if agent.is_empty() => None,
            Some(agent) => Some(agent.clone()),
            None => existing.as_ref().and_then(|task| task.agent.clone()),
        };
        let next_action = input.next_action.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.next_action.clone())
                .unwrap_or_default()
        });
        let needs_input = input.needs_input.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.needs_input.clone())
                .unwrap_or_default()
        });
        let deliverables = input.deliverables.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.deliverables.clone())
                .unwrap_or_default()
        });
        let mut steps = input.steps.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.steps.clone())
                .unwrap_or_default()
        });
        if let Some(updates) = &input.step_updates {
            for update in updates {
                let step = steps.get_mut(update.index as usize).ok_or_else(|| {
                    Error::InvalidInput("step_updates index is outside the existing plan".into())
                })?;
                if let Some(status) = update.status {
                    step.status = status;
                }
                if let Some(note) = &update.note {
                    step.note.clone_from(note);
                }
            }
        }
        let goal = input.goal.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.goal.clone())
                .unwrap_or_default()
        });
        let acceptance = input.acceptance.unwrap_or_else(|| {
            existing
                .as_ref()
                .map(|task| task.acceptance.clone())
                .unwrap_or_default()
        });
        if let Some(ref task) = existing {
            if task.archived {
                return Err(Error::TaskArchived);
            }
            if task.title == input.title
                && task.status == input.status
                && task.progress == input.progress
                && task.branch == input.branch
                && task.agent == agent
                && task.next_action == next_action
                && task.needs_input == needs_input
                && task.deliverables == deliverables
                && task.steps == steps
                && task.goal == goal
                && task.acceptance == acceptance
            {
                return Ok(TaskReceipt::from(task));
            }
            // Missing tokens are allowed only for creation and a truly identical retry.
            // The decision happens under the same write transaction as the update.
            if input.expected_updated_at.is_none() {
                return Err(Error::Conflict);
            }
        }
        let review_status = if input.status == Status::Done {
            match &existing {
                None => ReviewStatus::Pending,
                Some(task)
                    if task.status != Status::Done
                        || task.review_status == ReviewStatus::Accepted =>
                {
                    ReviewStatus::Pending
                }
                Some(task) => task.review_status,
            }
        } else if existing
            .as_ref()
            .is_some_and(|task| task.review_status == ReviewStatus::ChangesRequested)
        {
            // Keep a rejection visible while the Agent works through the requested changes.
            ReviewStatus::ChangesRequested
        } else {
            ReviewStatus::None
        };
        let updated_at = changed_at(existing.as_ref().map(|task| task.updated_at.as_str()));
        // Reopening a done task before review cancels that review; keep a visible trace.
        let review_withdrawn_at = match &existing {
            _ if input.status == Status::Done => None,
            Some(task)
                if task.status == Status::Done && task.review_status == ReviewStatus::Pending =>
            {
                Some(updated_at.clone())
            }
            Some(task) => task.review_withdrawn_at.clone(),
            None => None,
        };
        let deliverables = serde_json::to_string(&deliverables)
            .map_err(|err| Error::InvalidInput(err.to_string()))?;
        let steps =
            serde_json::to_string(&steps).map_err(|err| Error::InvalidInput(err.to_string()))?;
        let acceptance = serde_json::to_string(&acceptance)
            .map_err(|err| Error::InvalidInput(err.to_string()))?;
        let id = if let Some(task) = existing {
            tx.execute(
                "UPDATE tasks SET title=?1,status=?2,progress=?3,branch=?4,updated_at=?5,
                    agent=?6,next_action=?7,needs_input=?8,deliverables=?9,review_status=?10,agent_updated_at=?5,
                    steps=?11,review_withdrawn_at=?12,goal=?13,acceptance=?14 WHERE id=?15",
                params![input.title, input.status.as_str(), input.progress, input.branch, updated_at,
                    agent,next_action,needs_input,deliverables,review_status.as_str(),steps,review_withdrawn_at,
                    goal,acceptance,task.id]
            )?;
            task.id
        } else {
            tx.execute(
                "INSERT INTO tasks(project_id,task_key,title,status,progress,branch,updated_at,agent,next_action,needs_input,deliverables,review_status,agent_updated_at,steps,goal,acceptance)
                    VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?7,?13,?14,?15)",
                params![project_id, input.task_key, input.title, input.status.as_str(), input.progress, input.branch, updated_at,
                    agent,next_action,needs_input,deliverables,review_status.as_str(),steps,goal,acceptance]
            )?;
            tx.last_insert_rowid()
        };
        tx.execute(
            "INSERT INTO task_reports(task_id,reported_at,payload) VALUES (?1,?2,?3)",
            params![id, updated_at, report],
        )?;
        tx.execute(
            "DELETE FROM task_reports WHERE task_id=?1 AND id NOT IN
                (SELECT id FROM task_reports WHERE task_id=?1 ORDER BY id DESC LIMIT ?2)",
            params![id, REPORTS_KEPT],
        )?;
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
        if let Some(key) = &input.task_key {
            validate_text("task_key", key, 1, 160)?;
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
        let identity = input
            .project_path
            .as_deref()
            .map(|path| self.resolve_task_project(path))
            .transpose()?
            .map(|project| project.identity);
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT t.*,p.name AS project_name,p.path AS project_path
             FROM tasks t JOIN projects p ON t.project_id=p.id
             WHERE (?1 IS NULL OR p.identity=?1)
               AND (?2 IS NULL OR t.status=?2)
               AND (?3 OR t.status!='done')
               AND (?4 OR t.archived=0)
                AND (?5 IS NULL OR t.task_key=?5)
                AND (?8 IS NULL OR t.task_key LIKE ?8 ESCAPE '\\'
                     OR t.title LIKE ?8 ESCAPE '\\' OR t.goal LIKE ?8 ESCAPE '\\'
                     OR t.request LIKE ?8 ESCAPE '\\' OR t.progress LIKE ?8 ESCAPE '\\'
                     OR t.user_note LIKE ?8 ESCAPE '\\')
             ORDER BY CASE t.status WHEN 'in_progress' THEN 0 WHEN 'blocked' THEN 1 WHEN 'todo' THEN 2 ELSE 3 END,
                      t.updated_at DESC,t.id DESC
             LIMIT ?6 OFFSET ?7"
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
                    input.task_key,
                    input.limit + 1,
                    input.offset,
                    query
                ],
                |row| {
                    Ok(ListedTask {
                        task: read_task(row)?,
                        project_name: row.get("project_name")?,
                        project_path: row.get("project_path")?,
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
        if let Some(expected) = &input.expected_updated_at {
            validate_text("expected_updated_at", expected, 1, 64)?;
        }
        let project = self.resolve_task_project(&input.project_path)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx.query_row(
            "SELECT t.* FROM tasks t JOIN projects p ON t.project_id=p.id WHERE p.identity=?1 AND t.task_key=?2",
            params![project.identity, input.task_key], read_task
        ).optional()?.ok_or(Error::TaskNotFound)?;
        check_expected(input.expected_updated_at.as_deref(), Some(&task))?;
        if task.archived == input.archived {
            return Ok(TaskReceipt::from(&task));
        }
        if input.expected_updated_at.is_none() {
            return Err(Error::Conflict);
        }
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET archived=?1,updated_at=?2,agent_updated_at=?2 WHERE id=?3",
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

    /// Hide a task from the board by user request. The archive center restores
    /// the same row through restore_by_id without reporting Agent activity.
    pub fn archive_by_id(&self, input: ArchiveById) -> Result<TaskReceipt> {
        validate_task_id(input.id)?;
        validate_text("expected_updated_at", &input.expected_updated_at, 1, 64)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx
            .query_row("SELECT * FROM tasks WHERE id=?1", [input.id], read_task)
            .optional()?
            .ok_or(Error::TaskNotFound)?;
        check_expected(Some(&input.expected_updated_at), Some(&task))?;
        if task.archived {
            return Ok(TaskReceipt::from(&task));
        }
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET archived=1,updated_at=?1 WHERE id=?2",
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

    /// GUI capture is create-only. Retrying a key never overwrites work already taken on by an Agent.
    pub fn capture(&self, input: CaptureTask) -> Result<TaskReceipt> {
        validate_text("task_key", &input.task_key, 1, 160)?;
        validate_text("title", &input.title, 1, 200)?;
        validate_multiline("request", &input.request, 0, 2000)?;
        let project = self.resolve_task_project(&input.project_path)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO projects(identity,name,path) VALUES (?1,?2,?3) ON CONFLICT(identity) DO NOTHING",
            params![project.identity,project.name,project.path])?;
        let project_id: i64 = tx.query_row(
            "SELECT id FROM projects WHERE identity=?1",
            [&project.identity],
            |row| row.get(0),
        )?;
        if let Some(task) = tx
            .query_row(
                "SELECT * FROM tasks WHERE project_id=?1 AND task_key=?2",
                params![project_id, input.task_key],
                read_task,
            )
            .optional()?
        {
            return Ok(TaskReceipt::from(&task));
        }
        let updated_at = changed_at(None);
        tx.execute(
            "INSERT INTO tasks(project_id,task_key,title,status,progress,updated_at,request)
            VALUES (?1,?2,?3,'todo','等待 Agent 接手',?4,?5)",
            params![
                project_id,
                input.task_key,
                input.title,
                updated_at,
                input.request
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt {
            id,
            status: Status::Todo,
            updated_at,
        })
    }

    /// GUI review records a human decision without reporting new Agent activity.
    /// Rejection returns the original task to the default unfinished MCP query.
    pub fn review(&self, input: ReviewTask) -> Result<TaskReceipt> {
        validate_task_id(input.id)?;
        validate_text("expected_updated_at", &input.expected_updated_at, 1, 64)?;
        validate_multiline(
            "note",
            &input.note,
            if input.accepted { 0 } else { 1 },
            2000,
        )?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx
            .query_row("SELECT * FROM tasks WHERE id=?1", [input.id], read_task)
            .optional()?
            .ok_or(Error::TaskNotFound)?;
        check_expected(Some(&input.expected_updated_at), Some(&task))?;
        if task.archived
            || task.status != Status::Done
            || task.review_status != ReviewStatus::Pending
        {
            return Err(Error::NotReviewable);
        }
        let (status, review_status) = if input.accepted {
            (Status::Done, ReviewStatus::Accepted)
        } else {
            (Status::Todo, ReviewStatus::ChangesRequested)
        };
        let note = if input.accepted && input.note.is_empty() {
            task.user_note
        } else {
            input.note
        };
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET status=?1,review_status=?2,user_note=?3,updated_at=?4 WHERE id=?5",
            params![
                status.as_str(),
                review_status.as_str(),
                note,
                updated_at,
                task.id
            ],
        )?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(TaskReceipt {
            id: task.id,
            status,
            updated_at,
        })
    }

    /// Replace the latest human note; an empty string clears it. All Agent fields stay intact.
    pub fn feedback(&self, input: FeedbackTask) -> Result<TaskReceipt> {
        validate_task_id(input.id)?;
        validate_text("expected_updated_at", &input.expected_updated_at, 1, 64)?;
        validate_multiline("note", &input.note, 0, 2000)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = tx
            .query_row("SELECT * FROM tasks WHERE id=?1", [input.id], read_task)
            .optional()?
            .ok_or(Error::TaskNotFound)?;
        check_expected(Some(&input.expected_updated_at), Some(&task))?;
        if task.user_note == input.note {
            return Ok(TaskReceipt::from(&task));
        }
        let updated_at = changed_at(Some(&task.updated_at));
        tx.execute(
            "UPDATE tasks SET user_note=?1,updated_at=?2 WHERE id=?3",
            params![input.note, updated_at, task.id],
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

    /// Set from the desktop app and read by every MCP call, so it applies to
    /// already-running Agent sessions without touching client configuration.
    pub fn tracking_paused(&self) -> Result<bool> {
        Ok(self.get_setting(TRACKING_PAUSED)?.as_deref() == Some("1"))
    }

    pub fn set_tracking_paused(&self, paused: bool) -> Result<()> {
        self.set_setting(TRACKING_PAUSED, if paused { "1" } else { "0" })
    }

    /// Archives finished work left untouched for `older_than`: accepted tasks and
    /// done tasks the user never reviewed. Legacy done tasks stay visible. This is
    /// not Agent activity.
    pub fn archive_finished(&self, older_than: Duration) -> Result<usize> {
        let age = chrono::Duration::from_std(older_than)
            .map_err(|_| Error::InvalidInput("archive age is too large".into()))?;
        let cutoff = (Utc::now() - age).to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let archived = tx.execute(
            "UPDATE tasks SET archived=1,updated_at=?1
             WHERE archived=0 AND status='done' AND review_status IN ('accepted','pending') AND updated_at<?2",
            params![changed_at(None), cutoff],
        )?;
        if archived > 0 {
            tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        }
        tx.commit()?;
        Ok(archived)
    }

    /// Keeps at most `keep` finished tasks (accepted or unreviewed) on the board per
    /// project; older ones move to the archive. Legacy done tasks are left alone.
    pub fn archive_overflow(&self, keep: u32) -> Result<usize> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let archived = tx.execute(
            "UPDATE tasks SET archived=1,updated_at=?1 WHERE id IN (
               SELECT id FROM (
                 SELECT id,ROW_NUMBER() OVER (PARTITION BY project_id ORDER BY updated_at DESC,id DESC) AS rank
                 FROM tasks
                 WHERE archived=0 AND status='done' AND review_status IN ('accepted','pending')
               ) WHERE rank>?2)",
            params![changed_at(None), keep],
        )?;
        if archived > 0 {
            tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        }
        tx.commit()?;
        Ok(archived)
    }

    /// A consistent single-file copy, WAL content included, safe while
    /// MCP processes keep writing.
    pub fn backup_to(&self, target: &Path) -> Result<()> {
        if target.exists() {
            return Err(Error::InvalidInput("backup file already exists".into()));
        }
        self.connect()?
            .execute("VACUUM INTO ?1", [target.to_string_lossy()])?;
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

fn validate_multiline(name: &str, value: &str, min: usize, max: usize) -> Result<()> {
    let length = value.chars().count();
    if length < min || length > max || (min > 0 && value.trim().is_empty()) {
        return Err(Error::InvalidInput(format!(
            "{name} must contain {min} to {max} characters"
        )));
    }
    if value
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Err(Error::InvalidInput(format!(
            "{name} contains unsupported control characters"
        )));
    }
    Ok(())
}

fn validate_task_id(id: i64) -> Result<()> {
    if id <= 0 {
        return Err(Error::InvalidInput(
            "id must be a positive task identifier".into(),
        ));
    }
    Ok(())
}

fn validate_deliverables(deliverables: &[Deliverable]) -> Result<()> {
    if deliverables.len() > 5 {
        return Err(Error::InvalidInput(
            "deliverables allows at most 5 items".into(),
        ));
    }
    for deliverable in deliverables {
        validate_text("deliverable label", &deliverable.label, 1, 100)?;
        validate_text("deliverable uri", &deliverable.uri, 1, 1000)?;
        let lower = deliverable.uri.to_ascii_lowercase();
        if let Some(rest) = lower
            .strip_prefix("https://")
            .or_else(|| lower.strip_prefix("http://"))
        {
            if rest.trim().is_empty() {
                return Err(Error::InvalidInput(
                    "deliverable HTTP URL must include a host".into(),
                ));
            }
        } else if let Some(colon) = deliverable.uri.find(':') {
            let drive_path = colon == 1 && deliverable.uri.as_bytes()[0].is_ascii_alphabetic();
            if !drive_path {
                return Err(Error::InvalidInput(
                    "deliverable uri must be a file path or an http(s) URL".into(),
                ));
            }
        }
    }
    Ok(())
}

/// What the Agent sent, minus the fields that only route or version the write.
fn report_payload(mut value: serde_json::Value) -> String {
    if let Some(fields) = value.as_object_mut() {
        for routing in ["project_path", "task_key", "expected_updated_at"] {
            fields.remove(routing);
        }
    }
    value.to_string()
}

fn validate_step_updates(updates: &[StepUpdate]) -> Result<()> {
    if updates.len() > 12 {
        return Err(Error::InvalidInput(
            "step_updates allows at most 12 items".into(),
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for update in updates {
        if !seen.insert(update.index) {
            return Err(Error::InvalidInput(
                "step_updates contains a duplicate index".into(),
            ));
        }
        if update.status.is_none() && update.note.is_none() {
            return Err(Error::InvalidInput(
                "each step update needs status or note".into(),
            ));
        }
        if let Some(note) = &update.note {
            validate_text("step update note", note, 0, 200)?;
        }
    }
    Ok(())
}

fn validate_acceptance(items: &[String]) -> Result<()> {
    if items.len() > 8 {
        return Err(Error::InvalidInput(
            "acceptance allows at most 8 items".into(),
        ));
    }
    for item in items {
        validate_text("acceptance item", item, 1, 160)?;
    }
    Ok(())
}

fn validate_steps(steps: &[Step]) -> Result<()> {
    if steps.len() > 12 {
        return Err(Error::InvalidInput("steps allows at most 12 items".into()));
    }
    for step in steps {
        validate_text("step title", &step.title, 1, 120)?;
        validate_text("step note", &step.note, 0, 200)?;
    }
    Ok(())
}

fn check_expected(expected: Option<&str>, task: Option<&Task>) -> Result<()> {
    if let Some(expected) = expected {
        if task.is_none_or(|task| task.updated_at != expected) {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}

// A millisecond timestamp is also the optimistic concurrency token. Make it
// strictly increase per task even for rapid writes or a backwards wall clock.
fn changed_at(previous: Option<&str>) -> String {
    let now = Utc::now();
    let previous = previous.and_then(|text| DateTime::parse_from_rfc3339(text).ok());
    let millis = previous.map_or(now.timestamp_millis(), |timestamp| {
        now.timestamp_millis()
            .max(timestamp.timestamp_millis().saturating_add(1))
    });
    DateTime::<Utc>::from_timestamp_millis(millis)
        .unwrap_or(now)
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn read_task(row: &Row<'_>) -> rusqlite::Result<Task> {
    let raw: String = row.get("status")?;
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
    let review_raw: String = row.get("review_status")?;
    let review_status = match review_raw.as_str() {
        "none" => ReviewStatus::None,
        "pending" => ReviewStatus::Pending,
        "accepted" => ReviewStatus::Accepted,
        "changes_requested" => ReviewStatus::ChangesRequested,
        _ => {
            return Err(rusqlite::Error::InvalidColumnType(
                14,
                "review_status".into(),
                rusqlite::types::Type::Text,
            ))
        }
    };
    let raw_deliverables: String = row.get("deliverables")?;
    let deliverables = serde_json::from_str(&raw_deliverables).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let raw_steps: String = row.get("steps")?;
    let steps = serde_json::from_str(&raw_steps).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(17, rusqlite::types::Type::Text, Box::new(err))
    })?;
    let raw_acceptance: String = row.get("acceptance")?;
    let acceptance = serde_json::from_str(&raw_acceptance).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(20, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(Task {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        task_key: row.get("task_key")?,
        title: row.get("title")?,
        status,
        progress: row.get("progress")?,
        branch: row.get("branch")?,
        updated_at: row.get("updated_at")?,
        archived: row.get("archived")?,
        request: row.get("request")?,
        agent: row.get("agent")?,
        next_action: row.get("next_action")?,
        needs_input: row.get("needs_input")?,
        deliverables,
        review_status,
        user_note: row.get("user_note")?,
        agent_updated_at: row.get("agent_updated_at")?,
        steps,
        review_withdrawn_at: row.get("review_withdrawn_at")?,
        goal: row.get("goal")?,
        acceptance,
    })
}
