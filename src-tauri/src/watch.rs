//! Background duties of the running board, whether or not its window is visible:
//! a Windows notification when a task starts needing the user, and archiving
//! of old accepted work.

use crate::AppState;
use kanban_core::{BoardSnapshot, Status};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

const POLL: Duration = Duration::from_secs(2);
const ARCHIVE_EVERY: Duration = Duration::from_secs(3600);
/// More new items than this in one change become a single summary.
const SEPARATE_LIMIT: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    Blocked,
    Input,
    Review,
}

impl Need {
    fn label(self) -> &'static str {
        match self {
            Need::Blocked => "受阻",
            Need::Input => "需要你补充",
            Need::Review => "等你验收",
        }
    }
}

struct Alert {
    need: Need,
    title: String,
    detail: String,
}

/// Tasks that currently need the user, keyed by id. Mirrors `needsAttention` in display.ts.
fn needs(board: &BoardSnapshot) -> HashMap<i64, Alert> {
    let mut found = HashMap::new();
    for project in &board.projects {
        for task in &project.tasks {
            let need = match task.status {
                Status::Done if task.review_status == kanban_core::ReviewStatus::Pending => {
                    Need::Review
                }
                Status::Done => continue,
                Status::Blocked => Need::Blocked,
                _ if !task.needs_input.is_empty() => Need::Input,
                _ => continue,
            };
            let detail = if task.needs_input.is_empty() {
                &task.progress
            } else {
                &task.needs_input
            };
            found.insert(
                task.id,
                Alert {
                    need,
                    title: task.title.clone(),
                    detail: format!("{} · {detail}", project.name),
                },
            );
        }
    }
    found
}

pub(crate) fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut revision = None;
        let mut known: Option<HashMap<i64, Need>> = None;
        let mut archived_at: Option<Instant> = None;
        loop {
            let state = app.state::<AppState>();
            let preferences = match state.preferences.lock() {
                Ok(preferences) => preferences.clone(),
                Err(_) => return,
            };
            if preferences.auto_archive_days > 0
                && archived_at.is_none_or(|at| at.elapsed() >= ARCHIVE_EVERY)
            {
                archived_at = Some(Instant::now());
                let age = Duration::from_secs(u64::from(preferences.auto_archive_days) * 86_400);
                if let Err(err) = state.db.archive_finished(age) {
                    let _ = app.emit("app-error", format!("自动归档失败：{err}"));
                }
            }
            if let Ok(current) = state.db.revision() {
                if revision != Some(current) {
                    if let Ok(board) = state.db.board() {
                        revision = Some(current);
                        let alerts = needs(&board);
                        // The first read only learns the board; startup never replays old items.
                        if let (Some(previous), true) = (&known, preferences.notify) {
                            let fresh: Vec<&Alert> = alerts
                                .iter()
                                .filter(|(id, alert)| previous.get(id) != Some(&alert.need))
                                .map(|(_, alert)| alert)
                                .collect();
                            notify(&app, &fresh);
                        }
                        known = Some(alerts.iter().map(|(id, alert)| (*id, alert.need)).collect());
                    }
                }
            }
            std::thread::sleep(POLL);
        }
    });
}

fn notify(app: &AppHandle, fresh: &[&Alert]) {
    let show = |title: String, body: String| toast(app, &title, &body);
    if fresh.len() > SEPARATE_LIMIT {
        show(
            format!("{} 个任务等你处理", fresh.len()),
            fresh
                .iter()
                .map(|alert| alert.title.as_str())
                .collect::<Vec<_>>()
                .join("、"),
        );
        return;
    }
    for alert in fresh {
        show(
            format!("{}：{}", alert.need.label(), alert.title),
            alert.detail.clone(),
        );
    }
}

/// A reminder toast stays on screen until the user acts on it; clicking it or
/// "打开看板" brings the board forward.
#[cfg(windows)]
fn toast(app: &AppHandle, title: &str, body: &str) {
    use tauri_winrt_notification::{Scenario, Sound, Toast};
    let handle = app.clone();
    let shown = Toast::new(&app.config().identifier)
        .title(title)
        .text1(body)
        .scenario(Scenario::Reminder)
        .sound(Some(Sound::Default))
        .add_button("打开看板", "open")
        .add_button("稍后", "later")
        .on_activated(move |action| {
            if action.as_deref() != Some("later") {
                crate::show_window(&handle);
            }
            Ok(())
        })
        .show();
    if let Err(err) = shown {
        eprintln!("Notification failed: {err}");
    }
}

#[cfg(not(windows))]
fn toast(_app: &AppHandle, title: &str, body: &str) {
    eprintln!("{title}: {body}");
}
