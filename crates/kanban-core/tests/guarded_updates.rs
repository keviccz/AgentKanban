use kanban_core::{CaptureTask, Database, Error, FeedbackTask, ListTasks, Status};
use serde_json::{json, Value};

struct Fixture {
    root: tempfile::TempDir,
    db: Database,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open(root.path().join("test.sqlite3")).unwrap();
        Self { root, db }
    }

    fn input(&self, key: &str) -> Value {
        json!({"project_path":self.root.path(),"task_key":key,"title":"功能","status":"in_progress","progress":"开始"})
    }

    fn task(&self, key: &str) -> kanban_core::Task {
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
}

#[test]
fn tokens_are_required_for_real_upserts_and_archives_but_not_identical_retries() {
    let fixture = Fixture::new();
    let mut input = fixture.input("guarded");
    input["branch"] = json!("feature/original");
    input["goal"] = json!("保留原目标");
    let created = fixture.db.upsert_from_json(input.clone()).unwrap();
    assert_eq!(fixture.db.upsert_from_json(input.clone()).unwrap(), created);
    let mut omitted_goal = input.clone();
    omitted_goal.as_object_mut().unwrap().remove("goal");
    assert_eq!(
        fixture.db.upsert_from_json(omitted_goal.clone()).unwrap(),
        created
    );
    // Omitting branch clears it and is a real change, even when all other fields match.
    omitted_goal.as_object_mut().unwrap().remove("branch");
    assert!(matches!(
        fixture.db.upsert_from_json(omitted_goal),
        Err(Error::Conflict)
    ));
    input["progress"] = json!("必须携带版本才能更新");
    assert!(matches!(
        fixture.db.upsert_from_json(input.clone()),
        Err(Error::Conflict)
    ));
    assert_eq!(fixture.db.revision().unwrap(), 1);
    assert_eq!(fixture.db.reports(created.id).unwrap().len(), 1);
    input["expected_updated_at"] = json!(created.updated_at);
    let updated = fixture.db.upsert_from_json(input).unwrap();
    let archive = |archived, expected_updated_at| kanban_core::ArchiveTask {
        project_path: fixture.root.path().to_str().unwrap().into(),
        task_key: "guarded".into(),
        archived,
        expected_updated_at,
    };
    assert!(matches!(
        fixture.db.archive(archive(true, None)),
        Err(Error::Conflict)
    ));
    let archived = fixture
        .db
        .archive(archive(true, Some(updated.updated_at)))
        .unwrap();
    assert_eq!(fixture.db.archive(archive(true, None)).unwrap(), archived);
    assert!(matches!(
        fixture.db.archive(archive(false, None)),
        Err(Error::Conflict)
    ));
    fixture
        .db
        .archive(archive(false, Some(archived.updated_at)))
        .unwrap();
    assert!(!fixture.task("guarded").archived);
}

#[test]
fn step_patches_preserve_omitted_fields_clear_notes_and_reject_whole_invalid_transactions() {
    let fixture = Fixture::new();
    let mut input = fixture.input("steps");
    input["steps"] = json!([
        {"title":"实现","status":"in_progress","note":"保留此备注"},
        {"title":"验证","status":"todo","note":"待清除"}
    ]);
    let created = fixture.db.upsert_from_json(input).unwrap();
    let mut update = fixture.input("steps");
    update["expected_updated_at"] = json!(created.updated_at);
    update["step_updates"] = json!([{"index":0,"status":"done"},{"index":1,"note":""}]);
    let moved = fixture.db.upsert_from_json(update.clone()).unwrap();
    let task = fixture.task("steps");
    assert_eq!(task.steps[0].status, kanban_core::StepStatus::Done);
    assert_eq!(task.steps[0].note, "保留此备注");
    assert_eq!(task.steps[1].status, kanban_core::StepStatus::Todo);
    assert!(task.steps[1].note.is_empty());
    // An identical retry without a token is safe; an old explicit token is not.
    assert!(matches!(
        fixture.db.upsert_from_json(update.clone()),
        Err(Error::Conflict)
    ));
    update
        .as_object_mut()
        .unwrap()
        .remove("expected_updated_at");
    assert_eq!(fixture.db.upsert_from_json(update).unwrap(), moved);
    let revision = fixture.db.revision().unwrap();
    let reports = fixture.db.reports(moved.id).unwrap();
    for patches in [
        json!([{"index":0,"status":"todo"},{"index":0,"status":"done"}]),
        json!([{"index":0,"status":"todo"},{"index":2,"status":"done"}]),
        json!([{"index":1}]),
        json!([{"index":1,"note":null}]),
        json!([{"index":1,"note":"长".repeat(201)}]),
    ] {
        let mut invalid = fixture.input("steps");
        invalid["expected_updated_at"] = json!(moved.updated_at);
        invalid["progress"] = json!("整单不能保存此进展");
        invalid["step_updates"] = patches;
        assert!(matches!(
            fixture.db.upsert_from_json(invalid),
            Err(Error::InvalidInput(_))
        ));
        assert_eq!(fixture.task("steps"), task);
        assert_eq!(fixture.db.revision().unwrap(), revision);
        assert_eq!(fixture.db.reports(moved.id).unwrap(), reports);
    }
    let mut both = fixture.input("steps");
    both["expected_updated_at"] = json!(moved.updated_at);
    both["steps"] = json!([]);
    both["step_updates"] = json!([]);
    assert!(matches!(
        fixture.db.upsert_from_json(both),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn invalid_new_step_patch_leaves_no_partial_project_or_task() {
    let fixture = Fixture::new();
    let mut input = fixture.input("new");
    input["step_updates"] = json!([{"index":0,"status":"done"}]);
    assert!(matches!(
        fixture.db.upsert_from_json(input),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fixture.db.revision().unwrap(), 0);
    let connection = rusqlite::Connection::open(fixture.db.path()).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM projects", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn stale_step_indices_cannot_modify_a_reordered_plan() {
    let fixture = Fixture::new();
    let mut input = fixture.input("reorder");
    input["steps"] = json!([{"title":"甲","status":"todo"},{"title":"乙","status":"todo"}]);
    let initial = fixture.db.upsert_from_json(input.clone()).unwrap();
    input["steps"].as_array_mut().unwrap().reverse();
    input["expected_updated_at"] = json!(initial.updated_at);
    fixture.db.upsert_from_json(input).unwrap();
    let before = fixture.task("reorder");
    let mut stale = fixture.input("reorder");
    stale["step_updates"] = json!([{"index":0,"status":"done"}]);
    stale["expected_updated_at"] = json!(initial.updated_at);
    assert!(matches!(
        fixture.db.upsert_from_json(stale),
        Err(Error::Conflict)
    ));
    assert_eq!(fixture.task("reorder"), before);
}

#[test]
fn keyword_search_matches_handoff_fields_and_treats_like_metacharacters_literally() {
    let fixture = Fixture::new();
    let captured = fixture
        .db
        .capture(CaptureTask {
            project_path: fixture.root.path().to_str().unwrap().into(),
            task_key: "feature:lookup".into(),
            title: "查找标题".into(),
            request: "原始约束百分比 50%_\\ 禁止覆盖".into(),
        })
        .unwrap();
    fixture
        .db
        .feedback(FeedbackTask {
            id: captured.id,
            expected_updated_at: captured.updated_at,
            note: "补充独特意见".into(),
        })
        .unwrap();
    fixture
        .db
        .upsert_from_json(fixture.input("unrelated"))
        .unwrap();
    for keyword in [
        "feature:lookup",
        "查找标题",
        "原始约束",
        "补充独特意见",
        "%",
        "_",
        "\\",
        "%_\\",
    ] {
        let page = fixture
            .db
            .list(ListTasks {
                query: Some(keyword.into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(page.items.len(), 1, "query={keyword}");
        assert_eq!(page.items[0].task.id, captured.id);
    }
    for query in [String::new(), " ".into(), "字".repeat(161), "多\n行".into()] {
        assert!(matches!(
            fixture.db.list(ListTasks {
                query: Some(query),
                ..Default::default()
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    let mut input = fixture.input("goal-search");
    input["goal"] = json!("独特目标词");
    input["progress"] = json!("独特进展词");
    fixture.db.upsert_from_json(input).unwrap();
    for query in ["独特目标词", "独特进展词"] {
        assert_eq!(
            fixture
                .db
                .list(ListTasks {
                    query: Some(query.into()),
                    ..Default::default()
                })
                .unwrap()
                .items[0]
                .task
                .task_key,
            "goal-search"
        );
    }
}

#[test]
fn json_reports_preserve_omission_null_and_explicit_empty_step_notes() {
    let fixture = Fixture::new();
    let mut original = fixture.input("raw-report");
    original["agent"] = Value::Null;
    original["steps"] = json!([{"title":"原字段","status":"todo","note":""}]);
    let receipt = fixture.db.upsert_from_json(original.clone()).unwrap();
    let mut expected = original;
    expected.as_object_mut().unwrap().remove("project_path");
    expected.as_object_mut().unwrap().remove("task_key");
    let report = &fixture.db.reports(receipt.id).unwrap()[0].payload;
    assert_eq!(report, &expected);
    assert!(report.get("branch").is_none());
    assert_eq!(report["agent"], Value::Null);
    assert!(report.get("agent").is_some());
    assert_eq!(report["steps"][0]["note"], "");
    let mut update = fixture.input("raw-report");
    update["expected_updated_at"] = json!(receipt.updated_at);
    update["progress"] = json!("新上报");
    update["branch"] = Value::Null;
    fixture.db.upsert_from_json(update.clone()).unwrap();
    update.as_object_mut().unwrap().remove("project_path");
    update.as_object_mut().unwrap().remove("task_key");
    update
        .as_object_mut()
        .unwrap()
        .remove("expected_updated_at");
    assert_eq!(fixture.db.reports(receipt.id).unwrap()[0].payload, update);
    assert_eq!(fixture.task("raw-report").status, Status::InProgress);
}
