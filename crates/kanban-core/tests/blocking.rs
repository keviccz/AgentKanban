use kanban_core::{Database, ListTasks, Status};
use serde_json::json;

#[test]
fn blocked_projects_leave_the_board_and_come_back_when_unblocked() {
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("屏蔽项目");
    let open = root.path().join("正常项目");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::create_dir(&open).unwrap();
    let db = Database::open(root.path().join("data/test.sqlite3")).unwrap();
    for (path, key) in [(&blocked, "blocked:task"), (&open, "open:task")] {
        db.upsert_from_json(json!({
            "project_path": path, "task_key": key, "title": "任务",
            "status": Status::InProgress, "progress": "进行中"
        }))
        .unwrap();
    }
    let project_id = db
        .list(ListTasks {
            task_key: Some("blocked:task".into()),
            ..Default::default()
        })
        .unwrap()
        .items[0]
        .task
        .project_id;
    let blocked_path = blocked.to_str().unwrap();
    assert!(!db.is_path_blocked(blocked_path).unwrap());

    let revision = db.revision().unwrap();
    db.set_project_blocked(project_id, true).unwrap();
    assert_eq!(db.revision().unwrap(), revision + 1);
    // Blocking twice is a no-op and does not bump the revision.
    db.set_project_blocked(project_id, true).unwrap();
    assert_eq!(db.revision().unwrap(), revision + 1);

    let names: Vec<String> = db.board().unwrap().projects.into_iter().map(|p| p.name).collect();
    assert_eq!(names, ["正常项目"]);
    assert!(db.is_path_blocked(blocked_path).unwrap());
    assert!(!db.is_path_blocked(open.to_str().unwrap()).unwrap());
    // An unresolvable path is not "blocked"; normal validation reports it.
    assert!(!db.is_path_blocked("relative/path").unwrap());
    let listed = db.blocked_projects().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, project_id);
    assert_eq!(listed[0].name, "屏蔽项目");

    db.set_project_blocked(project_id, false).unwrap();
    assert_eq!(db.board().unwrap().projects.len(), 2);
    assert!(db.blocked_projects().unwrap().is_empty());
    assert!(db.set_project_blocked(0, true).is_err());
    assert!(db.set_project_blocked(9_999, true).is_err());
}
