use kanban_core::{
    ArchiveById, ArchiveTask, CaptureTask, Database, FeedbackTask, ListTasks, ReviewStatus,
    ReviewTask, Status, StepStatus, Task,
};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Barrier},
};

const FOLLOW: &str = "tutorial:follow-progress";
const REVIEW: &str = "tutorial:review-delivery";
const MARKER: &str = "tutorial_initialized_v1";

fn fixture() -> (tempfile::TempDir, Database) {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("data/agentkanban.sqlite3")).unwrap();
    (root, db)
}

fn task(db: &Database, key: &str) -> Task {
    db.list(ListTasks {
        task_key: Some(key.into()),
        include_done: true,
        include_archived: true,
        ..Default::default()
    })
    .unwrap()
    .items
    .pop()
    .unwrap()
    .task
}

fn count(db: &Database, table: &str) -> i64 {
    assert!(["projects", "tasks", "task_reports"].contains(&table));
    Connection::open(db.path())
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn opening_storage_does_not_seed_and_first_gui_initialization_adds_two_distinct_examples() {
    let (root, db) = fixture();
    assert!(db.board().unwrap().projects.is_empty());
    assert_eq!(db.get_setting(MARKER).unwrap(), None);
    let conn = Connection::open(db.path()).unwrap();
    let schema_before: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();

    assert!(db.initialize_tutorial().unwrap());
    assert!(!db.initialize_tutorial().unwrap());
    let board = db.board().unwrap();
    assert_eq!(board.projects.len(), 1);
    let project = &board.projects[0];
    assert_eq!(project.name, "新手教程");
    assert_eq!(project.tasks.len(), 2);
    assert_eq!(
        Path::new(&project.path).canonicalize().unwrap(),
        root.path().join("data/tutorial").canonicalize().unwrap()
    );
    let identity: String = conn
        .query_row("SELECT identity FROM projects", [], |row| row.get(0))
        .unwrap();
    assert_eq!(identity, "agentkanban:tutorial:v1");
    assert_eq!(count(&db, "projects"), 1);
    assert_eq!(count(&db, "tasks"), 2);
    assert_eq!(board.revision, 1);
    assert_eq!(db.get_setting(MARKER).unwrap().as_deref(), Some("seeded"));
    let schema_after: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(schema_after, schema_before);

    let follow = task(&db, FOLLOW);
    let review = task(&db, REVIEW);
    assert_ne!(follow.id, review.id);
    assert_eq!(
        (follow.status, follow.review_status),
        (Status::InProgress, ReviewStatus::None)
    );
    assert_eq!(
        (review.status, review.review_status),
        (Status::Done, ReviewStatus::Pending)
    );
    assert_eq!(
        follow
            .steps
            .iter()
            .map(|step| step.status)
            .collect::<Vec<_>>(),
        [StepStatus::Done, StepStatus::InProgress, StepStatus::Todo]
    );
    assert!(review
        .steps
        .iter()
        .all(|step| step.status == StepStatus::Done));
    assert_eq!(review.acceptance.len(), 2);
    for task in [follow, review] {
        assert!(task.title.starts_with("示例："));
        assert_eq!(task.agent.as_deref(), Some("教学示例"));
        assert!(task.progress.contains("教学示例") && task.request.contains("教学示例"));
        assert!(
            task.user_note.is_empty()
                && task.needs_input.is_empty()
                && task.deliverables.is_empty()
        );
        assert_eq!(task.agent_updated_at, None);
        assert!(db.reports(task.id).unwrap().is_empty());
    }
    assert_eq!(count(&db, "task_reports"), 0);
}

#[test]
fn existing_or_archived_real_work_is_never_given_tutorial_tasks() {
    for archived in [false, true] {
        let (root, db) = fixture();
        let created = db
            .capture(CaptureTask {
                project_path: root.path().to_str().unwrap().into(),
                task_key: "real:keep".into(),
                title: "保留真实任务".into(),
                request: "已有用户数据".into(),
            })
            .unwrap();
        if archived {
            db.archive_by_id(ArchiveById {
                id: created.id,
                expected_updated_at: created.updated_at,
            })
            .unwrap();
        }
        let before = task(&db, "real:keep");
        let revision = db.revision().unwrap();
        assert!(!db.initialize_tutorial().unwrap());
        assert_eq!(task(&db, "real:keep"), before);
        assert_eq!(count(&db, "projects"), 1);
        assert_eq!(count(&db, "tasks"), 1);
        assert_eq!(db.revision().unwrap(), revision);
        assert_eq!(db.get_setting(MARKER).unwrap().as_deref(), Some("skipped"));
        assert!(!root.path().join("data/tutorial").exists());
    }
}

#[test]
fn old_preferences_or_retained_history_skip_initialization_even_with_an_empty_visible_board() {
    for key in ["ui", "geometry", "tracking_paused"] {
        let (root, db) = fixture();
        db.set_setting(key, "{}").unwrap();
        assert!(!db.initialize_tutorial().unwrap());
        assert!(db.board().unwrap().projects.is_empty());
        assert_eq!(db.get_setting(key).unwrap().as_deref(), Some("{}"));
        assert!(!root.path().join("data/tutorial").exists());
        // Once checked as an existing user, deleting old preferences cannot
        // unexpectedly turn the next launch into an onboarding session.
        Connection::open(db.path())
            .unwrap()
            .execute("DELETE FROM settings WHERE key=?1", [key])
            .unwrap();
        assert!(!db.initialize_tutorial().unwrap());
        assert_eq!(count(&db, "tasks"), 0);
    }
    for keep_project in [false, true] {
        let (_root, db) = fixture();
        let conn = Connection::open(db.path()).unwrap();
        if keep_project {
            conn.execute(
                "INSERT INTO projects(identity,name,path) VALUES ('old','已有项目','old')",
                [],
            )
            .unwrap();
        } else {
            conn.execute("UPDATE metadata SET value=3 WHERE key='revision'", [])
                .unwrap();
        }
        assert!(!db.initialize_tutorial().unwrap());
        assert_eq!(count(&db, "tasks"), 0);
        assert_eq!(db.get_setting(MARKER).unwrap().as_deref(), Some("skipped"));
    }
}

#[test]
fn archiving_both_examples_and_reopening_does_not_resurrect_them() {
    let (_root, db) = fixture();
    assert!(db.initialize_tutorial().unwrap());
    for key in [FOLLOW, REVIEW] {
        let task = task(&db, key);
        db.archive_by_id(ArchiveById {
            id: task.id,
            expected_updated_at: task.updated_at,
        })
        .unwrap();
    }
    let reopened = Database::open(db.path()).unwrap();
    let revision = reopened.revision().unwrap();
    assert!(!reopened.initialize_tutorial().unwrap());
    assert!(reopened.board().unwrap().projects.is_empty());
    assert_eq!(count(&reopened, "tasks"), 2);
    assert!(task(&reopened, FOLLOW).archived && task(&reopened, REVIEW).archived);
    assert_eq!(reopened.revision().unwrap(), revision);
}

#[test]
fn examples_accept_real_feedback_and_human_acceptance_or_rejection() {
    for accepted in [true, false] {
        let (_root, db) = fixture();
        db.initialize_tutorial().unwrap();
        for key in [FOLLOW, REVIEW] {
            let before = task(&db, key);
            let note = "这是我自己填写的练习意见";
            db.feedback(FeedbackTask {
                id: before.id,
                expected_updated_at: before.updated_at.clone(),
                note: note.into(),
            })
            .unwrap();
            let after = task(&db, key);
            assert_eq!(after.user_note, note);
            assert_eq!(after.status, before.status);
            assert_eq!(after.request, before.request);
            assert_eq!(after.agent_updated_at, None);
        }
        let before = task(&db, REVIEW);
        db.review(ReviewTask {
            id: before.id,
            expected_updated_at: before.updated_at,
            accepted,
            note: if accepted {
                String::new()
            } else {
                "请补充演示说明".into()
            },
        })
        .unwrap();
        let after = task(&db, REVIEW);
        assert_eq!(after.id, before.id);
        assert_eq!(
            after.status,
            if accepted { Status::Done } else { Status::Todo }
        );
        assert_eq!(
            after.review_status,
            if accepted {
                ReviewStatus::Accepted
            } else {
                ReviewStatus::ChangesRequested
            }
        );
        assert_eq!(
            after.user_note,
            if accepted {
                "这是我自己填写的练习意见"
            } else {
                "请补充演示说明"
            }
        );
        assert_eq!(after.agent_updated_at, None);
        assert_eq!(count(&db, "tasks"), 2);
        assert_eq!(count(&db, "task_reports"), 0);
    }
}

#[test]
fn concurrent_initializers_commit_exactly_one_pair_of_examples() {
    let (_root, db) = fixture();
    let barrier = Arc::new(Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.initialize_tutorial().unwrap()
            })
        })
        .collect();
    let inserted = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .filter(|inserted| *inserted)
        .count();
    assert_eq!(inserted, 1);
    assert_eq!(count(&db, "projects"), 1);
    assert_eq!(count(&db, "tasks"), 2);
    assert_eq!(db.revision().unwrap(), 1);
}

#[test]
fn a_failed_second_insert_rolls_back_project_tasks_marker_and_revision() {
    let (_root, db) = fixture();
    let conn = Connection::open(db.path()).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_tutorial BEFORE INSERT ON tasks
        WHEN NEW.task_key='tutorial:review-delivery'
        BEGIN SELECT RAISE(ABORT,'injected tutorial failure'); END;",
    )
    .unwrap();
    assert!(db.initialize_tutorial().is_err());
    assert_eq!(count(&db, "projects"), 0);
    assert_eq!(count(&db, "tasks"), 0);
    assert_eq!(db.get_setting(MARKER).unwrap(), None);
    assert_eq!(db.revision().unwrap(), 0);
    conn.execute_batch("DROP TRIGGER reject_tutorial;").unwrap();
    assert!(db.initialize_tutorial().unwrap());
    assert_eq!(count(&db, "tasks"), 2);
}

#[test]
fn summary_paths_support_exact_queries_and_normal_archive_restore_without_reidentifying_real_projects(
) {
    let (root, db) = fixture();
    db.initialize_tutorial().unwrap();
    let summary = db
        .list(ListTasks {
            include_done: true,
            ..Default::default()
        })
        .unwrap();
    for listed in summary.items {
        let exact = db
            .list(ListTasks {
                project_path: Some(listed.project_path.clone()),
                task_key: Some(listed.task.task_key.clone()),
                include_done: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(exact.items.len(), 1);
        assert_eq!(exact.items[0].task.id, listed.task.id);
        let archived = db
            .archive(ArchiveTask {
                project_path: listed.project_path.clone(),
                task_key: listed.task.task_key.clone(),
                archived: true,
                expected_updated_at: Some(listed.task.updated_at),
            })
            .unwrap();
        assert!(task(&db, &listed.task.task_key).archived);
        let archived_query = db
            .list(ListTasks {
                project_path: Some(listed.project_path.clone()),
                task_key: Some(listed.task.task_key.clone()),
                include_done: true,
                include_archived: true,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(archived_query.items.len(), 1);
        assert!(archived_query.items[0].task.archived);
        db.archive(ArchiveTask {
            project_path: listed.project_path,
            task_key: listed.task.task_key.clone(),
            archived: false,
            expected_updated_at: Some(archived.updated_at),
        })
        .unwrap();
        assert!(!task(&db, &listed.task.task_key).archived);
    }
    let real_path = root.path().to_str().unwrap();
    let identity = kanban_core::resolve_project(real_path).unwrap();
    let captured = db
        .capture(CaptureTask {
            project_path: real_path.into(),
            task_key: "real:outside-tutorial".into(),
            title: "真实项目不受教程影响".into(),
            request: "".into(),
        })
        .unwrap();
    let real = db
        .list(ListTasks {
            project_path: Some(real_path.into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(real.items.len(), 1);
    assert_eq!(real.items[0].task.id, captured.id);
    assert_eq!(real.items[0].project_name, identity.name);
    assert_eq!(real.items[0].project_path, identity.path);
    assert_eq!(count(&db, "projects"), 2);
}
