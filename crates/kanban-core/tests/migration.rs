use kanban_core::{
    resolve_project, Database, Error, ListTasks, ReviewStatus, ReviewTask, Status, UpsertTask,
};
use rusqlite::{params, Connection};

fn legacy_database() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy.sqlite3");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE projects(id INTEGER PRIMARY KEY,identity TEXT NOT NULL UNIQUE,name TEXT NOT NULL,path TEXT NOT NULL);
         CREATE TABLE tasks(id INTEGER PRIMARY KEY,project_id INTEGER NOT NULL REFERENCES projects(id),task_key TEXT NOT NULL,title TEXT NOT NULL,status TEXT NOT NULL CHECK(status IN ('todo','in_progress','blocked','done')),progress TEXT NOT NULL,branch TEXT,updated_at TEXT NOT NULL,archived INTEGER NOT NULL DEFAULT 0 CHECK(archived IN (0,1)),UNIQUE(project_id,task_key));
         CREATE INDEX tasks_project_state ON tasks(project_id,archived,status);
         CREATE TABLE metadata(key TEXT PRIMARY KEY,value INTEGER NOT NULL);
         INSERT INTO metadata VALUES('revision',17);
         CREATE TABLE settings(key TEXT PRIMARY KEY,value TEXT NOT NULL);
         INSERT INTO settings VALUES('ui','{\"theme\":\"dark\"}');
         PRAGMA user_version=1;"
    ).unwrap();
    let project = resolve_project(root.path().to_str().unwrap()).unwrap();
    conn.execute(
        "INSERT INTO projects(id,identity,name,path) VALUES (1,?1,'旧项目',?2)",
        params![project.identity, project.path],
    )
    .unwrap();
    conn.execute_batch(
        "INSERT INTO tasks VALUES(7,1,'legacy-done','已交付旧任务','done','保留已有结果','main','2026-09-20T10:00:00.000Z',0);
         INSERT INTO tasks VALUES(8,1,'legacy-active','进行中的旧任务','in_progress','正在运行',NULL,'2026-09-21T10:00:00.000Z',0);
         INSERT INTO tasks VALUES(9,1,'legacy-archive','已归档旧任务','blocked','保留阻塞信息',NULL,'2026-09-22T10:00:00.000Z',1);"
    ).unwrap();
    (root, path)
}

#[test]
fn v1_migration_preserves_tasks_settings_and_legacy_done_review_boundary() {
    let (root, path) = legacy_database();
    let db = Database::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        5
    );
    assert_eq!(db.revision().unwrap(), 17);
    assert_eq!(
        db.get_setting("ui").unwrap().as_deref(),
        Some(r#"{"theme":"dark"}"#)
    );
    let all = db
        .list(ListTasks {
            include_done: true,
            include_archived: true,
            limit: 100,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.items.len(), 3);
    for item in &all.items {
        let task = &item.task;
        assert_eq!(task.agent_updated_at.as_ref(), Some(&task.updated_at));
        assert!(
            task.request.is_empty()
                && task.user_note.is_empty()
                && task.next_action.is_empty()
                && task.needs_input.is_empty()
        );
        assert!(task.deliverables.is_empty());
        assert_eq!(task.agent, None);
        assert_eq!(task.review_status, ReviewStatus::None);
    }
    let legacy = all
        .items
        .iter()
        .find(|item| item.task.id == 7)
        .unwrap()
        .task
        .clone();
    assert_eq!(legacy.task_key, "legacy-done");
    assert_eq!(legacy.title, "已交付旧任务");
    assert_eq!(legacy.status, Status::Done);
    assert_eq!(legacy.branch.as_deref(), Some("main"));
    assert!(matches!(
        db.review(ReviewTask {
            id: legacy.id,
            expected_updated_at: legacy.updated_at.clone(),
            accepted: true,
            note: String::new()
        }),
        Err(Error::NotReviewable)
    ));
    let input = UpsertTask {
        project_path: root.path().to_str().unwrap().into(),
        task_key: legacy.task_key.clone(),
        title: legacy.title.clone(),
        status: legacy.status,
        progress: legacy.progress.clone(),
        branch: legacy.branch.clone(),
        agent: None,
        next_action: None,
        needs_input: None,
        deliverables: None,
        steps: None,
        step_updates: None,
        goal: None,
        acceptance: None,
        expected_updated_at: Some(legacy.updated_at.clone()),
    };
    let receipt = db.upsert(input).unwrap();
    assert_eq!(receipt.updated_at, legacy.updated_at);
    assert_eq!(db.revision().unwrap(), 17);
    let reopened = Database::open(&path).unwrap();
    let task = reopened
        .list(ListTasks {
            task_key: Some("legacy-done".into()),
            include_done: true,
            ..Default::default()
        })
        .unwrap()
        .items
        .pop()
        .unwrap()
        .task;
    assert_eq!(task, legacy);
}

#[test]
fn failed_migration_rolls_back_schema_and_original_data_before_retry() {
    let (_root, path) = legacy_database();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER migration_failure BEFORE UPDATE ON tasks BEGIN SELECT RAISE(ABORT,'migration failure fixture'); END;").unwrap();
    let error = Database::open(&path).unwrap_err();
    assert!(error.to_string().contains("migration failure fixture"));
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let columns: Vec<String> = conn
        .prepare("PRAGMA table_info(tasks)")
        .unwrap()
        .query_map([], |row| row.get(1))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(!columns.contains(&"request".into()));
    assert!(!columns.contains(&"agent_updated_at".into()));
    assert_eq!(
        conn.query_row("SELECT count(*) FROM tasks", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row(
            "SELECT value FROM metadata WHERE key='revision'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        17
    );
    conn.execute_batch("DROP TRIGGER migration_failure")
        .unwrap();
    let recovered = Database::open(path).unwrap();
    assert_eq!(recovered.board().unwrap().projects[0].tasks.len(), 2);
    assert_eq!(recovered.revision().unwrap(), 17);
}

#[test]
fn migrated_legacy_done_tasks_are_never_auto_archived_without_human_acceptance() {
    let (_root, path) = legacy_database();
    let db = Database::open(path).unwrap();
    assert_eq!(db.archive_finished(std::time::Duration::ZERO).unwrap(), 0);
    let done = db
        .list(ListTasks {
            task_key: Some("legacy-done".into()),
            include_done: true,
            ..Default::default()
        })
        .unwrap()
        .items
        .remove(0)
        .task;
    assert!(!done.archived);
    assert_eq!(done.review_status, ReviewStatus::None);
}
