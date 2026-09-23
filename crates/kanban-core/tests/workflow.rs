use kanban_core::{
    ArchiveById, ArchiveTask, CaptureTask, Database, Deliverable, Error, FeedbackTask, ListTasks,
    ReviewStatus, ReviewTask, Status, Step, StepStatus, Task, UpsertTask,
};
use std::sync::{Arc, Barrier};

struct Fixture {
    root: tempfile::TempDir,
    db: Database,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open(root.path().join("data/agentkanban.sqlite3")).unwrap();
        Self { root, db }
    }

    fn capture(&self, key: &str) -> CaptureTask {
        CaptureTask {
            project_path: self.root.path().to_str().unwrap().into(),
            task_key: key.into(),
            title: "用户原始需求".into(),
            request: "保留原始数据\n交付可打开的报告，支持中文。".into(),
        }
    }

    fn upsert(&self, key: &str, status: Status) -> UpsertTask {
        UpsertTask {
            project_path: self.root.path().to_str().unwrap().into(),
            task_key: key.into(),
            title: "用户原始需求".into(),
            status,
            progress: "Agent 正在处理".into(),
            branch: Some("feature/report".into()),
            agent: None,
            next_action: None,
            needs_input: None,
            deliverables: None,
            steps: None,
            expected_updated_at: None,
        }
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
            .pop()
            .unwrap()
            .task
    }

    fn archive(&self, key: &str, archived: bool, expected: Option<String>) -> ArchiveTask {
        ArchiveTask {
            project_path: self.root.path().to_str().unwrap().into(),
            task_key: key.into(),
            archived,
            expected_updated_at: expected,
        }
    }
}

fn delivery() -> Vec<Deliverable> {
    vec![
        Deliverable {
            label: "中文交付报告".into(),
            uri: "C:\\项目 空格\\report.html".into(),
        },
        Deliverable {
            label: "使用说明".into(),
            uri: "https://example.com/report".into(),
        },
    ]
}

#[test]
fn capture_retries_cannot_overwrite_agent_work_or_resurrect_archived_tasks() {
    let fixture = Fixture::new();
    let capture = fixture.capture("user-uuid");
    let first = fixture.db.capture(capture.clone()).unwrap();
    assert_eq!(first.status, Status::Todo);
    let task = fixture.task("user-uuid");
    assert_eq!(task.request, capture.request);
    assert_eq!(task.progress, "等待 Agent 接手");
    assert_eq!(task.agent_updated_at, None);
    assert_eq!(task.review_status, ReviewStatus::None);
    let changed_retry = CaptureTask {
        title: "retry must not replace title".into(),
        request: "retry must not replace request".into(),
        ..capture.clone()
    };
    assert_eq!(fixture.db.capture(changed_retry.clone()).unwrap(), first);
    assert_eq!(fixture.db.revision().unwrap(), 1);
    let mut takeover = fixture.upsert("user-uuid", Status::InProgress);
    takeover.agent = Some("Codex".into());
    takeover.next_action = Some("生成报告并验证".into());
    takeover.expected_updated_at = Some(first.updated_at);
    let taken = fixture.db.upsert(takeover).unwrap();
    assert_eq!(taken.id, first.id);
    assert_eq!(fixture.db.capture(changed_retry.clone()).unwrap(), taken);
    let task = fixture.task("user-uuid");
    assert_eq!(task.request, capture.request);
    assert_eq!(task.agent.as_deref(), Some("Codex"));
    assert_eq!(task.next_action, "生成报告并验证");
    assert_eq!(task.agent_updated_at.as_ref(), Some(&taken.updated_at));
    let archived = fixture
        .db
        .archive(fixture.archive("user-uuid", true, Some(taken.updated_at)))
        .unwrap();
    let revision = fixture.db.revision().unwrap();
    assert_eq!(fixture.db.capture(changed_retry).unwrap(), archived);
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert!(fixture.task("user-uuid").archived);
}

#[test]
fn rejected_delivery_reappears_for_agent_and_can_be_reworked_and_accepted() {
    let fixture = Fixture::new();
    let captured = fixture
        .db
        .capture(fixture.capture("delivery-cycle"))
        .unwrap();
    let mut completed = fixture.upsert("delivery-cycle", Status::Done);
    completed.deliverables = Some(delivery());
    completed.agent = Some("Claude Code".into());
    completed.expected_updated_at = Some(captured.updated_at);
    let done = fixture.db.upsert(completed).unwrap();
    assert!(fixture
        .db
        .list(ListTasks::default())
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        fixture.task("delivery-cycle").review_status,
        ReviewStatus::Pending
    );
    let reject = fixture
        .db
        .review(ReviewTask {
            id: done.id,
            expected_updated_at: done.updated_at.clone(),
            accepted: false,
            note: "图表缺少图例\n请标明坐标单位".into(),
        })
        .unwrap();
    let discovered = fixture.db.list(ListTasks::default()).unwrap().items;
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].task.id, captured.id);
    assert_eq!(discovered[0].task.status, Status::Todo);
    assert_eq!(
        discovered[0].task.review_status,
        ReviewStatus::ChangesRequested
    );
    assert_eq!(discovered[0].task.user_note, "图表缺少图例\n请标明坐标单位");
    assert_eq!(discovered[0].task.deliverables, delivery());
    assert_eq!(
        discovered[0].task.agent_updated_at.as_deref(),
        Some(done.updated_at.as_str())
    );
    assert!(reject.updated_at > done.updated_at);
    let mut rework = fixture.upsert("delivery-cycle", Status::InProgress);
    rework.expected_updated_at = Some(reject.updated_at);
    let working = fixture.db.upsert(rework).unwrap();
    assert_eq!(
        fixture.task("delivery-cycle").review_status,
        ReviewStatus::ChangesRequested
    );
    let mut redeliver = fixture.upsert("delivery-cycle", Status::Done);
    redeliver.expected_updated_at = Some(working.updated_at);
    let done_again = fixture.db.upsert(redeliver).unwrap();
    assert_eq!(
        fixture.task("delivery-cycle").review_status,
        ReviewStatus::Pending
    );
    let accepted = fixture
        .db
        .review(ReviewTask {
            id: done_again.id,
            expected_updated_at: done_again.updated_at.clone(),
            accepted: true,
            note: String::new(),
        })
        .unwrap();
    let final_task = fixture.task("delivery-cycle");
    assert_eq!(accepted.status, Status::Done);
    assert_eq!(final_task.review_status, ReviewStatus::Accepted);
    assert_eq!(final_task.user_note, "图表缺少图例\n请标明坐标单位");
    assert_eq!(final_task.agent_updated_at, Some(done_again.updated_at));
    assert_eq!(final_task.deliverables, delivery());
}

#[test]
fn optional_patches_preserve_values_and_noop_keeps_accepted_review_and_timestamps() {
    let fixture = Fixture::new();
    let mut input = fixture.upsert("patch", Status::Done);
    input.agent = Some("Cursor".into());
    input.next_action = Some("等待人工检查".into());
    input.needs_input = Some("确认是否符合预期".into());
    input.deliverables = Some(delivery());
    let done = fixture.db.upsert(input).unwrap();
    let accepted = fixture
        .db
        .review(ReviewTask {
            id: done.id,
            expected_updated_at: done.updated_at,
            accepted: true,
            note: "验收通过".into(),
        })
        .unwrap();
    let before = fixture.task("patch");
    let revision = fixture.db.revision().unwrap();
    let mut omitted = fixture.upsert("patch", Status::Done);
    omitted.expected_updated_at = Some(accepted.updated_at.clone());
    assert_eq!(fixture.db.upsert(omitted.clone()).unwrap(), accepted);
    assert_eq!(fixture.task("patch"), before);
    assert_eq!(fixture.db.revision().unwrap(), revision);
    // Only a meaningful new Agent field changes acceptance back to pending.
    omitted.next_action = Some("提供补充交付".into());
    let updated = fixture.db.upsert(omitted).unwrap();
    let after = fixture.task("patch");
    assert_eq!(after.review_status, ReviewStatus::Pending);
    assert_eq!(after.agent_updated_at.as_ref(), Some(&updated.updated_at));
    assert_eq!(after.agent.as_deref(), Some("Cursor"));
    assert_eq!(after.deliverables, delivery());
    // Old branch replacement semantics remain, while explicit empty new fields clear.
    let mut clear = fixture.upsert("patch", Status::Done);
    clear.branch = None;
    clear.agent = Some(String::new());
    clear.next_action = Some(String::new());
    clear.needs_input = Some(String::new());
    clear.deliverables = Some(vec![]);
    let cleared = fixture.db.upsert(clear.clone()).unwrap();
    let task = fixture.task("patch");
    assert_eq!(task.branch, None);
    assert_eq!(task.agent, None);
    assert!(
        task.next_action.is_empty() && task.needs_input.is_empty() && task.deliverables.is_empty()
    );
    assert_eq!(task.user_note, "验收通过");
    assert_eq!(fixture.db.upsert(clear).unwrap(), cleared);
}

#[test]
fn human_feedback_preserves_agent_fields_and_invalidates_stale_agent_or_review_actions() {
    let fixture = Fixture::new();
    let done = fixture
        .db
        .upsert(fixture.upsert("feedback", Status::Done))
        .unwrap();
    let before = fixture.task("feedback");
    let feedback = fixture
        .db
        .feedback(FeedbackTask {
            id: done.id,
            expected_updated_at: done.updated_at.clone(),
            note: "额外要求\n保留所有历史结果".into(),
        })
        .unwrap();
    let mut expected_after = before;
    expected_after.user_note = "额外要求\n保留所有历史结果".into();
    expected_after.updated_at = feedback.updated_at.clone();
    assert_eq!(fixture.task("feedback"), expected_after);
    assert!(feedback.updated_at > done.updated_at);
    let revision = fixture.db.revision().unwrap();
    let mut stale = fixture.upsert("feedback", Status::Done);
    stale.progress = "stale agent must not overwrite".into();
    stale.expected_updated_at = Some(done.updated_at.clone());
    assert!(matches!(fixture.db.upsert(stale), Err(Error::Conflict)));
    assert!(matches!(
        fixture.db.review(ReviewTask {
            id: done.id,
            expected_updated_at: done.updated_at,
            accepted: true,
            note: String::new()
        }),
        Err(Error::Conflict)
    ));
    assert_eq!(fixture.task("feedback"), expected_after);
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert_eq!(
        fixture
            .db
            .feedback(FeedbackTask {
                id: done.id,
                expected_updated_at: feedback.updated_at.clone(),
                note: expected_after.user_note
            })
            .unwrap(),
        feedback
    );
    assert_eq!(fixture.db.revision().unwrap(), revision);
    fixture
        .db
        .feedback(FeedbackTask {
            id: done.id,
            expected_updated_at: feedback.updated_at,
            note: String::new(),
        })
        .unwrap();
    assert!(fixture.task("feedback").user_note.is_empty());
}

#[test]
fn rejection_requires_a_note_and_only_pending_unarchived_done_tasks_are_reviewable() {
    let fixture = Fixture::new();
    let todo = fixture.db.capture(fixture.capture("review-guard")).unwrap();
    assert!(matches!(
        fixture.db.review(ReviewTask {
            id: todo.id,
            expected_updated_at: todo.updated_at,
            accepted: true,
            note: String::new()
        }),
        Err(Error::NotReviewable)
    ));
    let done = fixture
        .db
        .upsert(fixture.upsert("review-guard", Status::Done))
        .unwrap();
    let revision = fixture.db.revision().unwrap();
    assert!(matches!(
        fixture.db.review(ReviewTask {
            id: done.id,
            expected_updated_at: done.updated_at.clone(),
            accepted: false,
            note: " \n\t".into()
        }),
        Err(Error::InvalidInput(_))
    ));
    assert_eq!(fixture.db.revision().unwrap(), revision);
    let archived = fixture
        .db
        .archive(fixture.archive("review-guard", true, Some(done.updated_at)))
        .unwrap();
    assert!(matches!(
        fixture.db.review(ReviewTask {
            id: done.id,
            expected_updated_at: archived.updated_at,
            accepted: true,
            note: String::new()
        }),
        Err(Error::NotReviewable)
    ));
    fixture
        .db
        .archive(fixture.archive("review-guard", false, None))
        .unwrap();
    let task = fixture.task("review-guard");
    let accepted = fixture
        .db
        .review(ReviewTask {
            id: task.id,
            expected_updated_at: task.updated_at,
            accepted: true,
            note: "符合要求".into(),
        })
        .unwrap();
    assert!(matches!(
        fixture.db.review(ReviewTask {
            id: accepted.id,
            expected_updated_at: accepted.updated_at,
            accepted: true,
            note: "不能重复验收".into()
        }),
        Err(Error::NotReviewable)
    ));
}

#[test]
fn optimistic_writes_from_agent_and_human_have_exactly_one_winner() {
    let fixture = Fixture::new();
    let initial = fixture
        .db
        .upsert(fixture.upsert("racing", Status::InProgress))
        .unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut input = fixture.upsert("racing", Status::Done);
    input.expected_updated_at = Some(initial.updated_at.clone());
    let agent_db = fixture.db.clone();
    let agent_barrier = barrier.clone();
    let agent = std::thread::spawn(move || {
        agent_barrier.wait();
        agent_db.upsert(input)
    });
    let human_db = fixture.db.clone();
    let human = std::thread::spawn(move || {
        barrier.wait();
        human_db.feedback(FeedbackTask {
            id: initial.id,
            expected_updated_at: initial.updated_at,
            note: "修改需求".into(),
        })
    });
    let results = [agent.join().unwrap(), human.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(Error::Conflict)))
            .count(),
        1
    );
    assert_eq!(fixture.db.revision().unwrap(), 2);
}

#[test]
fn archive_conflicts_are_read_only_and_real_archive_changes_report_agent_activity() {
    let fixture = Fixture::new();
    let created = fixture
        .db
        .upsert(fixture.upsert("archive-cas", Status::Todo))
        .unwrap();
    let noted = fixture
        .db
        .feedback(FeedbackTask {
            id: created.id,
            expected_updated_at: created.updated_at.clone(),
            note: "用户刚补充".into(),
        })
        .unwrap();
    assert!(matches!(
        fixture
            .db
            .archive(fixture.archive("archive-cas", true, Some(created.updated_at))),
        Err(Error::Conflict)
    ));
    assert!(!fixture.task("archive-cas").archived);
    let archived = fixture
        .db
        .archive(fixture.archive("archive-cas", true, Some(noted.updated_at)))
        .unwrap();
    let task = fixture.task("archive-cas");
    assert_eq!(task.agent_updated_at.as_ref(), Some(&archived.updated_at));
    assert_eq!(
        fixture
            .db
            .archive(fixture.archive("archive-cas", true, Some(archived.updated_at.clone())))
            .unwrap(),
        archived
    );
    let mut missing = fixture.upsert("missing", Status::Todo);
    missing.expected_updated_at = Some(archived.updated_at);
    let revision = fixture.db.revision().unwrap();
    assert!(matches!(fixture.db.upsert(missing), Err(Error::Conflict)));
    assert_eq!(fixture.db.revision().unwrap(), revision);
    assert!(fixture
        .db
        .list(ListTasks {
            task_key: Some("missing".into()),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty());
}

#[test]
fn exact_key_filter_keeps_done_and_archive_defaults_and_new_fields_have_bounds() {
    let fixture = Fixture::new();
    fixture
        .db
        .upsert(fixture.upsert("key", Status::Todo))
        .unwrap();
    fixture
        .db
        .upsert(fixture.upsert("key-extra", Status::Done))
        .unwrap();
    let exact = fixture
        .db
        .list(ListTasks {
            task_key: Some("key".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(exact.items.len(), 1);
    assert_eq!(exact.items[0].task.task_key, "key");
    assert!(fixture
        .db
        .list(ListTasks {
            task_key: Some("key-extra".into()),
            ..Default::default()
        })
        .unwrap()
        .items
        .is_empty());
    assert_eq!(
        fixture
            .db
            .list(ListTasks {
                task_key: Some("key-extra".into()),
                include_done: true,
                ..Default::default()
            })
            .unwrap()
            .items
            .len(),
        1
    );
    let before = fixture.task("key");
    let revision = fixture.db.revision().unwrap();
    let mut invalid = fixture.upsert("key", Status::Todo);
    invalid.agent = Some("a".repeat(101));
    assert!(fixture.db.upsert(invalid).is_err());
    let mut invalid = fixture.upsert("key", Status::Todo);
    invalid.next_action = Some("步".repeat(601));
    assert!(fixture.db.upsert(invalid).is_err());
    let mut invalid = fixture.upsert("key", Status::Todo);
    invalid.needs_input = Some("line one\nline two".into());
    assert!(fixture.db.upsert(invalid).is_err());
    for deliverables in [
        vec![delivery()[0].clone(); 6],
        vec![Deliverable {
            label: "invalid".into(),
            uri: "javascript:alert(1)".into(),
        }],
        vec![Deliverable {
            label: "x".repeat(101),
            uri: "report.html".into(),
        }],
    ] {
        let mut invalid = fixture.upsert("key", Status::Todo);
        invalid.deliverables = Some(deliverables);
        assert!(fixture.db.upsert(invalid).is_err());
    }
    let mut invalid_capture = fixture.capture("too-large");
    invalid_capture.request = "需".repeat(2001);
    assert!(fixture.db.capture(invalid_capture).is_err());
    assert!(fixture
        .db
        .feedback(FeedbackTask {
            id: before.id,
            expected_updated_at: before.updated_at.clone(),
            note: "反".repeat(2001)
        })
        .is_err());
    assert_eq!(fixture.task("key"), before);
    assert_eq!(fixture.db.revision().unwrap(), revision);
}

fn step(title: &str, status: StepStatus) -> Step {
    Step {
        title: title.into(),
        status,
        note: String::new(),
    }
}

#[test]
fn v04_steps_are_a_preserved_patch_and_identical_plans_do_not_bump_updates() {
    let fixture = Fixture::new();
    let plan = vec![
        step("实现导出", StepStatus::InProgress),
        step("补测试", StepStatus::Todo),
    ];
    let created = fixture
        .db
        .upsert(UpsertTask {
            steps: Some(plan.clone()),
            ..fixture.upsert("feature:steps", Status::InProgress)
        })
        .unwrap();
    assert_eq!(fixture.task("feature:steps").steps, plan);
    // Omitted steps keep the plan; an unchanged resend is not new progress.
    let same = fixture
        .db
        .upsert(fixture.upsert("feature:steps", Status::InProgress))
        .unwrap();
    assert_eq!(same.updated_at, created.updated_at);
    let mut advanced = plan.clone();
    advanced[0].status = StepStatus::Done;
    advanced[0].note = "已完成 CSV 导出".into();
    let moved = fixture
        .db
        .upsert(UpsertTask {
            steps: Some(advanced.clone()),
            expected_updated_at: Some(created.updated_at.clone()),
            ..fixture.upsert("feature:steps", Status::InProgress)
        })
        .unwrap();
    assert_ne!(moved.updated_at, created.updated_at);
    assert_eq!(fixture.task("feature:steps").steps, advanced);
    fixture
        .db
        .upsert(UpsertTask {
            steps: Some(vec![]),
            ..fixture.upsert("feature:steps", Status::InProgress)
        })
        .unwrap();
    assert!(fixture.task("feature:steps").steps.is_empty());

    for invalid in [
        vec![step("", StepStatus::Todo)],
        vec![step("多行\n标题", StepStatus::Todo)],
        (0..13)
            .map(|i| step(&format!("步骤 {i}"), StepStatus::Todo))
            .collect(),
        vec![Step {
            note: "长".repeat(201),
            ..step("备注过长", StepStatus::Done)
        }],
    ] {
        assert!(matches!(
            fixture.db.upsert(UpsertTask {
                steps: Some(invalid),
                ..fixture.upsert("feature:steps", Status::InProgress)
            }),
            Err(Error::InvalidInput(_))
        ));
    }
    assert!(serde_json::from_str::<Step>(r#"{"title":"x","status":"started"}"#).is_err());
}

#[test]
fn v04_reopening_before_review_leaves_a_trace_until_the_next_completion() {
    let fixture = Fixture::new();
    fixture
        .db
        .upsert(fixture.upsert("feature:withdraw", Status::Done))
        .unwrap();
    assert_eq!(
        fixture.task("feature:withdraw").review_status,
        ReviewStatus::Pending
    );
    let reopened = fixture
        .db
        .upsert(fixture.upsert("feature:withdraw", Status::InProgress))
        .unwrap();
    let task = fixture.task("feature:withdraw");
    assert_eq!(task.review_status, ReviewStatus::None);
    assert_eq!(task.review_withdrawn_at, Some(reopened.updated_at));
    let mut later = fixture.upsert("feature:withdraw", Status::InProgress);
    later.progress = "继续修复".into();
    fixture.db.upsert(later).unwrap();
    assert!(fixture
        .task("feature:withdraw")
        .review_withdrawn_at
        .is_some());
    fixture
        .db
        .upsert(fixture.upsert("feature:withdraw", Status::Done))
        .unwrap();
    let task = fixture.task("feature:withdraw");
    assert_eq!(task.review_status, ReviewStatus::Pending);
    assert_eq!(task.review_withdrawn_at, None);
    // A task that never awaited review gets no trace.
    fixture
        .db
        .upsert(fixture.upsert("feature:plain", Status::Todo))
        .unwrap();
    fixture
        .db
        .upsert(fixture.upsert("feature:plain", Status::InProgress))
        .unwrap();
    assert_eq!(fixture.task("feature:plain").review_withdrawn_at, None);
}

#[test]
fn v04_gui_archive_is_versioned_and_not_agent_activity() {
    let fixture = Fixture::new();
    let receipt = fixture
        .db
        .upsert(fixture.upsert("feature:gui-archive", Status::InProgress))
        .unwrap();
    assert!(matches!(
        fixture.db.archive_by_id(ArchiveById {
            id: receipt.id,
            expected_updated_at: "2000-01-01T00:00:00.000Z".into(),
        }),
        Err(Error::Conflict)
    ));
    let archived = fixture
        .db
        .archive_by_id(ArchiveById {
            id: receipt.id,
            expected_updated_at: receipt.updated_at.clone(),
        })
        .unwrap();
    let task = fixture.task("feature:gui-archive");
    assert!(task.archived);
    assert_eq!(task.updated_at, archived.updated_at);
    assert_eq!(task.agent_updated_at, Some(receipt.updated_at));
    assert!(fixture.db.board().unwrap().projects.is_empty());
    // The Agent can still restore it through the MCP tool.
    fixture
        .db
        .archive(fixture.archive("feature:gui-archive", false, None))
        .unwrap();
    assert!(!fixture.task("feature:gui-archive").archived);
}
