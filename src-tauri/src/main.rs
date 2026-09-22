#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use kanban_core::{BoardSnapshot, Database};
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

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct Preferences {
    theme: String,
    always_on_top: bool,
    compact: bool,
    filter: String,
    collapsed_projects: Vec<i64>,
    expanded_projects: Vec<i64>,
    completed_projects: Vec<i64>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: "light".into(),
            always_on_top: true,
            compact: false,
            filter: "all".into(),
            collapsed_projects: vec![],
            expanded_projects: vec![],
            completed_projects: vec![],
        }
    }
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

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Result<BoardSnapshot, String> {
    state.db.board().map_err(error)
}

#[tauri::command]
fn get_revision(state: State<AppState>) -> Result<i64, String> {
    state.db.revision().map_err(error)
}

#[tauri::command]
fn get_preferences(state: State<AppState>) -> Result<Preferences, String> {
    Ok(state.preferences.lock().map_err(error)?.clone())
}

#[tauri::command]
fn set_preferences(
    window: WebviewWindow,
    state: State<AppState>,
    preferences: Preferences,
) -> Result<Preferences, String> {
    if !["light", "dark"].contains(&preferences.theme.as_str())
        || !["all", "in_progress", "blocked", "todo"].contains(&preferences.filter.as_str())
    {
        return Err("无效的显示设置".into());
    }
    // Compact geometry belongs to the native window command, never a stale UI snapshot.
    let previous = state.preferences.lock().map_err(error)?.clone();
    let compact = previous.compact;
    let next = Preferences {
        compact,
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

#[tauri::command]
fn hide_window(window: WebviewWindow, state: State<AppState>) -> Result<(), String> {
    save_geometry(&state)?;
    window.emit("visibility-changed", false).map_err(error)?;
    window.hide().map_err(error)
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
            app.manage(AppState {
                db,
                preferences: Mutex::new(prefs),
                geometry: Mutex::new(geometry),
                geometry_save: Mutex::new(()),
                geometry_dirty: AtomicBool::new(false),
            });

            let show = MenuItem::with_id(app, "show", "显示看板", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "隐藏看板", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出 AgentKanban", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &hide, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().ok_or("missing app icon")?.clone())
                .tooltip("AgentKanban · 由 Agent 更新")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_window(app),
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
            get_preferences,
            set_preferences,
            set_compact,
            hide_window
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
