use kanban_core::{Database, SyncHealth, SyncOutcome, SyncTool, SyncTransport};
use rusqlite::Connection;
use serde_json::json;
use std::{
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};

fn fixture() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("agentkanban.sqlite3")).unwrap();
    (root, db)
}

#[test]
fn empty_health_is_read_only_and_pause_is_always_current() {
    let (_root, db) = fixture();
    assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());
    assert_eq!(db.get_setting("sync_health").unwrap(), None);
    db.set_tracking_paused(true).unwrap();
    let paused = db.get_sync_health().unwrap();
    assert!(paused.paused);
    assert!(paused.last_call.is_none());
    db.set_tracking_paused(false).unwrap();
    assert!(!db.get_sync_health().unwrap().paused);
    assert_eq!(db.revision().unwrap(), 0);
    let schema: i64 = Connection::open(db.path())
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(schema, 5);
}

#[test]
fn successful_reads_writes_errors_and_pauses_have_separate_history() {
    let (_root, db) = fixture();
    db.record_sync_event(
        SyncTransport::Mcp,
        SyncTool::TaskList,
        SyncOutcome::Ok,
        None,
    )
    .unwrap();
    let first = db.get_sync_health().unwrap();
    assert_eq!(first.last_call, first.last_success);
    assert!(first.last_write.is_none());
    db.record_sync_event(
        SyncTransport::Cli,
        SyncTool::TaskUpsert,
        SyncOutcome::Ok,
        None,
    )
    .unwrap();
    let wrote = db.get_sync_health().unwrap();
    assert_eq!(wrote.last_call, wrote.last_write);
    assert_eq!(wrote.last_success, wrote.last_write);
    db.record_sync_event(
        SyncTransport::Mcp,
        SyncTool::TaskList,
        SyncOutcome::Error,
        Some("Invalid tool arguments"),
    )
    .unwrap();
    let failed = db.get_sync_health().unwrap();
    assert_eq!(failed.last_success, wrote.last_success);
    assert_eq!(failed.last_write, wrote.last_write);
    assert_eq!(failed.last_call.unwrap().outcome, SyncOutcome::Error);
    db.set_tracking_paused(true).unwrap();
    db.record_sync_event(
        SyncTransport::Cli,
        SyncTool::TaskArchive,
        SyncOutcome::Paused,
        Some("ignored for a non-error result"),
    )
    .unwrap();
    let paused = db.get_sync_health().unwrap();
    assert!(paused.paused);
    assert_eq!(paused.last_success, wrote.last_success);
    assert_eq!(paused.last_write, wrote.last_write);
    assert_eq!(
        paused.last_call.as_ref().unwrap().outcome,
        SyncOutcome::Paused
    );
    assert!(paused.last_call.unwrap().error.is_none());
    assert_eq!(db.revision().unwrap(), 0);
}

#[test]
fn observations_never_change_tasks_reports_or_agent_timestamps() {
    let (root, db) = fixture();
    let receipt = db
        .upsert_from_json(json!({
            "project_path": root.path(), "task_key":"auto:unchanged", "title":"Existing task",
            "status":"todo", "progress":"Real work", "goal":"Private goal"
        }))
        .unwrap();
    let before = serde_json::to_value(db.board().unwrap()).unwrap();
    let reports = serde_json::to_value(db.reports(receipt.id).unwrap()).unwrap();
    for tool in [
        SyncTool::TaskList,
        SyncTool::TaskUpsert,
        SyncTool::TaskArchive,
    ] {
        db.record_sync_event(SyncTransport::Mcp, tool, SyncOutcome::Ok, None)
            .unwrap();
    }
    assert_eq!(serde_json::to_value(db.board().unwrap()).unwrap(), before);
    assert_eq!(
        serde_json::to_value(db.reports(receipt.id).unwrap()).unwrap(),
        reports
    );
}

#[test]
fn metadata_is_one_bounded_setting_with_only_three_event_slots() {
    let (_root, db) = fixture();
    let oversized_error = "测试\n".repeat(4000);
    for _ in 0..30 {
        db.record_sync_event(
            SyncTransport::Cli,
            SyncTool::TaskArchive,
            SyncOutcome::Error,
            Some(&oversized_error),
        )
        .unwrap();
    }
    let health = db.get_sync_health().unwrap();
    let error = health.last_call.unwrap().error.unwrap();
    assert_eq!(error.chars().count(), 160);
    assert!(!error.chars().any(char::is_control));
    let raw = db.get_setting("sync_health").unwrap().unwrap();
    assert!(raw.len() <= 4096);
    let stored: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(stored.as_object().unwrap().len(), 3);
    assert_eq!(stored["last_call"].as_object().unwrap().len(), 5);
    assert_eq!(stored["last_call"]["transport"], "cli");
    assert_eq!(stored["last_call"]["tool"], "task_archive");
    let count: i64 = Connection::open(db.path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn concurrent_observations_preserve_the_successful_write() {
    let (_root, db) = fixture();
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = [SyncOutcome::Ok, SyncOutcome::Error]
        .into_iter()
        .map(|outcome| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.record_sync_event(SyncTransport::Mcp, SyncTool::TaskUpsert, outcome, None)
                    .unwrap();
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let health = db.get_sync_health().unwrap();
    assert_eq!(health.last_success, health.last_write);
    assert_eq!(health.last_write.unwrap().outcome, SyncOutcome::Ok);
}

#[test]
fn metadata_lock_contention_has_a_short_wait_and_no_task_side_effects() {
    let (_root, db) = fixture();
    let conn = Connection::open(db.path()).unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    let result = db.record_sync_event(
        SyncTransport::Mcp,
        SyncTool::TaskList,
        SyncOutcome::Ok,
        None,
    );
    assert!(result.is_err());
    // The configured busy wait is 75 ms; leave scheduling tolerance without
    // accepting the ordinary task connection's 5-second wait.
    assert!(started.elapsed() < Duration::from_millis(500));
    conn.execute_batch("ROLLBACK").unwrap();
    assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());
    assert_eq!(db.revision().unwrap(), 0);
}

#[test]
fn diagnostics_do_not_prevent_first_launch_tutorial_but_other_settings_still_do() {
    let (_root, db) = fixture();
    db.record_sync_event(
        SyncTransport::Mcp,
        SyncTool::TaskList,
        SyncOutcome::Ok,
        None,
    )
    .unwrap();
    assert!(db.initialize_tutorial().unwrap());
    assert_eq!(db.board().unwrap().projects[0].tasks.len(), 2);
    let (_other_root, existing) = fixture();
    existing
        .record_sync_event(
            SyncTransport::Mcp,
            SyncTool::TaskList,
            SyncOutcome::Ok,
            None,
        )
        .unwrap();
    existing.set_setting("unrelated_preference", "1").unwrap();
    assert!(!existing.initialize_tutorial().unwrap());
    assert!(existing.board().unwrap().projects.is_empty());
}

#[test]
fn invalid_optional_metadata_is_recoverable_without_changing_tasks() {
    let (_root, db) = fixture();
    for invalid in ["not JSON".to_string(), "x".repeat(5000)] {
        db.set_setting("sync_health", &invalid).unwrap();
        assert_eq!(db.get_sync_health().unwrap(), SyncHealth::default());
        db.record_sync_event(
            SyncTransport::Cli,
            SyncTool::TaskList,
            SyncOutcome::Ok,
            None,
        )
        .unwrap();
        assert_eq!(
            db.get_sync_health().unwrap().last_call.unwrap().transport,
            SyncTransport::Cli
        );
    }
    assert_eq!(db.revision().unwrap(), 0);
}
