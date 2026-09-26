//! Bounded observations of completed tool calls, never a connection heartbeat.

use crate::{Database, Error, Result, TRACKING_PAUSED};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(super) const SETTING_KEY: &str = "sync_health";
const MAX_ERROR_CHARS: usize = 160;
const MAX_STORED_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncTransport {
    Mcp,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncTool {
    TaskList,
    TaskUpsert,
    TaskArchive,
}

impl SyncTool {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "task_list" => Some(Self::TaskList),
            "task_upsert" => Some(Self::TaskUpsert),
            "task_archive" => Some(Self::TaskArchive),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncOutcome {
    Ok,
    Paused,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncEvent {
    pub at: String,
    pub transport: SyncTransport,
    pub tool: SyncTool,
    pub outcome: SyncOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncHealth {
    pub paused: bool,
    pub last_call: Option<SyncEvent>,
    pub last_success: Option<SyncEvent>,
    pub last_write: Option<SyncEvent>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredHealth {
    last_call: Option<SyncEvent>,
    last_success: Option<SyncEvent>,
    last_write: Option<SyncEvent>,
}

fn read_stored(conn: &Connection) -> Result<StoredHealth> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key=?1 AND length(CAST(value AS BLOB))<=?2",
            params![SETTING_KEY, MAX_STORED_BYTES as i64],
            |row| row.get(0),
        )
        .optional()?;
    // Optional diagnostic metadata cannot make the task database unusable.
    Ok(raw
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default())
}

impl Database {
    /// Read observations and the current pause setting; absence or age says
    /// nothing about whether an MCP client is presently connected.
    pub fn get_sync_health(&self) -> Result<SyncHealth> {
        let conn = self.connect()?;
        let stored = read_stored(&conn)?;
        let paused: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key=?1",
                [TRACKING_PAUSED],
                |row| row.get(0),
            )
            .optional()?;
        Ok(SyncHealth {
            paused: paused.as_deref() == Some("1"),
            last_call: stored.last_call,
            last_success: stored.last_success,
            last_write: stored.last_write,
        })
    }

    /// Best-effort metadata only. Callers must preserve the tool's original
    /// result if this fails, and pass a static error category, never a payload.
    pub fn record_sync_event(
        &self,
        transport: SyncTransport,
        tool: SyncTool,
        outcome: SyncOutcome,
        error: Option<&str>,
    ) -> Result<()> {
        // Do not use connect(): its ordinary task operations can wait 5 seconds.
        // READ_WRITE also prevents diagnostics from recreating a removed DB.
        let mut conn = Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.busy_timeout(Duration::from_millis(75))?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut stored = read_stored(&tx)?;
        let event = SyncEvent {
            at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
            transport,
            tool,
            outcome,
            error: (outcome == SyncOutcome::Error).then(|| {
                error
                    .unwrap_or("Tool call failed")
                    .chars()
                    .filter(|character| !character.is_control())
                    .take(MAX_ERROR_CHARS)
                    .collect()
            }),
        };
        if outcome == SyncOutcome::Ok {
            stored.last_success = Some(event.clone());
            if matches!(tool, SyncTool::TaskUpsert | SyncTool::TaskArchive) {
                stored.last_write = Some(event.clone());
            }
        }
        stored.last_call = Some(event);
        let value = serde_json::to_string(&stored)
            .map_err(|_| Error::InvalidInput("Cannot encode sync health".into()))?;
        if value.len() > MAX_STORED_BYTES {
            return Err(Error::InvalidInput(
                "Sync health exceeds its size limit".into(),
            ));
        }
        tx.execute(
            "INSERT INTO settings(key,value) VALUES (?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![SETTING_KEY, value],
        )?;
        tx.commit()?;
        Ok(())
    }
}
