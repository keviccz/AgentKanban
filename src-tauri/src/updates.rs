//! Desktop-owned update operations. A WebView may close without cancelling a download.

use chrono::{DateTime, SecondsFormat, Utc};
use kanban_core::Database;
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const ENDPOINT: &str =
    "https://github.com/keviccz/AgentKanban/releases/latest/download/latest.json";
const LAST_CHECK: &str = "updates_last_check";
const CHECK_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
const EVENT: &str = "update-status";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdatePhase {
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Installing,
    Blocked,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UpdateStatus {
    pub phase: UpdatePhase,
    pub current_version: String,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub checked_at: Option<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub message: String,
}

struct UpdateSession {
    status: UpdateStatus,
    release: Option<Update>,
    // Only download().await's successful, signature-verified result may enter this cache.
    // No unverified/partial package is persisted, and failure drops the previous bytes.
    verified_bytes: Option<Arc<[u8]>>,
}

impl UpdateSession {
    fn new(checked_at: Option<String>) -> Self {
        Self {
            status: UpdateStatus {
                phase: UpdatePhase::Idle,
                current_version: env!("CARGO_PKG_VERSION").into(),
                version: None,
                notes: None,
                checked_at,
                downloaded_bytes: 0,
                total_bytes: None,
                message: "可手动检查 GitHub Releases 上的新版本".into(),
            },
            release: None,
            verified_bytes: None,
        }
    }

    fn operation_in_progress(&self) -> bool {
        matches!(
            self.status.phase,
            UpdatePhase::Checking | UpdatePhase::Downloading | UpdatePhase::Installing
        )
    }

    fn can_check(&self) -> bool {
        !self.operation_in_progress() && self.verified_bytes.is_none()
    }

    fn begin_check(&mut self, checked_at: String) {
        self.status.phase = UpdatePhase::Checking;
        self.status.checked_at = Some(checked_at);
        self.status.version = None;
        self.status.notes = None;
        self.status.downloaded_bytes = 0;
        self.status.total_bytes = None;
        self.status.message = "正在检查新版本…".into();
        self.release = None;
    }

    fn download_progress(&mut self, bytes: usize, total: Option<u64>) {
        self.status.downloaded_bytes = self.status.downloaded_bytes.saturating_add(bytes as u64);
        self.status.total_bytes = total.filter(|size| *size > 0);
    }

    fn download_finished(&mut self, result: Result<Vec<u8>, String>) {
        self.verified_bytes = None;
        match result {
            Ok(bytes) if !bytes.is_empty() => {
                self.status.phase = UpdatePhase::Ready;
                self.status.downloaded_bytes = bytes.len() as u64;
                self.status.message = "下载及签名验证完成；安装前会检查 MCP 占用并备份数据".into();
                self.verified_bytes = Some(bytes.into());
            }
            Ok(_) => self.fail_download("更新包为空，请重试下载".into()),
            Err(message) => self.fail_download(message),
        }
    }

    fn fail_download(&mut self, message: String) {
        self.verified_bytes = None;
        self.status.phase = UpdatePhase::Error;
        self.status.downloaded_bytes = 0;
        self.status.total_bytes = None;
        self.status.message = message;
    }
}

#[derive(Clone)]
pub(crate) struct UpdateState {
    db: Database,
    session: Arc<Mutex<UpdateSession>>,
}

impl UpdateState {
    pub(crate) fn new(db: Database) -> Self {
        let checked_at = db
            .get_setting(LAST_CHECK)
            .ok()
            .flatten()
            .filter(|value| DateTime::parse_from_rfc3339(value).is_ok());
        Self {
            db,
            session: Arc::new(Mutex::new(UpdateSession::new(checked_at))),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, UpdateSession>, String> {
        self.session.lock().map_err(|_| "更新状态暂时不可用".into())
    }

    fn status(&self) -> Result<UpdateStatus, String> {
        Ok(self.lock()?.status.clone())
    }
}

fn publish(
    app: &AppHandle,
    state: &UpdateState,
    change: impl FnOnce(&mut UpdateSession),
) -> Result<UpdateStatus, String> {
    let status = {
        let mut session = state.lock()?;
        change(&mut session);
        session.status.clone()
    };
    // No window/listener is also a valid state (for example, starting in the tray).
    let _ = app.emit(EVENT, &status);
    Ok(status)
}

fn due_for_check(last_check: Option<&str>, now: DateTime<Utc>) -> bool {
    let Some(previous) = last_check.and_then(|value| DateTime::parse_from_rfc3339(value).ok())
    else {
        return true;
    };
    let elapsed = now.signed_duration_since(previous).num_seconds();
    // A corrected clock must not suppress updates indefinitely.
    !(0..CHECK_INTERVAL_SECONDS).contains(&elapsed)
}

fn automatic_preferences(app: &AppHandle) -> Result<(bool, bool), String> {
    let state = app.state::<crate::AppState>();
    let prefs = state.preferences.lock().map_err(|_| "无法读取更新设置")?;
    Ok((prefs.auto_check_updates, prefs.auto_download_updates))
}

#[tauri::command]
pub(crate) fn get_update_status(state: State<'_, UpdateState>) -> Result<UpdateStatus, String> {
    state.status()
}

#[tauri::command(async)]
pub(crate) fn check_updates(app: AppHandle, manual: bool) -> Result<UpdateStatus, String> {
    start_check(&app, manual)
}

fn start_check(app: &AppHandle, manual: bool) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>().inner().clone();
    let status = {
        let mut session = state.lock()?;
        // Do not replace the Update metadata while its matching bytes are downloading,
        // verified, blocked on an MCP session, or being installed.
        if !session.can_check() || (!manual && !automatic_preferences(app)?.0) {
            return Ok(session.status.clone());
        }
        let now = Utc::now();
        let last_check = state
            .db
            .get_setting(LAST_CHECK)
            .map_err(|err| err.to_string())?;
        if !manual && !due_for_check(last_check.as_deref(), now) {
            return Ok(session.status.clone());
        }
        let checked_at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        if let Err(err) = state.db.set_setting(LAST_CHECK, &checked_at) {
            session.status.phase = UpdatePhase::Error;
            session.status.message = format!("无法保存更新检查时间：{err}");
            let status = session.status.clone();
            drop(session);
            let _ = app.emit(EVENT, &status);
            return Ok(status);
        }
        session.begin_check(checked_at);
        session.status.clone()
    };
    let _ = app.emit(EVENT, &status);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let outcome = check_release(&handle).await;
        let available = matches!(&outcome, Ok(CheckResult::Available(_)));
        let _ = publish(&handle, &state, |session| match outcome {
            Ok(CheckResult::Available(mut update)) => {
                // The plugin has no download timeout by default. A failed request is retryable.
                update.timeout = Some(Duration::from_secs(30 * 60));
                session.status.phase = UpdatePhase::Available;
                session.status.version = Some(update.version.clone());
                session.status.notes = update.body.clone();
                session.status.message = format!("发现新版本 {}，可以开始下载", update.version);
                session.release = Some(*update);
            }
            Ok(CheckResult::Current) => {
                session.status.phase = UpdatePhase::Idle;
                session.status.message = "当前已是最新版本".into();
            }
            Ok(CheckResult::Unpublished) => {
                session.status.phase = UpdatePhase::Idle;
                session.status.message = "GitHub Releases 尚未发布可供自动更新的版本".into();
            }
            Err(message) => {
                session.status.phase = UpdatePhase::Error;
                session.status.message = format!("检查更新失败：{message}；可以稍后重试");
            }
        });
        // Read again after the request so turning auto-download off takes effect immediately.
        if available && automatic_preferences(&handle).is_ok_and(|prefs| prefs.1) {
            let _ = start_download(&handle);
        }
    });
    Ok(status)
}

enum CheckResult {
    Available(Box<Update>),
    Current,
    Unpublished,
}

async fn check_release(app: &AppHandle) -> Result<CheckResult, String> {
    let updater = app
        .updater_builder()
        .endpoints(vec![ENDPOINT
            .parse()
            .map_err(|err| format!("更新地址无效：{err}"))?])
        .map_err(|err| err.to_string())?
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|err| err.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(CheckResult::Available(Box::new(update))),
        Ok(None) => Ok(CheckResult::Current),
        Err(tauri_plugin_updater::Error::ReleaseNotFound) => {
            // The plugin discards HTTP status for non-2xx metadata responses. Diagnose only
            // this fixed, small endpoint; never classify a generic network/JSON error as 404.
            let response = reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .map_err(|err| err.to_string())?
                .head(ENDPOINT)
                .send()
                .await
                .map_err(|err| err.to_string())?;
            classify_missing_release(response.status().as_u16())
        }
        Err(err) => Err(err.to_string()),
    }
}

fn classify_missing_release(status: u16) -> Result<CheckResult, String> {
    if status == 404 {
        Ok(CheckResult::Unpublished)
    } else {
        Err(format!("发布信息不可用（HTTP {status}），请稍后重新检查"))
    }
}

#[tauri::command]
pub(crate) fn download_update(app: AppHandle) -> Result<UpdateStatus, String> {
    start_download(&app)
}

fn start_download(app: &AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>().inner().clone();
    let (update, status) = {
        let mut session = state.lock()?;
        if session.operation_in_progress() || session.verified_bytes.is_some() {
            return Ok(session.status.clone());
        }
        let Some(update) = session.release.clone() else {
            return Ok(session.status.clone());
        };
        session.status.phase = UpdatePhase::Downloading;
        session.status.downloaded_bytes = 0;
        session.status.total_bytes = None;
        session.status.message = "正在后台下载；可以关闭此设置面板".into();
        (update, session.status.clone())
    };
    let _ = app.emit(EVENT, &status);
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut last_event = Instant::now();
        let downloaded = update
            .download(
                |length, total| {
                    if let Ok(mut session) = state.lock() {
                        session.download_progress(length, total);
                        if last_event.elapsed() >= Duration::from_millis(200) {
                            let status = session.status.clone();
                            drop(session);
                            let _ = handle.emit(EVENT, &status);
                            last_event = Instant::now();
                        }
                    }
                },
                || {
                    // This callback runs BEFORE the plugin verifies the signature.
                    let _ = publish(&handle, &state, |session| {
                        session.status.message = "下载完成，正在验证更新签名…".into();
                    });
                },
            )
            .await
            .map_err(|err| format!("下载或签名验证失败：{err}；缓存已清除，可重试下载"));
        let _ = publish(&handle, &state, |session| {
            session.download_finished(downloaded)
        });
    });
    Ok(status)
}

#[tauri::command]
pub(crate) fn install_update(app: AppHandle) -> Result<UpdateStatus, String> {
    let state = app.state::<UpdateState>().inner().clone();
    let (update, bytes, status) = {
        let mut session = state.lock()?;
        if session.operation_in_progress() {
            return Ok(session.status.clone());
        }
        let (Some(update), Some(bytes)) = (session.release.clone(), session.verified_bytes.clone())
        else {
            return Err("请先完成更新下载和签名验证".into());
        };
        session.status.phase = UpdatePhase::Installing;
        session.status.message = "正在检查 MCP 占用并备份数据库…".into();
        (update, bytes, session.status.clone())
    };
    let _ = app.emit(EVENT, &status);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = guarded_install(
            running_installed_mcp,
            || {
                let directory = state
                    .db
                    .path()
                    .parent()
                    .ok_or("无法定位数据库目录")?
                    .join("backups");
                std::fs::create_dir_all(&directory)
                    .map_err(|err| format!("无法创建备份目录：{err}"))?;
                crate::write_backup(&state.db, &directory)
            },
            |backup| {
                publish(&handle, &state, |session| {
                    session.status.message =
                        format!("数据库已备份到 {}；正在启动安装程序…", backup.display());
                })?;
                // Windows exits this process only once the official installer is launched.
                update
                    .install(bytes.as_ref())
                    .map_err(|err| err.to_string())
            },
        );
        let _ = publish(&handle, &state, |session| match result {
            Ok(backup) => {
                session.status.message = format!(
                    "安装程序已启动；数据库备份：{}。重新打开后查看实际版本。",
                    backup.display()
                );
            }
            Err(InstallFailure::Blocked(message)) => {
                session.status.phase = UpdatePhase::Blocked;
                session.status.message = message;
            }
            Err(InstallFailure::Failed(message)) => {
                session.fail_download(message);
            }
        });
    });
    Ok(status)
}

#[derive(Debug, PartialEq, Eq)]
enum InstallFailure {
    Blocked(String),
    Failed(String),
}

fn guarded_install(
    mut running_mcp: impl FnMut() -> Result<Vec<u32>, String>,
    backup: impl FnOnce() -> Result<PathBuf, String>,
    install: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, InstallFailure> {
    let check = |pids: Vec<u32>| {
        if pids.is_empty() {
            Ok(())
        } else {
            Err(InstallFailure::Blocked(format!(
                "此安装目录的 MCP 仍被客户端使用（进程 {}）。请结束相关 Agent 会话或关闭客户端后重试；已下载内容会保留。",
                pids.iter().map(u32::to_string).collect::<Vec<_>>().join("、")
            )))
        }
    };
    check(running_mcp().map_err(InstallFailure::Blocked)?)?;
    let backup_path = backup().map_err(|err| {
        InstallFailure::Blocked(format!("安装前备份失败，尚未启动安装：{err}；可以重试安装"))
    })?;
    // A client could have connected while SQLite was creating the backup.
    if let Err(err) = running_mcp()
        .map_err(InstallFailure::Blocked)
        .and_then(check)
    {
        let InstallFailure::Blocked(message) = err else {
            unreachable!()
        };
        return Err(InstallFailure::Blocked(format!(
            "{message} 备份：{}",
            backup_path.display()
        )));
    }
    install(&backup_path).map_err(|err| {
        InstallFailure::Failed(format!(
            "启动安装失败：{err}；可重新下载后重试。数据库备份：{}",
            backup_path.display()
        ))
    })?;
    Ok(backup_path)
}

/// Native scheduling also works while the board stays hidden in the tray.
pub(crate) fn spawn_auto_check(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            // The DB timestamp enforces 24 hours across application restarts as well.
            if let Err(err) = start_check(&app, false) {
                eprintln!("Automatic update check was not started: {err}");
            }
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
}

#[cfg(windows)]
fn normalized_windows_path(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else {
        text.strip_prefix("\\\\?\\").unwrap_or(&text).to_owned()
    };
    text.to_lowercase()
}

#[cfg(windows)]
fn same_executable_path(expected: &Path, candidate: &Path) -> bool {
    normalized_windows_path(expected) == normalized_windows_path(candidate)
}

#[cfg(windows)]
fn running_installed_mcp() -> Result<Vec<u32>, String> {
    use std::{mem::size_of, os::windows::ffi::OsStringExt};
    use windows_sys::Win32::{
        Foundation::{
            CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE,
            INVALID_HANDLE_VALUE,
        },
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    struct OwnedHandle(HANDLE);
    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let executable = std::env::current_exe().map_err(|err| format!("无法定位安装目录：{err}"))?;
    let directory = executable.parent().ok_or("无法定位安装目录")?;
    let directory =
        std::fs::canonicalize(directory).map_err(|err| format!("无法读取安装目录：{err}"))?;
    let expected = directory.join("agentkanban-mcp.exe");
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(format!(
            "无法检查 MCP 占用：{}",
            std::io::Error::last_os_error()
        ));
    }
    let snapshot = OwnedHandle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut has_entry = unsafe { Process32FirstW(snapshot.0, &mut entry) };
    let mut matches = Vec::new();
    loop {
        if has_entry == 0 {
            if unsafe { GetLastError() } == ERROR_NO_MORE_FILES {
                return Ok(matches);
            }
            return Err(format!(
                "无法完成 MCP 占用检查：{}",
                std::io::Error::last_os_error()
            ));
        }
        let length = entry
            .szExeFile
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(entry.szExeFile.len());
        let name = String::from_utf16_lossy(&entry.szExeFile[..length]);
        if name.eq_ignore_ascii_case("agentkanban-mcp.exe") {
            let handle =
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID) };
            if handle.is_null() {
                if unsafe { GetLastError() } != ERROR_INVALID_PARAMETER {
                    return Err(format!(
                        "无法确认 MCP 进程 {} 的安装位置，请关闭该会话后重试：{}",
                        entry.th32ProcessID,
                        std::io::Error::last_os_error()
                    ));
                }
            } else {
                let handle = OwnedHandle(handle);
                let mut path = vec![0u16; 32_768];
                let mut length = path.len() as u32;
                if unsafe {
                    QueryFullProcessImageNameW(handle.0, 0, path.as_mut_ptr(), &mut length)
                } == 0
                {
                    if unsafe { GetLastError() } != ERROR_INVALID_PARAMETER {
                        return Err(format!(
                            "无法确认 MCP 进程 {} 的安装位置：{}",
                            entry.th32ProcessID,
                            std::io::Error::last_os_error()
                        ));
                    }
                } else {
                    let path =
                        PathBuf::from(std::ffi::OsString::from_wide(&path[..length as usize]));
                    let path = std::fs::canonicalize(&path).unwrap_or(path);
                    if same_executable_path(&expected, &path) {
                        matches.push(entry.th32ProcessID);
                    }
                }
            }
        }
        has_entry = unsafe { Process32NextW(snapshot.0, &mut entry) };
    }
}

#[cfg(not(windows))]
fn running_installed_mcp() -> Result<Vec<u32>, String> {
    Err("此版本仅支持 Windows 安全安装；请使用发布页中的安装包".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn automatic_checks_are_throttled_across_restarts_but_bad_or_future_times_are_not() {
        let now = DateTime::parse_from_rfc3339("2026-09-26T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!due_for_check(Some("2026-09-26T11:59:59Z"), now));
        assert!(!due_for_check(Some("2026-09-25T12:00:01Z"), now));
        assert!(due_for_check(Some("2026-09-25T12:00:00Z"), now));
        assert!(due_for_check(Some("2026-09-27T12:00:00Z"), now));
        assert!(due_for_check(Some("invalid"), now));
        assert!(due_for_check(None, now));
    }

    #[test]
    fn checking_cannot_replace_an_operation_or_verified_package() {
        let mut session = UpdateSession::new(None);
        for phase in [
            UpdatePhase::Checking,
            UpdatePhase::Downloading,
            UpdatePhase::Installing,
        ] {
            session.status.phase = phase;
            assert!(!session.can_check());
        }
        session.status.phase = UpdatePhase::Available;
        assert!(session.can_check());
        session.download_finished(Ok(vec![1, 2, 3]));
        assert!(!session.can_check());
        session.status.phase = UpdatePhase::Blocked;
        assert!(!session.can_check());
        assert_eq!(session.verified_bytes.as_deref(), Some(&[1, 2, 3][..]));
    }

    #[test]
    fn download_finish_is_not_ready_until_signature_verification_returns_success() {
        let mut session = UpdateSession::new(None);
        session.status.phase = UpdatePhase::Downloading;
        session.download_progress(3, None);
        session.status.message = "下载完成，正在验证更新签名…".into();
        assert_eq!(session.status.phase, UpdatePhase::Downloading);
        assert!(session.verified_bytes.is_none());
        assert_eq!(session.status.total_bytes, None);
        session.download_finished(Err("bad signature".into()));
        assert_eq!(session.status.phase, UpdatePhase::Error);
        assert!(session.verified_bytes.is_none());
        assert_eq!(session.status.downloaded_bytes, 0);
        session.download_finished(Ok(vec![1, 2, 3]));
        assert_eq!(session.status.phase, UpdatePhase::Ready);
        assert_eq!(session.status.downloaded_bytes, 3);
        session.download_finished(Err("retry failed".into()));
        assert!(session.verified_bytes.is_none());
        session.download_finished(Ok(Vec::new()));
        assert_eq!(session.status.phase, UpdatePhase::Error);
        assert!(session.verified_bytes.is_none());
    }

    #[test]
    fn official_plugin_verifies_download_before_cache_and_rejects_tampered_bytes() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        // Public test vector from minisign-verify 0.2.5's documentation. There is no
        // private key here, and no installer is ever called by this loopback test.
        const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkKUldRZjZMUkNHQTlpNTNtbFllY080SXpUNTFUR1Bwdld1Y05TQ2gxQ0JNMFFUYUxuNzNZN0dGTzMK";
        const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIG1pbmlzaWduIHNlY3JldCBrZXkKUlVRZjZMUkNHQTlpNTU5cjNnN1YxcU55SkRBcEdpcDhNZnFjYWRJZ1Q5Q3VoVjNFTWhIb04xbUdUa1VpZEYvejdTcmxRZ1hkeThvZmpiN2JOSkp5bERPb2NyQ284S0x6WndvPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNjMzNzAwODM1CWZpbGU6dGVzdAlwcmVoYXNoZWQKd0xNRGp5OUZMQXV4WjNxNE5sRXZrZ3R5aHJyMGd0VHU2S0M0S0JKZElUYmJPZUFpMXpCSVlvMHY0aVRndDhqSnBJaWRSSm5wOTRBQlFrSkFnQW9vQlE9PQ==";
        for (payload, should_verify) in
            [(b"test".as_slice(), true), (b"tampered".as_slice(), false)]
        {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let metadata = serde_json::json!({
                "version": "99.0.0",
                "platforms": { "windows-x86_64": {
                    "url": format!("http://{address}/package"), "signature": SIGNATURE
                }}
            })
            .to_string();
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(10);
                for body in [metadata.as_bytes(), payload] {
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                                assert!(
                                    Instant::now() < deadline,
                                    "loopback updater request timed out"
                                );
                                std::thread::sleep(Duration::from_millis(10));
                            }
                            Err(err) => panic!("loopback accept failed: {err}"),
                        }
                    };
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        let mut byte = [0];
                        stream.read_exact(&mut byte).unwrap();
                        request.extend(byte);
                    }
                    // No Content-Length exercises the unknown-total download UI contract.
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                        .unwrap();
                    stream.write_all(body).unwrap();
                }
            });
            let mut context = tauri::test::mock_context(tauri::test::noop_assets());
            context.config_mut().plugins.0.insert(
                "updater".into(),
                serde_json::json!({
                    "pubkey": PUBLIC_KEY,
                    "endpoints": [format!("http://{address}/latest.json")],
                    "dangerousInsecureTransportProtocol": true
                }),
            );
            let app = tauri::test::mock_builder()
                .plugin(tauri_plugin_updater::Builder::new().build())
                .build(context)
                .unwrap();
            let updater = app
                .updater_builder()
                .target("windows-x86_64")
                .no_proxy()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            let session = RefCell::new(UpdateSession::new(None));
            session.borrow_mut().status.phase = UpdatePhase::Downloading;
            let finished = Cell::new(false);
            let downloaded = tauri::async_runtime::block_on(async {
                let update = updater.check().await.unwrap().unwrap();
                update
                    .download(
                        |length, total| session.borrow_mut().download_progress(length, total),
                        || {
                            assert!(session.borrow().verified_bytes.is_none());
                            assert_eq!(session.borrow().status.phase, UpdatePhase::Downloading);
                            finished.set(true);
                        },
                    )
                    .await
            });
            server.join().unwrap();
            assert!(
                finished.get(),
                "the plugin finishes transfer before verifying either payload"
            );
            assert_eq!(downloaded.is_ok(), should_verify);
            session
                .borrow_mut()
                .download_finished(downloaded.map_err(|err| err.to_string()));
            let session = session.borrow();
            assert_eq!(session.verified_bytes.is_some(), should_verify);
            assert_eq!(
                session.status.phase,
                if should_verify {
                    UpdatePhase::Ready
                } else {
                    UpdatePhase::Error
                }
            );
            assert_eq!(session.status.total_bytes, None);
        }
    }

    #[test]
    fn only_a_confirmed_404_is_treated_as_unpublished() {
        assert!(matches!(
            classify_missing_release(404),
            Ok(CheckResult::Unpublished)
        ));
        for status in [200, 204, 301, 401, 403, 429, 500, 503] {
            assert!(classify_missing_release(status).is_err());
        }
    }

    #[test]
    fn busy_or_unknown_mcp_never_backs_up_or_installs() {
        for check in [Ok(vec![42]), Err("cannot inspect process".into())] {
            let backup_called = Cell::new(false);
            let install_called = Cell::new(false);
            let result = guarded_install(
                || check.clone(),
                || {
                    backup_called.set(true);
                    Ok(PathBuf::from("unused"))
                },
                |_| {
                    install_called.set(true);
                    Ok(())
                },
            );
            assert!(matches!(result, Err(InstallFailure::Blocked(_))));
            assert!(!backup_called.get());
            assert!(!install_called.get());
        }
    }

    #[test]
    fn backup_failure_never_runs_the_installer() {
        let install_called = Cell::new(false);
        let result = guarded_install(
            || Ok(vec![]),
            || Err("disk full".into()),
            |_| {
                install_called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(InstallFailure::Blocked(message)) if message.contains("disk full"))
        );
        assert!(!install_called.get());
    }

    #[test]
    fn a_client_connecting_during_backup_blocks_installation() {
        let checks = Cell::new(0);
        let install_called = Cell::new(false);
        let result = guarded_install(
            || {
                checks.set(checks.get() + 1);
                Ok(if checks.get() == 1 { vec![] } else { vec![42] })
            },
            || Ok(PathBuf::from("saved.sqlite3")),
            |_| {
                install_called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(InstallFailure::Blocked(message)) if message.contains("saved.sqlite3"))
        );
        assert!(!install_called.get());
    }

    #[test]
    fn installer_runs_only_after_both_checks_and_successful_backup() {
        let calls = RefCell::new(Vec::new());
        let result = guarded_install(
            || {
                calls.borrow_mut().push("check");
                Ok(vec![])
            },
            || {
                calls.borrow_mut().push("backup");
                Ok(PathBuf::from("saved.sqlite3"))
            },
            |path| {
                assert_eq!(path, Path::new("saved.sqlite3"));
                calls.borrow_mut().push("install");
                Ok(())
            },
        );
        assert_eq!(result, Ok(PathBuf::from("saved.sqlite3")));
        assert_eq!(*calls.borrow(), ["check", "backup", "check", "install"]);
    }

    #[cfg(windows)]
    #[test]
    fn process_paths_match_the_exact_installation_not_sibling_prefixes() {
        let expected = Path::new(r"C:\Apps\AgentKanban\agentkanban-mcp.exe");
        for path in [
            r"c:\apps\AGENTKANBAN\agentkanban-mcp.exe",
            r"\\?\C:\Apps\AgentKanban\agentkanban-mcp.exe",
            "C:/Apps/AgentKanban/agentkanban-mcp.exe",
        ] {
            assert!(same_executable_path(expected, Path::new(path)));
        }
        for path in [
            r"C:\Apps\AgentKanban-test\agentkanban-mcp.exe",
            r"C:\Apps\AgentKanban\subdir\agentkanban-mcp.exe",
            r"C:\Other\agentkanban-mcp.exe",
            r"C:\Apps\AgentKanban\agentkanban-mcp-helper.exe",
        ] {
            assert!(!same_executable_path(expected, Path::new(path)));
        }
        assert!(same_executable_path(
            Path::new(r"\\server\share\agentkanban-mcp.exe"),
            Path::new(r"\\?\UNC\server\share\agentkanban-mcp.exe")
        ));
    }
}
