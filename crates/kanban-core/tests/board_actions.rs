use kanban_core::{Database, ListTasks, ReviewStatus, ReviewTask, Status, Task};
use serde_json::json;

fn setup() -> (tempfile::TempDir, Database, std::path::PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("动作项目");
    std::fs::create_dir(&project).unwrap();
    let db = Database::open(root.path().join("data/test.sqlite3")).unwrap();
    (root, db, project)
}

fn task(db: &Database, key: &str) -> Task {
    db.list(ListTasks { task_key: Some(key.into()), include_done: true, include_archived: true, ..Default::default() }).unwrap().items.remove(0).task
}

fn seed(db: &Database, project: &std::path::Path, key: &str, status: Status) {
    db.upsert_from_json(json!({
        "project_path": project, "task_key": key, "title": key, "status": status, "progress": "p"
    }))
    .unwrap();
}

#[test]
fn a_one_click_acceptance_can_be_undone_once() {
    let (_root, db, project) = setup();
    seed(&db, &project, "done", Status::Done);
    let pending = task(&db, "done");
    let accepted = db
        .review(ReviewTask { id: pending.id, expected_updated_at: pending.updated_at, accepted: true, note: String::new() })
        .unwrap();
    let undone = db.undo_accept(accepted.id, &accepted.updated_at).unwrap();
    assert_eq!(task(&db, "done").review_status, ReviewStatus::Pending);
    // A second undo, or one with a stale version, is refused.
    assert!(db.undo_accept(undone.id, &undone.updated_at).is_err());
    assert!(db.undo_accept(accepted.id, &accepted.updated_at).is_err());
}

#[test]
fn a_project_archive_can_be_undone_without_touching_older_archives() {
    let (_root, db, project) = setup();
    seed(&db, &project, "a", Status::InProgress);
    seed(&db, &project, "b", Status::Todo);
    let project_id = task(&db, "a").project_id;
    let ids = db.archive_project(project_id).unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(db.restore_many(&ids).unwrap(), 2);
    assert!(!task(&db, "a").archived && !task(&db, "b").archived);
    // Restoring again is a no-op and does not bump the revision.
    let revision = db.revision().unwrap();
    assert_eq!(db.restore_many(&ids).unwrap(), 0);
    assert_eq!(db.revision().unwrap(), revision);
    assert!(db.restore_many(&[0]).is_err());
}

#[test]
fn summary_lists_finished_work_including_archived_and_rename_keeps_identity() {
    let (_root, db, project) = setup();
    seed(&db, &project, "finished", Status::Done);
    seed(&db, &project, "working", Status::InProgress);
    let project_id = task(&db, "finished").project_id;
    db.archive_project(project_id).unwrap();
    let rows = db.finished_since("2000-01-01T00:00:00.000Z").unwrap();
    assert_eq!(rows.iter().map(|row| row.task.task_key.as_str()).collect::<Vec<_>>(), ["finished"]);
    assert!(db.finished_since("2999-01-01T00:00:00.000Z").unwrap().is_empty());

    db.rename_project(project_id, "  新名字  ").unwrap();
    db.restore_many(&[task(&db, "working").id]).unwrap();
    assert_eq!(db.board().unwrap().projects[0].name, "新名字");
    // A later Agent report for the same folder keeps the new name.
    seed(&db, &project, "later", Status::Todo);
    assert_eq!(db.board().unwrap().projects[0].name, "新名字");
    assert!(db.rename_project(project_id, " ").is_err());
    assert!(db.rename_project(project_id, "两\n行").is_err());
    assert!(db.rename_project(9_999, "x").is_err());
}
