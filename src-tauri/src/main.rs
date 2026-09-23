#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod integration;
mod preferences;
mod task_actions;

use kanban_core::{
    ArchiveById, BoardSnapshot, CaptureTask, Database, FeedbackTask, ReviewTask, TaskReceipt,
};
use preferences::Preferences;
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, State, WebviewWindow,
};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, ShortcutState};
use tauri_plugin_opener::OpenerExt;

const SHORTCUT: &str = "Ctrl+Alt+K";
const CREATE_SHORTCUT: &str = "Ctrl+Alt+N";
const SHORTCUTS: [&str; 2] = [SHORTCUT, CREATE_SHORTCUT];

#[derive(Default)]
struct DesktopErrors {
    autostart: Option<String>,
    shortcut: Option<String>,
}

#[derive(Serialize)]
struct DesktopSettings {
    autostart_enabled: bool,
    shortcut_enabled: bool,
    shortcut: &'static str,
    create_shortcut: &'static str,
    autostart_error: Option<String>,
    shortcut_error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Geometry {
    x: Option<i32>,
    y: Option<i32>,
    width: f64,
    height: f64,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            x: None,
            y: None,
            width: 380.0,
            height: 520.0,
        }
    }
}

struct AppState {
    db: Database,
    preferences: Mutex<Preferences>,
    preferences_save: Mutex<()>,
    desktop_errors: Mutex<DesktopErrors>,
    geometry: Mutex<Geometry>,
    geometry_save: Mutex<()>,
    geometry_dirty: AtomicBool,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn save_geometry(state: &AppState) -> Result<(), String> {
    let _writer = state.geometry_save.lock().map_err(error)?;
    if state.geometry_dirty.swap(false, Ordering::SeqCst) {
        let geometry = state.geometry.lock().map_err(error)?.clone();
        if let Err(err) = state.db.set_setting(
            "geometry",
            &serde_json::to_string(&geometry).map_err(error)?,
        ) {
            state.geometry_dirty.store(true, Ordering::SeqCst);
            return Err(error(err));
        }
    }
    Ok(())
}

fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("visibility-changed", true);
    }
}

fn toggle_window(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let result = (|| -> Result<(), String> {
        if window.is_visible().map_err(error)? && !window.is_minimized().map_err(error)? {
            hide_window(window, app.state())?;
        } else {
            show_window(app);
        }
        Ok(())
    })();
    if let Err(err) = result {
        let _ = app.emit("app-error", format!("快捷键切换窗口失败：{err}"));
    }
}

fn show_quick_create(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let result = (|| -> Result<(), String> {
        let preferences = set_compact(window.clone(), app.state(), false)?;
        window.unminimize().map_err(error)?;
        window.show().map_err(error)?;
        window.set_focus().map_err(error)?;
        window.emit("visibility-changed", true).map_err(error)?;
        // The frontend uses this canonical state before displaying its capture form.
        window.emit("quick-create", preferences).map_err(error)
    })();
    if let Err(err) = result {
        let _ = app.emit("app-error", format!("打开新建任务失败：{err}"));
    }
}

#[tauri::command(async)]
fn get_snapshot(state: State<AppState>) -> Result<BoardSnapshot, String> {
    state.db.board().map_err(error)
}

#[tauri::command(async)]
fn get_revision(state: State<AppState>) -> Result<i64, String> {
    state.db.revision().map_err(error)
}

#[tauri::command(async)]
fn create_task(state: State<AppState>, input: CaptureTask) -> Result<TaskReceipt, String> {
    state.db.capture(input).map_err(error)
}

#[tauri::command(async)]
fn review_task(state: State<AppState>, input: ReviewTask) -> Result<TaskReceipt, String> {
    state.db.review(input).map_err(error)
}

#[tauri::command(async)]
fn send_task_feedback(state: State<AppState>, input: FeedbackTask) -> Result<TaskReceipt, String> {
    state.db.feedback(input).map_err(error)
}

#[tauri::command(async)]
fn archive_task(state: State<AppState>, input: ArchiveById) -> Result<TaskReceipt, String> {
    state.db.archive_by_id(input).map_err(error)
}

#[tauri::command(async)]
fn get_handoff(state: State<AppState>, id: i64) -> Result<String, String> {
    if id <= 0 {
        return Err("任务标识无效".into());
    }
    let board = state.db.board().map_err(error)?;
    for project in board.projects {
        if let Some(task) = project.tasks.iter().find(|task| task.id == id) {
            return task_actions::handoff(&project.path, task);
        }
    }
    Err("任务不存在或已归档，请刷新看板后重试".into())
}

#[tauri::command]
fn open_external_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let url = task_actions::external_url(&url)?;
    // Only this explicit frontend action can open a URL. No file or shell fallback.
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|err| format!("打开网页失败：{err}"))
}

#[tauri::command]
fn get_preferences(state: State<AppState>) -> Result<Preferences, String> {
    Ok(state.preferences.lock().map_err(error)?.clone())
}

#[tauri::command(async)]
fn set_preferences(
    window: WebviewWindow,
    state: State<AppState>,
    preferences: Preferences,
) -> Result<Preferences, String> {
    let _writer = state.preferences_save.lock().map_err(error)?;
    preferences.validate()?;
    // Native-owned fields must not be overwritten by an older UI snapshot.
    let previous = state.preferences.lock().map_err(error)?.clone();
    let next = Preferences {
        compact: previous.compact,
        shortcut_enabled: previous.shortcut_enabled,
        ..preferences
    };
    window
        .set_always_on_top(next.always_on_top)
        .map_err(error)?;
    if let Err(err) = state
        .db
        .set_setting("ui", &serde_json::to_string(&next).map_err(error)?)
    {
        let _ = window.set_always_on_top(previous.always_on_top);
        return Err(error(err));
    }
    *state.preferences.lock().map_err(error)? = next.clone();
    Ok(next)
}

#[tauri::command]
fn set_compact(
    window: WebviewWindow,
    state: State<AppState>,
    compact: bool,
) -> Result<Preferences, String> {
    let _writer = state.preferences_save.lock().map_err(error)?;
    let previous = state.preferences.lock().map_err(error)?.clone();
    if previous.compact == compact {
        return Ok(previous);
    }
    let geometry = state.geometry.lock().map_err(error)?.clone();
    let width = geometry.width.max(320.0);
    let height = if compact {
        48.0
    } else {
        geometry.height.max(360.0)
    };
    // Update before the resize event so compact height cannot replace expanded height.
    // Never hold the mutex over a native setter: Windows may synchronously emit Resized.
    let next = Preferences {
        compact,
        ..previous.clone()
    };
    *state.preferences.lock().map_err(error)? = next.clone();
    let apply = || -> Result<(), String> {
        window.set_resizable(!compact).map_err(error)?;
        window
            .set_min_size(Some(tauri::LogicalSize::new(
                320.0,
                if compact { 48.0 } else { 360.0 },
            )))
            .map_err(error)?;
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(error)?;
        state
            .db
            .set_setting("ui", &serde_json::to_string(&next).map_err(error)?)
            .map_err(error)
    };
    if let Err(err) = apply() {
        *state.preferences.lock().map_err(error)? = previous.clone();
        let _ = window.set_resizable(!previous.compact);
        let _ = window.set_min_size(Some(tauri::LogicalSize::new(
            320.0,
            if previous.compact { 48.0 } else { 360.0 },
        )));
        let _ = window.set_size(tauri::LogicalSize::new(
            width,
            if previous.compact {
                48.0
            } else {
                geometry.height.max(360.0)
            },
        ));
        return Err(err);
    }
    Ok(next)
}

#[tauri::command(async)]
fn hide_window(window: WebviewWindow, state: State<AppState>) -> Result<(), String> {
    save_geometry(&state)?;
    window.emit("visibility-changed", false).map_err(error)?;
    window.hide().map_err(error)
}

fn shortcut_states(app: &tauri::AppHandle) -> [bool; 2] {
    SHORTCUTS.map(|shortcut| app.global_shortcut().is_registered(shortcut))
}

fn restore_shortcut_states(app: &tauri::AppHandle, desired: [bool; 2]) -> Result<(), String> {
    let manager = app.global_shortcut();
    let mut errors = Vec::new();
    for (shortcut, enabled) in SHORTCUTS.into_iter().zip(desired) {
        if manager.is_registered(shortcut) == enabled {
            continue;
        }
        let changed = if enabled {
            manager.register(shortcut)
        } else {
            manager.unregister(shortcut)
        };
        if let Err(err) = changed {
            errors.push(format!(
                "无法{}快捷键 {shortcut}：{err}",
                if enabled { "注册" } else { "停用" }
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}

// Keep the two shortcuts as one setting, including a failed second registration.
fn change_shortcuts(app: &tauri::AppHandle, enabled: bool) -> Result<[bool; 2], String> {
    let previous = shortcut_states(app);
    if let Err(err) = restore_shortcut_states(app, [enabled; 2]) {
        return Err(match restore_shortcut_states(app, previous) {
            Ok(()) => format!("{err}；已恢复原快捷键状态"),
            Err(rollback_err) => format!("{err}；恢复原状态也失败：{rollback_err}"),
        });
    }
    Ok(previous)
}

fn desktop_settings(app: &tauri::AppHandle, state: &AppState) -> Result<DesktopSettings, String> {
    let enabled = app.autolaunch().is_enabled();
    let mut errors = state.desktop_errors.lock().map_err(error)?;
    let autostart_enabled = match enabled {
        Ok(enabled) => {
            errors.autostart = None;
            enabled
        }
        Err(err) => {
            errors.autostart = Some(format!("读取开机启动设置失败：{err}"));
            false
        }
    };
    Ok(DesktopSettings {
        autostart_enabled,
        shortcut_enabled: shortcut_states(app).iter().all(|registered| *registered),
        shortcut: SHORTCUT,
        create_shortcut: CREATE_SHORTCUT,
        autostart_error: errors.autostart.clone(),
        shortcut_error: errors.shortcut.clone(),
    })
}

#[tauri::command]
fn get_desktop_settings(
    app: tauri::AppHandle,
    state: State<AppState>,
) -> Result<DesktopSettings, String> {
    desktop_settings(&app, &state)
}

#[tauri::command]
fn set_autostart(
    app: tauri::AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<DesktopSettings, String> {
    let manager = app.autolaunch();
    match manager.is_enabled() {
        Ok(actual) if actual == enabled => return desktop_settings(&app, &state),
        Ok(_) => {}
        Err(err) => {
            let message = format!("读取开机启动设置失败：{err}");
            state.desktop_errors.lock().map_err(error)?.autostart = Some(message.clone());
            return Err(message);
        }
    }
    let changed = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    if let Err(err) = changed {
        let message = format!("修改开机启动设置失败：{err}");
        state.desktop_errors.lock().map_err(error)?.autostart = Some(message.clone());
        return Err(message);
    }
    state.desktop_errors.lock().map_err(error)?.autostart = None;
    let actual = desktop_settings(&app, &state)?;
    if let Some(err) = &actual.autostart_error {
        return Err(err.clone());
    }
    if actual.autostart_enabled != enabled {
        return Err("开机启动设置未达到请求状态，请检查系统设置".into());
    }
    Ok(actual)
}

#[tauri::command]
fn set_shortcut_enabled(
    app: tauri::AppHandle,
    state: State<AppState>,
    enabled: bool,
) -> Result<DesktopSettings, String> {
    let _writer = state.preferences_save.lock().map_err(error)?;
    let previous = state.preferences.lock().map_err(error)?.clone();
    let next = Preferences {
        shortcut_enabled: enabled,
        ..previous
    };
    let encoded = serde_json::to_string(&next).map_err(error)?;
    let previous_registered = match change_shortcuts(&app, enabled) {
        Ok(previous) => previous,
        Err(message) => {
            state.desktop_errors.lock().map_err(error)?.shortcut = Some(message.clone());
            return Err(message);
        }
    };
    if let Err(err) = state.db.set_setting("ui", &encoded) {
        let message = match restore_shortcut_states(&app, previous_registered) {
            Ok(()) => format!("快捷键设置保存失败，已恢复原状态：{err}"),
            Err(rollback_err) => {
                format!("快捷键设置保存失败：{err}；恢复原状态也失败：{rollback_err}")
            }
        };
        state.desktop_errors.lock().map_err(error)?.shortcut = Some(message.clone());
        return Err(message);
    }
    *state.preferences.lock().map_err(error)? = next;
    state.desktop_errors.lock().map_err(error)?.shortcut = None;
    desktop_settings(&app, &state)
}

#[tauri::command]
fn get_integration_info(
    app: tauri::AppHandle,
    state: State<AppState>,
) -> Result<integration::IntegrationInfo, String> {
    integration::info(&state.db, &app.package_info().version.to_string())
}

#[tauri::command]
async fn check_mcp(state: State<'_, AppState>) -> Result<integration::McpCheck, String> {
    let executable = integration::mcp_path()?;
    let data_dir = state
        .db
        .path()
        .parent()
        .ok_or("无法定位数据库目录")?
        .to_path_buf();
    Ok(integration::check(executable, data_dir).await)
}

fn run() -> tauri::Result<()> {
    let mut context = tauri::generate_context!();
    // Build the window below so the WebView builder can use an absolute data path.
    // WindowConfig.data_directory only accepts a relative path under Tauri's own cache.
    context.config_mut().app.windows[0].create = false;
    // Opt-in local WebView QA only. This branch is absent from release builds.
    #[cfg(debug_assertions)]
    if let Ok(port) = std::env::var("AGENTKANBAN_DEBUG_PORT") {
        if let Ok(port) = port.parse::<u16>() {
            context.config_mut().app.windows[0].additional_browser_args =
                Some(format!("--remote-debugging-port={port}"));
        }
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("AgentKanban")
                .build(),
        )
        .plugin(
            tauri_plugin_opener::Builder::new()
                .open_js_links_on_click(false)
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        if shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::KeyN) {
                            show_quick_create(app);
                        } else if shortcut.matches(Modifiers::CONTROL | Modifiers::ALT, Code::KeyK)
                        {
                            toggle_window(app);
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let db = Database::open_default()?;
            let prefs: Preferences = db
                .get_setting("ui")?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let geometry: Geometry = db
                .get_setting("geometry")?
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            let window =
                tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                    .data_directory(Database::default_data_dir()?.join("webview"))
                    .build()?;
            window.set_always_on_top(prefs.always_on_top)?;
            window.set_resizable(!prefs.compact)?;
            window.set_min_size(Some(tauri::LogicalSize::new(
                320.0,
                if prefs.compact { 48.0 } else { 360.0 },
            )))?;
            window.set_size(tauri::LogicalSize::new(
                geometry.width.max(320.0),
                if prefs.compact {
                    48.0
                } else {
                    geometry.height.max(360.0)
                },
            ))?;
            if let (Some(x), Some(y)) = (geometry.x, geometry.y) {
                // Keep the title bar reachable after monitor removal or scaling changes.
                let reachable = window.available_monitors()?.iter().any(|m| {
                    let p = m.position();
                    let s = m.size();
                    x >= p.x
                        && y >= p.y
                        && x + 100 < p.x + s.width as i32
                        && y + 48 < p.y + s.height as i32
                });
                if reachable {
                    window.set_position(tauri::PhysicalPosition::new(x, y))?;
                } else {
                    window.center()?;
                }
            } else {
                window.center()?;
            }
            let shortcut_enabled = prefs.shortcut_enabled;
            app.manage(AppState {
                db,
                preferences: Mutex::new(prefs),
                preferences_save: Mutex::new(()),
                desktop_errors: Mutex::new(DesktopErrors::default()),
                geometry: Mutex::new(geometry),
                geometry_save: Mutex::new(()),
                geometry_dirty: AtomicBool::new(false),
            });

            // Merely installing the autostart plugin preserves the OS setting.
            // A conflicting hotkey is visible in Settings but must never prevent startup.
            if shortcut_enabled {
                if let Err(err) = change_shortcuts(app.handle(), true) {
                    let message = format!("全局快捷键启用失败，可能已被其他应用占用：{err}");
                    eprintln!("{message}");
                    app.state::<AppState>()
                        .desktop_errors
                        .lock()
                        .map_err(error)?
                        .shortcut = Some(message);
                }
            }

            let show = MenuItem::with_id(app, "show", "显示看板", true, None::<&str>)?;
            let create = MenuItem::with_id(app, "new_task", "新建任务", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏看板", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 AgentKanban", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &create, &hide, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().ok_or("missing app icon")?.clone())
                .tooltip("AgentKanban · 由 Agent 更新")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_window(app),
                    "new_task" => show_quick_create(app),
                    "hide" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = hide_window(w, app.state());
                        }
                    }
                    "quit" => {
                        if let Err(err) = save_geometry(&app.state::<AppState>()) {
                            eprintln!("Could not save window position: {err}");
                        }
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(600));
                if let Err(err) = save_geometry(&handle.state::<AppState>()) {
                    let _ = handle.emit("app-error", format!("窗口设置保存失败：{err}"));
                }
            });
            window.show()?;
            Ok(())
        })
        .on_window_event(|window, event| {
            let Some(state) = window.try_state::<AppState>() else {
                return;
            };
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = save_geometry(&state);
                    let _ = window.emit("visibility-changed", false);
                    let _ = window.hide();
                }
                tauri::WindowEvent::Moved(position) => {
                    if let Ok(mut g) = state.geometry.lock() {
                        g.x = Some(position.x);
                        g.y = Some(position.y);
                        state.geometry_dirty.store(true, Ordering::SeqCst);
                    }
                }
                tauri::WindowEvent::Resized(size) => {
                    let compact = state.preferences.lock().map(|p| p.compact).unwrap_or(true);
                    if !compact && size.width > 0 && size.height > 0 {
                        if let (Ok(scale), Ok(mut g)) =
                            (window.scale_factor(), state.geometry.lock())
                        {
                            let logical = size.to_logical::<f64>(scale);
                            // Exclude the queued strip resize when expanding again.
                            if logical.height >= 360.0 {
                                g.width = logical.width;
                                g.height = logical.height;
                                state.geometry_dirty.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_revision,
            create_task,
            review_task,
            send_task_feedback,
            archive_task,
            get_handoff,
            open_external_link,
            get_preferences,
            set_preferences,
            set_compact,
            hide_window,
            get_desktop_settings,
            set_autostart,
            set_shortcut_enabled,
            get_integration_info,
            check_mcp
        ])
        .build(context)?;
    app.run(|app, event| {
        if matches!(event, tauri::RunEvent::Exit) {
            let _ = save_geometry(&app.state::<AppState>());
        }
    });
    Ok(())
}

fn main() {
    if let Err(err) = run() {
        eprintln!("AgentKanban 启动失败：{err}");
        #[cfg(windows)]
        {
            let message: Vec<u16> =
                format!("AgentKanban 启动失败：{err}\n请检查数据目录权限和数据库文件。\0")
                    .encode_utf16()
                    .collect();
            let title: Vec<u16> = "AgentKanban\0".encode_utf16().collect();
            unsafe {
                windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    0x10,
                );
            }
        }
        std::process::exit(1);
    }
}
