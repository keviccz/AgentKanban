use kanban_core::{
    ArchiveById, ArchiveQuery, CaptureTask, Database, Error, FeedbackTask, ListTasks, ReviewStatus,
    ReviewTask, Status, Task, TaskReceipt,
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
    time::Duration,
};

struct Fixture {
    _root: tempfile::TempDir,
    project: PathBuf,
    db: Database,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("归档测试项目");
        std::fs::create_dir(&project).unwrap();
        let db = Database::open(root.path().join("data/test.sqlite3")).unwrap();
        Self {
            _root: root,
            project,
            db,
        }
    }

    fn seed(&self, key: &str, status: Status) -> TaskReceipt {
        self.db
            .upsert_from_json(json!({
                "project_path": self.project, "task_key": key, "title": "归档任务",
                "status": status, "progress": "保留进展"
            }))
            .unwrap()
    }

    fn task(&self, key: &str) -> Task {
        self.db
            .list(ListTasks {
                task_key: Some(key.into()),
                include_done: true,
                include_archived: true,
                ..Default::default()
            })
            .unwrap()
            .items
            .remove(0)
            .task
    }

    fn archive(&self, task: TaskReceipt) -> TaskReceipt {
        self.db
            .archive_by_id(ArchiveById {
                id: task.id,
                expected_updated_at: task.updated_at,
            })
            .unwrap()
    }
}

#[test]
fn archived_pages_include_all_statuses_and_have_a_stable_tie_breaker() {
    let fixture = Fixture::new();
    let mut ids = Vec::new();
    for index in 0..23 {
        let status = [
            Status::Todo,
            Status::InProgress,
            Status::Blocked,
            Status::Done,
        ][index % 4];
        ids.push(
            fixture
                .archive(fixture.seed(&format!("archive:{index}"), status))
                .id,
        );
    }
    for (index, status) in [
        Status::Todo,
        Status::InProgress,
        Status::Blocked,
        Status::Done,
    ]
    .into_iter()
    .enumerate()
    {
        fixture.seed(&format!("active:{index}"), status);
    }
    // Deliberately identical timestamps verify ordering does not depend on status or row scan order.
    rusqlite::Connection::open(fixture.db.path())
        .unwrap()
        .execute(
            "UPDATE tasks SET updated_at='2026-01-01T00:00:00.000Z' WHERE archived=1",
            [],
        )
        .unwrap();
    let revision = fixture.db.revision().unwrap();
    let first = fixture.db.list_archived(ArchiveQuery::default()).unwrap();
    assert_eq!(first.items.len(), 20);
    assert_eq!(first.next_offset, Some(20));
    assert!(first.items.iter().all(|item| item.task.archived));
    let second = fixture
        .db
        .list_archived(ArchiveQuery {
            offset: first.next_offset.unwrap(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(second.items.len(), 3);
    assert_eq!(second.next_offset, None);
    let project_id = first.items[0].task.project_id;
    let scoped = |project_id| {
        fixture
            .db
            .list_archived(ArchiveQuery {
                project_id: Some(project_id),
                limit: 100,
                ..Default::default()
            })
            .unwrap()
            .items
            .len()
    };
    assert_eq!(scoped(project_id), 23);
    assert_eq!(scoped(project_id + 1_000), 0);
    ids.reverse();
    assert_eq!(
        first
            .items
            .iter()
            .chain(&second.items)
            .map(|item| item.task.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert_eq!(fixture.db.board().unwrap().projects[0].tasks.len(), 4);
}

#[test]
fn search_covers_project_and_human_fields_and_treats_sql_wildcards_literally() {
    let fixture = Fixture::new();
    let captured = fixture
        .db
        .capture(CaptureTask {
            project_path: fixture.project.to_str().unwrap().into(),
            task_key: "KEY:Alpha".into(),
            title: "初始标题".into(),
            request: "原始请求检索词".into(),
        })
        .unwrap();
    let updated = fixture
        .db
        .upsert_from_json(json!({
            "project_path": fixture.project, "task_key": "KEY:Alpha",
            "title": "查找标题 100% 20_个 \\literal 'quote'", "status": "done",
            "goal": "独有目标检索词", "progress": "独有进展检索词",
            "expected_updated_at": captured.updated_at
        }))
        .unwrap();
    let noted = fixture
        .db
        .feedback(FeedbackTask {
            id: updated.id,
            expected_updated_at: updated.updated_at,
            note: "人工补充检索词".into(),
        })
        .unwrap();
    let target = fixture.archive(noted);
    let other_project = fixture._root.path().join("other-project");
    std::fs::create_dir(&other_project).unwrap();
    let other = fixture.db.upsert_from_json(json!({
        "project_path": other_project, "task_key": "other", "title": "100x 20a个 xliteral quote",
        "status": "todo", "progress": "普通内容"
    })).unwrap();
    fixture.archive(other);
    for query in [
        "查找标题",
        "key:alpha",
        "独有目标检索词",
        "独有进展检索词",
        "原始请求检索词",
        "人工补充检索词",
        "归档测试项目",
        fixture.project.to_str().unwrap(),
        "100%",
        "20_",
        r"\literal",
        "'quote'",
        "  人工补充检索词  ",
    ] {
        let page = fixture
            .db
            .list_archived(ArchiveQuery {
                query: Some(query.into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.items.len(), 1, "query={query}");
        assert_eq!(page.items[0].task.id, target.id);
    }
    let injection = fixture
        .db
        .list_archived(ArchiveQuery {
            query: Some("%' OR 1=1 --".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(injection.items.is_empty());
    // Filtering happens before pagination, and the matching active row stays out.
    fixture.seed("active:alpha", Status::Todo);
    let first = fixture
        .db
        .list_archived(ArchiveQuery {
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert_eq!(first.next_offset, Some(1));
    let filtered = fixture
        .db
        .list_archived(ArchiveQuery {
            query: Some("key:alpha".into()),
            limit: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.items[0].task.id, target.id);
    assert_eq!(filtered.next_offset, None);
}

#[test]
fn query_contract_rejects_invalid_input_and_large_offsets_are_safe() {
    let fixture = Fixture::new();
    fixture.archive(fixture.seed("validation", Status::Todo));
    let defaults: ArchiveQuery = serde_json::from_value(json!({})).unwrap();
    assert_eq!(
        (defaults.limit, defaults.offset, defaults.query),
        (20, 0, None)
    );
    for input in [
        json!({"query": null}),
        json!({"offset": -1}),
        json!({"include_done": true}),
    ] {
        assert!(serde_json::from_value::<ArchiveQuery>(input).is_err());
    }
    for query in [
        String::new(),
        " ".into(),
        "字".repeat(161),
        "多\n行".into(),
        "bad\0query".into(),
    ] {
        assert!(matches!(
            fixture.db.list_archived(ArchiveQuery {
                query: Some(query),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    for limit in [0, 101, u32::MAX] {
        assert!(matches!(
            fixture.db.list_archived(ArchiveQuery {
                limit,
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    let page = fixture
        .db
        .list_archived(ArchiveQuery {
            offset: u32::MAX,
            limit: 100,
            ..Default::default()
        })
        .unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.next_offset, None);
    assert!(fixture
        .db
        .list_archived(ArchiveQuery {
            query: Some("字".repeat(160)),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn restoration_preserves_every_task_field_review_state_and_report() {
    for (key, initial_status, review, withdraw) in [
        ("unfinished", Status::Blocked, None, false),
        ("pending", Status::Done, None, false),
        ("accepted", Status::Done, Some(true), false),
        ("changes-requested", Status::Done, Some(false), false),
        ("withdrawn", Status::Done, None, true),
    ] {
        let fixture = Fixture::new();
        let captured = fixture
            .db
            .capture(CaptureTask {
                project_path: fixture.project.to_str().unwrap().into(),
                task_key: key.into(),
                title: "用户需求".into(),
                request: "保留需求\n第二行".into(),
            })
            .unwrap();
        let mut update = json!({
            "project_path": fixture.project, "task_key": key, "title": "保留完整交付",
            "status": initial_status, "progress": "已完成检查", "branch": "feature/archive",
            "agent": "fixture-agent", "next_action": "等待用户选择", "needs_input": "请确认范围",
            "goal": "验证归档恢复", "acceptance": ["原有字段不丢失", "报告原样保留"],
            "steps": [{"title": "检查", "status": "done", "note": "保留步骤说明"}],
            "deliverables": [{"label": "示例报告", "uri": "https://example.com/report"}],
            "expected_updated_at": captured.updated_at
        });
        let mut receipt = fixture.db.upsert_from_json(update.clone()).unwrap();
        if withdraw {
            update["status"] = json!("in_progress");
            update["expected_updated_at"] = json!(receipt.updated_at);
            receipt = fixture.db.upsert_from_json(update).unwrap();
        }
        if let Some(accepted) = review {
            receipt = fixture
                .db
                .review(ReviewTask {
                    id: receipt.id,
                    expected_updated_at: receipt.updated_at,
                    accepted,
                    note: "保留验收说明".into(),
                })
                .unwrap();
        }
        receipt = fixture
            .db
            .feedback(FeedbackTask {
                id: receipt.id,
                expected_updated_at: receipt.updated_at,
                note: "保留人工补充\n第二行".into(),
            })
            .unwrap();
        fixture.archive(receipt);
        let before = fixture.task(key);
        let reports = fixture.db.reports(before.id).unwrap();
        let revision = fixture.db.revision().unwrap();
        assert!(!reports.is_empty());
        let restored = fixture
            .db
            .restore_by_id(ArchiveById {
                id: before.id,
                expected_updated_at: before.updated_at.clone(),
            })
            .unwrap();
        let mut expected = before.clone();
        expected.archived = false;
        expected.updated_at = restored.updated_at.clone();
        assert_eq!(fixture.task(key), expected, "case={key}");
        assert!(restored.updated_at > before.updated_at);
        assert_eq!(fixture.db.revision().unwrap(), revision + 1);
        assert_eq!(fixture.db.reports(before.id).unwrap(), reports);
        assert!(fixture
            .db
            .list_archived(ArchiveQuery::default())
            .unwrap()
            .items
            .is_empty());
        assert_eq!(fixture.db.board().unwrap().projects[0].tasks[0], expected);
    }
}

#[test]
fn stale_missing_or_invalid_restores_do_not_change_rows_or_revision() {
    let fixture = Fixture::new();
    let archived = fixture.archive(fixture.seed("conflict", Status::InProgress));
    fixture
        .db
        .feedback(FeedbackTask {
            id: archived.id,
            expected_updated_at: archived.updated_at.clone(),
            note: "归档后补充".into(),
        })
        .unwrap();
    let before = fixture.task("conflict");
    let reports = fixture.db.reports(before.id).unwrap();
    let revision = fixture.db.revision().unwrap();
    assert!(matches!(
        fixture.db.restore_by_id(ArchiveById {
            id: archived.id,
            expected_updated_at: archived.updated_at,
        }),
        Err(Error::Conflict)
    ));
    assert!(matches!(
        fixture.db.restore_by_id(ArchiveById {
            id: i64::MAX,
            expected_updated_at: before.updated_at.clone(),
        }),
        Err(Error::TaskNotFound)
    ));
    for input in [
        ArchiveById {
            id: 0,
            expected_updated_at: before.updated_at.clone(),
        },
        ArchiveById {
            id: -1,
            expected_updated_at: before.updated_at.clone(),
        },
        ArchiveById {
            id: before.id,
            expected_updated_at: String::new(),
        },
    ] {
        assert!(matches!(
            fixture.db.restore_by_id(input),
            Err(Error::InvalidInput(_))
        ));
    }
    assert_eq!(fixture.task("conflict"), before);
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert_eq!(fixture.db.reports(before.id).unwrap(), reports);
}

#[test]
fn a_current_noop_is_idempotent_but_a_stale_retry_still_conflicts() {
    let fixture = Fixture::new();
    let archived = fixture.archive(fixture.seed("idempotent", Status::Todo));
    let restore = ArchiveById {
        id: archived.id,
        expected_updated_at: archived.updated_at,
    };
    let restored = fixture.db.restore_by_id(restore.clone()).unwrap();
    let revision = fixture.db.revision().unwrap();
    assert_eq!(
        fixture
            .db
            .restore_by_id(ArchiveById {
                id: restored.id,
                expected_updated_at: restored.updated_at.clone(),
            })
            .unwrap(),
        restored
    );
    assert!(matches!(
        fixture.db.restore_by_id(restore),
        Err(Error::Conflict)
    ));
    assert_eq!(fixture.db.revision().unwrap(), revision);
}

#[test]
fn concurrent_restores_allow_one_change_and_preserve_the_original_row() {
    let fixture = Fixture::new();
    let archived = fixture.archive(fixture.seed("concurrent", Status::Blocked));
    let before = fixture.task("concurrent");
    let revision = fixture.db.revision().unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = (0..2)
        .map(|_| {
            let db = fixture.db.clone();
            let barrier = Arc::clone(&barrier);
            let input = ArchiveById {
                id: archived.id,
                expected_updated_at: archived.updated_at.clone(),
            };
            std::thread::spawn(move || {
                barrier.wait();
                db.restore_by_id(input)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Conflict)))
            .count(),
        1
    );
    assert_eq!(fixture.db.revision().unwrap(), revision + 1);
    let after = fixture.task("concurrent");
    assert_eq!(
        (after.id, after.project_id, after.task_key),
        (before.id, before.project_id, before.task_key)
    );
    assert!(!after.archived);
}

#[test]
fn a_database_failure_rolls_back_both_restoration_and_revision() {
    let fixture = Fixture::new();
    let archived = fixture.archive(fixture.seed("transaction", Status::InProgress));
    let before = fixture.task("transaction");
    let reports = fixture.db.reports(before.id).unwrap();
    let revision = fixture.db.revision().unwrap();
    rusqlite::Connection::open(fixture.db.path())
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_revision BEFORE UPDATE ON metadata
             WHEN NEW.key='revision'
             BEGIN SELECT RAISE(ABORT,'injected revision failure'); END;",
        )
        .unwrap();
    assert!(matches!(
        fixture.db.restore_by_id(ArchiveById {
            id: archived.id,
            expected_updated_at: archived.updated_at,
        }),
        Err(Error::Database(_))
    ));
    assert_eq!(fixture.task("transaction"), before);
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert_eq!(fixture.db.reports(before.id).unwrap(), reports);
}

#[test]
fn restoration_works_after_project_directory_disappears_and_resets_auto_archive_age() {
    let fixture = Fixture::new();
    let done = fixture.seed("accepted", Status::Done);
    let accepted = fixture
        .db
        .review(ReviewTask {
            id: done.id,
            expected_updated_at: done.updated_at,
            accepted: true,
            note: String::new(),
        })
        .unwrap();
    let archived = fixture.archive(accepted);
    let conn = rusqlite::Connection::open(fixture.db.path()).unwrap();
    conn.execute(
        "UPDATE tasks SET updated_at='2020-01-01T00:00:00.000Z' WHERE id=?1",
        [archived.id],
    )
    .unwrap();
    let identity: (i64, String, String, String) = conn
        .query_row("SELECT id,identity,name,path FROM projects", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    std::fs::remove_dir(&fixture.project).unwrap();
    let row = fixture
        .db
        .list_archived(ArchiveQuery::default())
        .unwrap()
        .items
        .remove(0);
    assert_eq!(row.project_path, identity.3);
    let restored = fixture
        .db
        .restore_by_id(ArchiveById {
            id: row.task.id,
            expected_updated_at: row.task.updated_at,
        })
        .unwrap();
    assert!(restored.updated_at.as_str() > "2020-01-01T00:00:00.000Z");
    assert_eq!(
        fixture
            .db
            .archive_finished(Duration::from_secs(24 * 60 * 60))
            .unwrap(),
        0
    );
    let project_after: (i64, String, String, String) = conn
        .query_row("SELECT id,identity,name,path FROM projects", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap();
    assert_eq!(identity, project_after);
    let task = fixture.task("accepted");
    assert!(!task.archived);
    assert_eq!(task.review_status, ReviewStatus::Accepted);
}

#[test]
fn archive_project_hides_every_board_task_and_each_restores_on_its_own() {
    let fixture = Fixture::new();
    let working = fixture.seed("project:working", Status::InProgress);
    fixture.seed("project:done", Status::Done);
    fixture.archive(fixture.seed("project:already", Status::Todo));
    let project_id = fixture.task("project:working").project_id;
    let revision = fixture.db.revision().unwrap();

    assert_eq!(fixture.db.archive_project(project_id).unwrap(), 2);
    assert_eq!(fixture.db.revision().unwrap(), revision + 1);
    assert!(fixture.db.board().unwrap().projects.is_empty());
    let task = fixture.task("project:working");
    assert!(task.archived);
    assert_eq!(task.status, Status::InProgress);

    // Nothing left to archive: no revision bump, and bad ids are rejected.
    assert_eq!(fixture.db.archive_project(project_id).unwrap(), 0);
    assert_eq!(fixture.db.revision().unwrap(), revision + 1);
    assert!(matches!(fixture.db.archive_project(0), Err(Error::InvalidInput(_))));

    fixture
        .db
        .restore_by_id(ArchiveById {
            id: working.id,
            expected_updated_at: task.updated_at,
        })
        .unwrap();
    assert!(!fixture.task("project:working").archived);
    assert!(fixture.task("project:done").archived);
}
