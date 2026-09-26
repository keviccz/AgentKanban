//! One-time, local teaching examples. Opening a database through MCP never
//! calls this initializer; only the desktop's first launch opts into it.

use crate::{
    changed_at, resolve_project, Database, Error, ProjectIdentity, Result, ReviewStatus, Status,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use std::path::Path;

const INITIALIZED: &str = "tutorial_initialized_v1";
const PROJECT_IDENTITY: &str = "agentkanban:tutorial:v1";

struct TutorialTask {
    key: &'static str,
    title: &'static str,
    status: Status,
    review_status: ReviewStatus,
    progress: &'static str,
    request: &'static str,
    goal: &'static str,
    next_action: &'static str,
    acceptance: Value,
    steps: Value,
}

fn examples() -> [TutorialTask; 2] {
    [
        TutorialTask {
            key: "tutorial:follow-progress",
            title: "示例：跟进任务与补充要求",
            status: Status::InProgress,
            review_status: ReviewStatus::None,
            progress: "这是教学示例，没有 Agent 正在执行。点开详情查看步骤；顶部列表按钮可切换简洁视图。",
            request: "这是一条本地教学示例，不是真实项目任务。\n你可以查看目标、验收标准和步骤，在详情中补充自己的要求。准备开始实际工作时，请新建任务并选择你自己的项目目录；再复制那条真实任务的开工说明交给 Agent。新建和复制说明都不会自动启动 Agent。",
            goal: "认识任务进度、计划步骤和人工补充要求，并学会从自己的项目开始真实任务。",
            next_action: "先展开详情查看步骤，试着补充要求；随后新建真实任务，选择自己的项目目录并复制开工说明。体验结束可归档本示例。",
            acceptance: json!([
                "能在详情中区分已完成、进行中和待办步骤，并保存一条自己的补充要求。",
                "知道真实工作需要新建到自己的项目，复制开工说明后再交给已接入的 Agent。"
            ]),
            steps: json!([
                {"title":"认识任务卡片与目标","status":"done","note":"示例中的步骤状态用于演示，不表示 Agent 已执行工作。"},
                {"title":"打开详情，查看步骤并补充要求","status":"in_progress","note":"你可以把自己的补充写到人工意见中；再次保存会替换上一条意见。"},
                {"title":"新建真实任务并复制开工说明","status":"todo","note":"选择自己的项目目录；请勿把本教程目录交给 Agent 执行。"}
            ]),
        },
        TutorialTask {
            key: "tutorial:review-delivery",
            title: "示例：验收或退回一项交付",
            status: Status::Done,
            review_status: ReviewStatus::Pending,
            progress: "这是教学示例，没有真实交付物。通过验收后进入灰色已完成区；填写要求并退回后回到待办。",
            request: "用这条教学示例体验人工验收。\n打开详情，你可以通过验收，或填写一条修改要求后退回待办。这里的完成状态只用于演示，不表示 Agent 真实执行或产生了成果。验收和退回都会实际保存到这条示例记录；体验结束可归档它。",
            goal: "理解 Agent 报告完成与用户验收的区别，体验通过验收和退回修改。",
            next_action: "打开详情，阅读两条验收标准，然后选择通过验收，或写明修改要求并退回；不会启动 Agent。",
            acceptance: json!([
                "通过验收后，这条示例显示为已验收，并进入灰色已完成区。",
                "若选择退回，填写的要求会保留，原示例回到待办且不会另建任务。"
            ]),
            steps: json!([
                {"title":"阅读完成与验收的区别","status":"done","note":"教学状态，不是 Agent 的实际执行记录。"},
                {"title":"准备人工验收演示","status":"done","note":"你可以亲自操作通过或退回；没有真实成果链接。"}
            ]),
        },
    ]
}

impl Database {
    /// Keep the registered tutorial address usable by all ordinary MCP routes.
    /// The exception belongs to this database and exactly this canonical path;
    /// real project and Git worktree identification remains unchanged.
    pub(super) fn resolve_task_project(&self, value: &str) -> Result<ProjectIdentity> {
        let tutorial = self
            .connect()?
            .query_row(
                "SELECT identity,name,path FROM projects WHERE identity=?1",
                [PROJECT_IDENTITY],
                |row| {
                    Ok(ProjectIdentity {
                        identity: row.get(0)?,
                        name: row.get(1)?,
                        path: row.get(2)?,
                    })
                },
            )
            .optional()?;
        if let Some(tutorial) = tutorial {
            let requested = Path::new(value);
            if requested.is_absolute() && !value.chars().any(char::is_control) {
                if let (Ok(actual), Ok(expected)) = (
                    requested.canonicalize(),
                    Path::new(&tutorial.path).canonicalize(),
                ) {
                    #[cfg(windows)]
                    let matches = actual.to_string_lossy().to_lowercase()
                        == expected.to_string_lossy().to_lowercase();
                    #[cfg(not(windows))]
                    let matches = actual == expected;
                    if matches {
                        return Ok(tutorial);
                    }
                }
            }
        }
        resolve_project(value)
    }

    /// Seed two clearly marked examples only for a genuinely unused database.
    /// Returns true only for the call that inserted them. Existing users are
    /// also marked as checked, so later archiving/cleanup cannot resurrect them.
    pub fn initialize_tutorial(&self) -> Result<bool> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let checked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM settings WHERE key=?1)",
            [INITIALIZED],
            |row| row.get(0),
        )?;
        if checked {
            return Ok(false);
        }
        // Archived tasks, empty retained projects, prior preferences and an
        // already-used revision all identify an existing board, even if the UI
        // currently looks empty. Do not insert examples into that user's data.
        let existing: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks)
                 OR EXISTS(SELECT 1 FROM projects)
                 OR EXISTS(SELECT 1 FROM settings WHERE key!=?1)
                 OR EXISTS(SELECT 1 FROM metadata WHERE key='revision' AND value>0)",
            [crate::sync_health::SETTING_KEY],
            |row| row.get(0),
        )?;
        if existing {
            tx.execute(
                "INSERT INTO settings(key,value) VALUES (?1,'skipped')",
                [INITIALIZED],
            )?;
            tx.commit()?;
            return Ok(false);
        }

        let data_dir = self
            .path()
            .parent()
            .ok_or(Error::DataDirectoryUnavailable)?;
        let directory = data_dir.join("tutorial");
        std::fs::create_dir_all(&directory)?;
        let canonical = directory.canonicalize()?;
        let path = canonical
            .to_str()
            .ok_or_else(|| Error::ProjectIdentity("tutorial path is not valid Unicode".into()))?;
        #[cfg(windows)]
        let path = path
            .strip_prefix("\\\\?\\UNC\\")
            .map(|unc| format!("\\\\{unc}"))
            .unwrap_or_else(|| path.strip_prefix("\\\\?\\").unwrap_or(path).to_string());

        // This reserved identity keeps examples separate even when an isolated
        // data directory happens to be inside a real Git working tree.
        tx.execute(
            "INSERT INTO projects(identity,name,path) VALUES (?1,'新手教程',?2)",
            params![PROJECT_IDENTITY, path],
        )?;
        let project_id = tx.last_insert_rowid();
        let updated_at = changed_at(None);
        for task in examples() {
            tx.execute(
                "INSERT INTO tasks(project_id,task_key,title,status,progress,updated_at,
                    request,agent,next_action,review_status,steps,goal,acceptance)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,'教学示例',?8,?9,?10,?11,?12)",
                params![
                    project_id,
                    task.key,
                    task.title,
                    task.status.as_str(),
                    task.progress,
                    updated_at,
                    task.request,
                    task.next_action,
                    task.review_status.as_str(),
                    task.steps.to_string(),
                    task.goal,
                    task.acceptance.to_string(),
                ],
            )?;
        }
        // No Agent timestamps, reports, user feedback or deliverables are
        // fabricated. Both tasks and this marker commit as a single change.
        tx.execute(
            "INSERT INTO settings(key,value) VALUES (?1,'seeded')",
            [INITIALIZED],
        )?;
        tx.execute("UPDATE metadata SET value=value+1 WHERE key='revision'", [])?;
        tx.commit()?;
        Ok(true)
    }
}
