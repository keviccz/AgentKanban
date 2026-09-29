use kanban_core::{ArchiveTask, CaptureTask, Database, Error, ListTasks, Status};
use serde_json::json;
use std::time::Duration;

fn setup() -> (tempfile::TempDir, Database, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("个人项目");
    std::fs::create_dir(&project).unwrap();
    let db = Database::open(root.path().join("data/test.sqlite3")).unwrap();
    (root, db, project)
}

fn capture(db: &Database, path: &str, key: &str, personal: bool) -> kanban_core::TaskReceipt {
    db.capture(CaptureTask {
        project_path: path.into(),
        task_key: key.into(),
        title: format!("title {key}"),
        request: String::new(),
        personal,
        later: false,
    })
    .unwrap()
}

fn list_all(db: &Database, project: Option<&std::path::Path>, key: Option<&str>) -> usize {
    db.list(ListTasks {
        project_path: project.map(|path| path.to_string_lossy().to_string()),
        task_key: key.map(Into::into),
        include_done: true,
        include_archived: true,
        ..Default::default()
    })
    .unwrap()
    .items
    .len()
}

#[test]
fn a_personal_todo_without_a_project_joins_the_personal_group() {
    let (_root, db, _project) = setup();
    let receipt = capture(&db, "", "me:1", true);
    assert_eq!(receipt.status, Status::InProgress);
    let board = db.board().unwrap();
    assert_eq!(board.projects.len(), 1);
    let group = &board.projects[0];
    assert!(group.personal);
    assert_eq!(group.path, "");
    assert!(group.tasks[0].personal);
    assert_eq!(group.tasks[0].agent_updated_at, None);
    // An Agent task still needs a real directory.
    assert!(db
        .capture(CaptureTask {
            project_path: String::new(),
            task_key: "capture:1".into(),
            title: "t".into(),
            request: String::new(),
            personal: false,
            later: false,
        })
        .is_err());
    // The personal group cannot be blocked.
    assert!(db.set_project_blocked(group.id, true).is_err());
}

#[test]
fn agents_can_neither_see_nor_change_personal_todos() {
    let (_root, db, project) = setup();
    let path = project.to_string_lossy().to_string();
    capture(&db, &path, "me:secret", true);
    capture(&db, "", "me:loose", true);
    db.upsert_from_json(json!({
        "project_path": project, "task_key": "auto:work", "title": "work", "status": "in_progress", "progress": "p"
    }))
    .unwrap();
    // Listing, searching and exact keys only return the Agent task.
    assert_eq!(list_all(&db, Some(&project), None), 1);
    assert_eq!(list_all(&db, None, None), 1);
    assert_eq!(list_all(&db, Some(&project), Some("me:secret")), 0);
    let search = db
        .list(ListTasks {
            query: Some("title".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(search.items.is_empty());
    // Writing or archiving the same key is refused without touching the todo.
    let upsert = db.upsert_from_json(json!({
        "project_path": project, "task_key": "me:secret", "title": "x", "status": "done", "progress": "p"
    }));
    assert!(matches!(upsert, Err(Error::InvalidInput(_))));
    let archive = db.archive(ArchiveTask {
        project_path: path,
        task_key: "me:secret".into(),
        archived: true,
        expected_updated_at: Some("2026-01-01T00:00:00.000Z".into()),
    });
    assert!(matches!(archive, Err(Error::TaskNotFound)));
    let board = db.board().unwrap();
    let todo = board
        .projects
        .iter()
        .flat_map(|project| &project.tasks)
        .find(|task| task.task_key == "me:secret")
        .unwrap();
    assert_eq!(todo.status, Status::InProgress);
    assert!(!todo.archived);
    // Personal writes never look like Agent sync activity.
    let before = db.last_task_update().unwrap();
    capture(&db, "", "me:later", true);
    assert_eq!(db.last_task_update().unwrap(), before);
}

#[test]
fn the_user_finishes_and_reopens_a_personal_todo() {
    let (_root, db, project) = setup();
    let created = capture(&db, "", "me:1", true);
    let done = db
        .set_personal_status(created.id, &created.updated_at, Status::Done)
        .unwrap();
    assert_eq!(done.status, Status::Done);
    // Stale versions are refused; the same state is a no-op.
    assert!(matches!(
        db.set_personal_status(created.id, &created.updated_at, Status::InProgress),
        Err(Error::Conflict)
    ));
    let again = db
        .set_personal_status(done.id, &done.updated_at, Status::Done)
        .unwrap();
    assert_eq!(again.updated_at, done.updated_at);
    let reopened = db
        .set_personal_status(done.id, &done.updated_at, Status::InProgress)
        .unwrap();
    assert_eq!(reopened.status, Status::InProgress);
    // Agent tasks keep their own status flow.
    let agent = db
        .upsert_from_json(json!({
            "project_path": project, "task_key": "auto:a", "title": "a", "status": "in_progress", "progress": "p"
        }))
        .unwrap();
    assert!(db
        .set_personal_status(agent.id, &agent.updated_at, Status::Done)
        .is_err());
    // Finished personal todos are not in the Agent work summary.
    db.set_personal_status(reopened.id, &reopened.updated_at, Status::Done)
        .unwrap();
    assert!(db
        .finished_since("2000-01-01T00:00:00.000Z")
        .unwrap()
        .is_empty());
}

#[test]
fn finished_personal_todos_follow_auto_archive() {
    let (_root, db, _project) = setup();
    let created = capture(&db, "", "me:1", true);
    db.set_personal_status(created.id, &created.updated_at, Status::Done)
        .unwrap();
    assert_eq!(db.archive_finished(Duration::ZERO).unwrap(), 1);
    let board = db.board().unwrap();
    assert!(board.projects.is_empty());
}

#[test]
fn a_personal_todo_can_wait_until_the_user_starts_it() {
    let (_root, db, project) = setup();
    let later = db
        .capture(CaptureTask {
            project_path: String::new(),
            task_key: "me:later".into(),
            title: "later".into(),
            request: String::new(),
            personal: true,
            later: true,
        })
        .unwrap();
    assert_eq!(later.status, Status::Todo);
    let started = db
        .set_personal_status(later.id, &later.updated_at, Status::InProgress)
        .unwrap();
    assert_eq!(started.status, Status::InProgress);
    let paused = db
        .set_personal_status(started.id, &started.updated_at, Status::Todo)
        .unwrap();
    assert_eq!(paused.status, Status::Todo);
    // Blocked belongs to Agent work; a personal todo never takes it.
    assert!(matches!(
        db.set_personal_status(paused.id, &paused.updated_at, Status::Blocked),
        Err(Error::InvalidInput(_))
    ));
    // `later` only applies to personal todos.
    let path = project.to_string_lossy().to_string();
    let agent = db
        .capture(CaptureTask {
            project_path: path,
            task_key: "capture:x".into(),
            title: "x".into(),
            request: String::new(),
            personal: false,
            later: true,
        })
        .unwrap();
    assert_eq!(agent.status, Status::Todo);
}
