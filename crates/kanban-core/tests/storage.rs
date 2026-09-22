use kanban_core::{resolve_project, ArchiveTask, Database, Error, ListTasks, Status, UpsertTask};
use rusqlite::Connection;
use std::{
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Fixture {
    root: TempDir,
    db: Database,
    project: String,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("中文项目");
        std::fs::create_dir(&directory).unwrap();
        let project = directory.to_str().unwrap().to_string();
        let db = Database::open(root.path().join("data/agentkanban.sqlite3")).unwrap();
        Self { root, db, project }
    }

    fn input(&self, key: &str, status: Status) -> UpsertTask {
        UpsertTask {
            project_path: self.project.clone(),
            task_key: key.into(),
            title: "中文任务标题".into(),
            status,
            progress: "已创建，等待开始".into(),
            branch: Some("feature/中文".into()),
            agent: None,
            next_action: None,
            needs_input: None,
            deliverables: None,
            expected_updated_at: None,
        }
    }

    fn archive(&self, key: &str, archived: bool) -> ArchiveTask {
        ArchiveTask {
            project_path: self.project.clone(),
            task_key: key.into(),
            archived,
            expected_updated_at: None,
        }
    }
}

#[test]
fn idempotent_updates_complete_and_reopen_keep_one_record_across_restart() {
    let fixture = Fixture::new();
    let input = fixture.input("feature-1", Status::Todo);
    let created = fixture.db.upsert(input.clone()).unwrap();
    let repeated = fixture.db.upsert(input).unwrap();
    assert_eq!(created, repeated);
    assert_eq!(fixture.db.revision().unwrap(), 1);
    let progress = fixture
        .db
        .upsert(fixture.input("feature-1", Status::InProgress))
        .unwrap();
    assert_eq!(created.id, progress.id);
    let done = fixture
        .db
        .upsert(fixture.input("feature-1", Status::Done))
        .unwrap();
    assert_eq!(created.id, done.id);
    assert!(fixture
        .db
        .list(ListTasks::default())
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        fixture
            .db
            .list(ListTasks {
                status: Some(Status::Done),
                ..Default::default()
            })
            .unwrap()
            .items
            .len(),
        1
    );
    let reopened = fixture
        .db
        .upsert(fixture.input("feature-1", Status::InProgress))
        .unwrap();
    assert_eq!(reopened.id, created.id);
    let second_process_view = Database::open(fixture.db.path()).unwrap();
    let snapshot = second_process_view.board().unwrap();
    assert_eq!(snapshot.revision, 4);
    assert_eq!(snapshot.projects.len(), 1);
    assert_eq!(snapshot.projects[0].tasks.len(), 1);
    assert_eq!(snapshot.projects[0].tasks[0].status, Status::InProgress);
    assert_eq!(
        snapshot.projects[0].tasks[0].branch.as_deref(),
        Some("feature/中文")
    );
    assert!(snapshot.projects[0].tasks[0].updated_at.ends_with('Z'));
}

#[test]
fn archive_is_reversible_preserves_data_and_is_idempotent() {
    let fixture = Fixture::new();
    let created = fixture
        .db
        .upsert(fixture.input("keep", Status::Blocked))
        .unwrap();
    let archived = fixture.db.archive(fixture.archive("keep", true)).unwrap();
    assert_eq!(created.id, archived.id);
    assert_eq!(fixture.db.revision().unwrap(), 2);
    assert_eq!(
        fixture.db.archive(fixture.archive("keep", true)).unwrap(),
        archived
    );
    assert_eq!(fixture.db.revision().unwrap(), 2);
    assert!(fixture.db.board().unwrap().projects.is_empty());
    assert!(fixture
        .db
        .list(ListTasks::default())
        .unwrap()
        .items
        .is_empty());
    let record = fixture
        .db
        .list(ListTasks {
            include_archived: true,
            ..Default::default()
        })
        .unwrap()
        .items
        .pop()
        .unwrap();
    assert!(record.task.archived);
    assert_eq!(record.task.title, "中文任务标题");
    assert!(matches!(
        fixture.db.upsert(fixture.input("keep", Status::Done)),
        Err(Error::TaskArchived)
    ));
    fixture.db.archive(fixture.archive("keep", false)).unwrap();
    let record = fixture
        .db
        .board()
        .unwrap()
        .projects
        .pop()
        .unwrap()
        .tasks
        .pop()
        .unwrap();
    assert_eq!(record.id, created.id);
    assert_eq!(record.status, Status::Blocked);
    assert!(!record.archived);
    assert_eq!(fixture.db.revision().unwrap(), 3);
    assert!(matches!(
        fixture.db.archive(fixture.archive("missing", true)),
        Err(Error::TaskNotFound)
    ));
}

#[test]
fn query_defaults_filters_and_pagination_do_not_drop_or_repeat_tasks() {
    let fixture = Fixture::new();
    for index in 0..23 {
        fixture
            .db
            .upsert(fixture.input(&format!("todo-{index:02}"), Status::Todo))
            .unwrap();
    }
    fixture
        .db
        .upsert(fixture.input("done", Status::Done))
        .unwrap();
    fixture
        .db
        .upsert(fixture.input("blocked", Status::Blocked))
        .unwrap();
    fixture
        .db
        .upsert(fixture.input("active", Status::InProgress))
        .unwrap();
    fixture
        .db
        .upsert(fixture.input("archived", Status::InProgress))
        .unwrap();
    fixture
        .db
        .archive(fixture.archive("archived", true))
        .unwrap();
    let first = fixture.db.list(ListTasks::default()).unwrap();
    assert_eq!(first.items.len(), 20);
    assert_eq!(first.next_offset, Some(20));
    assert_eq!(first.items[0].task.task_key, "active");
    assert_eq!(first.items[1].task.task_key, "blocked");
    let second = fixture
        .db
        .list(ListTasks {
            offset: first.next_offset.unwrap(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second.items.len(), 5);
    assert_eq!(second.next_offset, None);
    assert!(second.items.iter().all(|item| first
        .items
        .iter()
        .all(|first| first.task.id != item.task.id)));
    let all = fixture
        .db
        .list(ListTasks {
            include_done: true,
            include_archived: true,
            limit: 100,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.items.len(), 27);
    assert_eq!(fixture.db.board().unwrap().projects[0].tasks.len(), 26);
    let empty = fixture.root.path().join("another-project");
    std::fs::create_dir(&empty).unwrap();
    assert!(fixture
        .db
        .list(ListTasks {
            project_path: Some(empty.to_str().unwrap().into()),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn settings_persist_without_triggering_board_revision() {
    let fixture = Fixture::new();
    assert_eq!(fixture.db.get_setting("ui").unwrap(), None);
    fixture.db.set_setting("ui", r#"{"theme":"dark"}"#).unwrap();
    fixture
        .db
        .set_setting("geometry", r#"{"x":120,"y":80}"#)
        .unwrap();
    assert_eq!(fixture.db.revision().unwrap(), 0);
    let reopened = Database::open(fixture.db.path()).unwrap();
    assert_eq!(
        reopened.get_setting("ui").unwrap().as_deref(),
        Some(r#"{"theme":"dark"}"#)
    );
    assert_eq!(
        reopened.get_setting("geometry").unwrap().as_deref(),
        Some(r#"{"x":120,"y":80}"#)
    );
}

#[test]
fn latest_task_update_includes_archived_tasks_and_ignores_settings() {
    let fixture = Fixture::new();
    assert_eq!(fixture.db.last_task_update().unwrap(), None);
    let created = fixture
        .db
        .upsert(fixture.input("latest", Status::Todo))
        .unwrap();
    assert_eq!(
        fixture.db.last_task_update().unwrap(),
        Some(created.updated_at)
    );
    let archived = fixture.db.archive(fixture.archive("latest", true)).unwrap();
    assert!(fixture.db.board().unwrap().projects.is_empty());
    assert_eq!(
        fixture.db.last_task_update().unwrap(),
        Some(archived.updated_at.clone())
    );
    fixture.db.set_setting("ui", r#"{"theme":"dark"}"#).unwrap();
    let reopened = Database::open(fixture.db.path()).unwrap();
    assert_eq!(
        reopened.last_task_update().unwrap(),
        Some(archived.updated_at)
    );
}

#[test]
fn invalid_input_cannot_create_partial_projects_or_tasks() {
    let fixture = Fixture::new();
    let mut input = fixture.input("bad", Status::Todo);
    input.title = " ".into();
    assert!(matches!(
        fixture.db.upsert(input.clone()),
        Err(Error::InvalidInput(_))
    ));
    input.title = "长".repeat(201);
    assert!(fixture.db.upsert(input.clone()).is_err());
    input.title = "okay".into();
    input.progress = "line 1\nline 2".into();
    assert!(fixture.db.upsert(input.clone()).is_err());
    input.progress = "okay".into();
    input.project_path = "relative-path".into();
    assert!(fixture.db.upsert(input.clone()).is_err());
    input.project_path = fixture.db.path().to_str().unwrap().into();
    assert!(fixture.db.upsert(input).is_err());
    assert!(fixture
        .db
        .list(ListTasks {
            limit: 0,
            ..Default::default()
        })
        .is_err());
    assert!(fixture
        .db
        .list(ListTasks {
            limit: 101,
            ..Default::default()
        })
        .is_err());
    assert!(serde_json::from_str::<ListTasks>(r#"{"unknown":true}"#).is_err());
    assert!(serde_json::from_str::<ListTasks>(r#"{"limit":-1}"#).is_err());
    assert!(serde_json::from_str::<ListTasks>(r#"{"status":null}"#).is_err());
    assert!(serde_json::from_str::<ListTasks>(r#"{"project_path":null}"#).is_err());
    assert_eq!(fixture.db.revision().unwrap(), 0);
    assert!(fixture.db.board().unwrap().projects.is_empty());
}

#[test]
fn unicode_lengths_are_characters_and_plain_directories_are_distinct() {
    let fixture = Fixture::new();
    let mut input = fixture.input("unicode", Status::Todo);
    input.title = "长".repeat(200);
    input.progress = "进".repeat(600);
    fixture.db.upsert(input.clone()).unwrap();
    let another = fixture.root.path().join("second");
    std::fs::create_dir(&another).unwrap();
    input.project_path = another.to_str().unwrap().into();
    fixture.db.upsert(input).unwrap();
    assert_eq!(fixture.db.board().unwrap().projects.len(), 2);
    let resolved = resolve_project(&fixture.project).unwrap();
    assert_eq!(resolved.name, "中文项目");
    assert!(resolved.identity.starts_with("dir:"));
}

#[test]
fn worktrees_and_repository_subdirectories_share_task_identity() {
    let fixture = Fixture::new();
    let repo = fixture.root.path().join("repository");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-b", "main"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Kanban Test",
            "-c",
            "user.email=kanban-test@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "--allow-empty",
            "-m",
            "Temporary test fixture",
        ],
    );
    let worktree = fixture.root.path().join("parallel-checkout");
    git(
        &repo,
        &["worktree", "add", "--detach", worktree.to_str().unwrap()],
    );
    let nested = repo.join("src");
    std::fs::create_dir(&nested).unwrap();
    let main_identity = resolve_project(repo.to_str().unwrap()).unwrap();
    let worktree_identity = resolve_project(worktree.to_str().unwrap()).unwrap();
    let nested_identity = resolve_project(nested.to_str().unwrap()).unwrap();
    assert_eq!(main_identity, worktree_identity);
    assert_eq!(main_identity, nested_identity);
    assert_eq!(main_identity.name, "repository");
    let mut input = fixture.input("same-feature", Status::Todo);
    input.project_path = repo.to_str().unwrap().into();
    let created = fixture.db.upsert(input.clone()).unwrap();
    input.project_path = worktree.to_str().unwrap().into();
    input.status = Status::InProgress;
    let updated = fixture.db.upsert(input).unwrap();
    assert_eq!(created.id, updated.id);
    assert_eq!(fixture.db.board().unwrap().projects.len(), 1);
    assert_eq!(fixture.db.board().unwrap().projects[0].tasks.len(), 1);
}

fn git(directory: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(args);
    for variable in ["GIT_DIR", "GIT_WORK_TREE", "GIT_COMMON_DIR"] {
        command.env_remove(variable);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .expect("git is required for the worktree identity test");
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn wal_readers_continue_and_contending_writer_waits_then_succeeds() {
    let fixture = Fixture::new();
    fixture
        .db
        .upsert(fixture.input("before", Status::Todo))
        .unwrap();
    let connection = Connection::open(fixture.db.path()).unwrap();
    let journal: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    // Another reader can still see the last committed state under WAL.
    assert_eq!(fixture.db.board().unwrap().projects[0].tasks.len(), 1);
    let clone = fixture.db.clone();
    let input = fixture.input("after", Status::InProgress);
    let started = Instant::now();
    let writer = thread::spawn(move || clone.upsert(input));
    thread::sleep(Duration::from_millis(250));
    assert!(!writer.is_finished());
    connection.execute_batch("COMMIT").unwrap();
    writer.join().unwrap().unwrap();
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert_eq!(fixture.db.board().unwrap().projects[0].tasks.len(), 2);
}

#[test]
fn write_lock_timeout_is_an_error_without_partial_writes() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.db.path()).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let started = Instant::now();
    let error = fixture
        .db
        .upsert(fixture.input("must-not-exist", Status::Todo))
        .unwrap_err();
    assert!(matches!(error, Error::Database(_)));
    assert!(error.to_string().contains("locked"));
    assert!(started.elapsed() >= Duration::from_secs(4));
    assert!(started.elapsed() < Duration::from_secs(12));
    connection.execute_batch("ROLLBACK").unwrap();
    assert_eq!(fixture.db.revision().unwrap(), 0);
    assert!(fixture.db.board().unwrap().projects.is_empty());
}

#[test]
fn failed_transaction_rolls_back_a_task_and_its_revision() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.db.path()).unwrap();
    connection.execute_batch("CREATE TRIGGER simulate_failure BEFORE UPDATE ON metadata BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END;").unwrap();
    let error = fixture
        .db
        .upsert(fixture.input("rollback", Status::Todo))
        .unwrap_err();
    assert!(error.to_string().contains("simulated disk failure"));
    assert_eq!(fixture.db.revision().unwrap(), 0);
    assert!(fixture.db.board().unwrap().projects.is_empty());
    let projects: i64 = connection
        .query_row("SELECT count(*) FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(projects, 0);
}

#[test]
fn future_schema_is_rejected_without_erasing_it() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.db.path()).unwrap();
    connection.execute_batch("PRAGMA user_version=99").unwrap();
    assert!(matches!(
        Database::open(fixture.db.path()),
        Err(Error::NewerSchema(99))
    ));
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 99);
}
