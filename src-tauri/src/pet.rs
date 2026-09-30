//! The desktop pet: small transparent windows of pixel Agents. Merged, one window holds
//! every pet; split, each Agent gets its own window. A pet sits on the board's top edge
//! and follows the board ("docked"), stays wherever the user drops it, or — in gravity
//! mode — drops onto the taskbar and wanders, flying back to the board when it needs you.
//!
//! Windows are created off the main thread (async commands, spawned threads): WebView2
//! deadlocks when a window is built inside a sync command or an event handler.

use crate::{error, AppState};
use kanban_core::Database;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    f64::consts::PI,
    sync::Mutex,
    time::{Duration, Instant},
};
use tauri::{
    menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem},
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, Wry,
};

/// The merged window; split windows are "pet-1", "pet-2", ...
pub(crate) const LABEL: &str = "pet";
const SPLIT_PREFIX: &str = "pet-";
const MAIN: &str = "main";
/// Logical sizes, matching src/pet/Pet.tsx: one pet per slot plus a margin.
const SLOT: f64 = 96.0;
const PAD: f64 = 16.0;
pub(crate) const HEIGHT: f64 = 132.0;
/// How far (logical px) a drop may land from the board's top edge and still dock.
const SNAP: f64 = 64.0;
/// Feet overlap the edge they stand on, so pets stand on the board or the taskbar.
const OVERLAP: f64 = 6.0;
const TICK: Duration = Duration::from_millis(33);
/// Logical px per second while wandering, and px/s² while falling.
const WALK_SPEED: f64 = 32.0;
const GRAVITY: f64 = 2400.0;
const GLIDE: Duration = Duration::from_millis(800);
/// How long a pet that needs you stays at the board before dropping back down.
const RETURN_HOLD: Duration = Duration::from_secs(8);

pub(crate) fn is_pet(label: &str) -> bool {
    label == LABEL || label.starts_with(SPLIT_PREFIX)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Place {
    pub docked: bool,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

impl Default for Place {
    fn default() -> Self {
        Self {
            docked: true,
            x: None,
            y: None,
        }
    }
}

impl Place {
    fn at(x: i32, y: i32) -> Self {
        Self {
            docked: false,
            x: Some(x),
            y: Some(y),
        }
    }
}

/// Saved in the "pet" setting. Places are keyed by Agent ("" for the merged window).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Config {
    pub split: bool,
    pub pinned: bool,
    pub on_top: bool,
    pub gravity: bool,
    pub places: HashMap<String, Place>,
    /// Agents that had their own window last time, so split windows come back at start.
    pub keys: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            split: false,
            pinned: false,
            on_top: true,
            gravity: false,
            places: HashMap::new(),
            keys: Vec::new(),
        }
    }
}

/// What the pet windows need to know about the options.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Options {
    split: bool,
    pinned: bool,
    on_top: bool,
    gravity: bool,
}

impl Config {
    fn options(&self) -> Options {
        Options {
            split: self.split,
            pinned: self.pinned,
            on_top: self.on_top,
            gravity: self.gravity,
        }
    }
    fn docked(&self, key: &str) -> bool {
        self.places.get(key).is_none_or(|p| p.docked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Then {
    Home,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Motion {
    /// Docked or where the user left it.
    Still,
    Fall {
        vy: f64,
    },
    Walk {
        dir: f64,
        until: Instant,
    },
    Idle {
        until: Instant,
    },
    Glide {
        from: (f64, f64),
        to: (f64, f64),
        start: Instant,
        then: Then,
    },
    /// Back at the board because it needs you.
    Hold {
        until: Instant,
    },
}

impl Motion {
    /// The name the pet window animates by, plus the walking direction.
    fn shown(&self) -> (&'static str, i8) {
        match self {
            Motion::Still | Motion::Hold { .. } => ("still", 1),
            Motion::Fall { .. } => ("fall", 1),
            Motion::Walk { dir, .. } => ("walk", if *dir < 0.0 { -1 } else { 1 }),
            Motion::Idle { .. } => ("idle", 1),
            Motion::Glide { .. } => ("glide", 1),
        }
    }
}

/// Physical px: the x range a window may walk in, and the top y at which it stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Area {
    left: f64,
    right: f64,
    floor: f64,
}

#[derive(Debug)]
struct Win {
    label: String,
    key: String,
    /// Logical width.
    width: f64,
    /// Physical position, kept while moving so rounding never drifts.
    pos: (f64, f64),
    scale: f64,
    area: Area,
    motion: Motion,
    drag: Option<Instant>,
    shown: (&'static str, i8),
    /// Hidden until its page has drawn the pets, so a new window never flashes empty.
    ready: bool,
    created: Instant,
    /// Played once the window shows: "split" or "merge".
    intro: Option<&'static str>,
}

struct Inner {
    config: Config,
    wins: Vec<Win>,
    /// Windows being replaced (split/merge): closed once their replacements have shown.
    retiring: Vec<String>,
    next_id: u32,
    rng: u64,
}

impl Inner {
    fn win(&mut self, label: &str) -> Option<&mut Win> {
        self.wins.iter_mut().find(|w| w.label == label)
    }
}

pub(crate) struct PetState(Mutex<Inner>);

impl PetState {
    pub fn load(db: &Database) -> Self {
        let config = db
            .get_setting("pet")
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9e37_79b9_7f4a_7c15)
            | 1;
        Self(Mutex::new(Inner {
            config,
            wins: Vec::new(),
            retiring: Vec::new(),
            next_id: 1,
            rng: seed,
        }))
    }
}

/// Runs `f` on the pet state. Never call window getters inside: they wait on the main
/// thread, whose window events lock this state too.
fn with<T>(app: &AppHandle, f: impl FnOnce(&mut Inner) -> T) -> Option<T> {
    let state = app.try_state::<PetState>()?;
    let mut inner = state.0.lock().ok()?;
    Some(f(&mut inner))
}

fn save(app: &AppHandle) -> Result<(), String> {
    let json = with(app, |s| serde_json::to_string(&s.config))
        .ok_or("桌宠状态不可用")?
        .map_err(error)?;
    app.state::<AppState>()
        .db
        .set_setting("pet", &json)
        .map_err(error)
}

fn labels(app: &AppHandle) -> Vec<String> {
    with(app, |s| s.wins.iter().map(|w| w.label.clone()).collect()).unwrap_or_default()
}

/// Sends an event to every pet window.
pub(crate) fn broadcast<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    for label in labels(app) {
        let _ = app.emit_to(label.as_str(), event, payload.clone());
    }
}

/// Shows or removes the pets to match the preference.
pub(crate) fn sync(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let open = !labels(app).is_empty();
    if enabled && !open {
        let (split, keys) =
            with(app, |s| (s.config.split, s.config.keys.clone())).unwrap_or_default();
        if split && !keys.is_empty() {
            for key in keys {
                open_window(app, &key, None, None)?;
            }
        } else {
            with(app, |s| s.config.split = false);
            open_window(app, "", None, None)?;
        }
        follow(app);
    } else if !enabled && open {
        for label in labels(app) {
            close_window(app, &label);
        }
        retire_now(app);
    }
    Ok(())
}

/// Takes windows out of play but keeps them on screen until their replacements show.
fn retire(app: &AppHandle, retired: &[String]) {
    with(app, |s| {
        s.wins.retain(|w| !retired.contains(&w.label));
        s.retiring.extend(retired.iter().cloned());
    });
}

/// Closes the replaced windows once every current window has shown.
fn retire_when_ready(app: &AppHandle) {
    let done = with(app, |s| {
        if s.wins.iter().all(|w| w.ready) {
            std::mem::take(&mut s.retiring)
        } else {
            Vec::new()
        }
    })
    .unwrap_or_default();
    for label in done {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.destroy();
        }
    }
}

fn retire_now(app: &AppHandle) {
    for label in with(app, |s| std::mem::take(&mut s.retiring)).unwrap_or_default() {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.destroy();
        }
    }
}

/// Shows a window whose page has drawn, plays its intro, and retires what it replaces.
fn reveal(app: &AppHandle, label: &str) {
    let intro = with(app, |s| {
        s.win(label).and_then(|w| {
            w.ready = true;
            w.intro.take()
        })
    })
    .flatten();
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.show();
    }
    if let Some(action) = intro {
        let _ = app.emit_to(label, "pet-action", action);
    }
    retire_when_ready(app);
}

fn close_window(app: &AppHandle, label: &str) {
    with(app, |s| s.wins.retain(|w| w.label != label));
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.destroy();
    }
}

/// Opens a pet window: `key` "" is the merged window. `at` places it explicitly (a split
/// window appearing where its pet stood in the merged window) and says if it is docked.
fn open_window(
    app: &AppHandle,
    key: &str,
    at: Option<(i32, i32, bool)>,
    intro: Option<&'static str>,
) -> Result<(), String> {
    let width = SLOT + PAD;
    // Merging back quickly after a split: the old merged window may still be on its way out.
    if key.is_empty() {
        let stale = with(app, |s| {
            let found = s.retiring.iter().any(|l| l == LABEL);
            s.retiring.retain(|l| l != LABEL);
            found
        });
        if stale == Some(true) {
            if let Some(window) = app.get_webview_window(LABEL) {
                let _ = window.destroy();
            }
        }
    }
    let (label, on_top) = with(app, |s| {
        let label = if key.is_empty() {
            LABEL.to_string()
        } else {
            s.next_id += 1;
            format!("{SPLIT_PREFIX}{}", s.next_id - 1)
        };
        s.wins.push(Win {
            label: label.clone(),
            key: key.to_string(),
            width,
            pos: (0.0, 0.0),
            scale: 1.0,
            area: Area::default(),
            motion: Motion::Still,
            drag: None,
            shown: ("still", 1),
            ready: false,
            created: Instant::now(),
            intro,
        });
        if let Some((x, y, docked)) = at {
            let place = if docked {
                Place::default()
            } else {
                Place::at(x, y)
            };
            s.config.places.insert(key.to_string(), place);
        }
        (label, s.config.on_top)
    })
    .ok_or("桌宠状态不可用")?;
    let built = tauri::WebviewWindowBuilder::new(app, &label, WebviewUrl::default())
        .title("AgentKanban Agents")
        .inner_size(width, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(on_top)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        // One WebView2 environment per process: share the board's data folder.
        .data_directory(Database::default_data_dir().map_err(error)?.join("webview"))
        .build();
    let window = match built {
        Ok(window) => window,
        Err(err) => {
            with(app, |s| s.wins.retain(|w| w.label != label));
            return Err(error(err));
        }
    };
    let place = with(app, |s| {
        s.config.places.get(key).cloned().unwrap_or_default()
    })
    .unwrap_or_default();
    let spot = match (at, place.docked, place.x, place.y) {
        (Some((x, y, _)), _, _, _) => Some((x, y)),
        (None, false, Some(x), Some(y)) if reachable(&window, x, y) => Some((x, y)),
        _ => None,
    };
    if spot.is_none() {
        with(app, |s| {
            s.config.places.insert(key.to_string(), Place::default())
        });
    }
    if let Some((x, y)) = spot.or_else(|| dock_target(app, &label)) {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
    // Shown by pet_ready once the page has drawn.
    if with(app, |s| s.config.gravity).unwrap_or(false) {
        start_fall(app, &label);
    }
    Ok(())
}

fn reachable(window: &WebviewWindow, x: i32, y: i32) -> bool {
    window.available_monitors().is_ok_and(|monitors| {
        monitors.iter().any(|m| {
            let p = m.position();
            let s = m.size();
            x + 40 >= p.x
                && y >= p.y
                && x + 40 < p.x + s.width as i32
                && y + 40 < p.y + s.height as i32
        })
    })
}

/// The board's top edge: merged pets centered, split pets side by side in list order.
fn dock_target(app: &AppHandle, label: &str) -> Option<(i32, i32)> {
    let main = app.get_webview_window(MAIN)?;
    let origin = main.outer_position().ok()?;
    let size = main.outer_size().ok()?;
    let scale = main.scale_factor().ok()?;
    let (index, group) = with(app, |s| {
        let index = s.wins.iter().position(|w| w.label == label)?;
        Some(if s.config.split {
            (index, s.wins.len() as f64 * SLOT + PAD)
        } else {
            (0, s.wins[index].width)
        })
    })??;
    let x =
        origin.x as f64 + (size.width as f64 - group * scale) / 2.0 + index as f64 * SLOT * scale;
    let y = origin.y as f64 - (HEIGHT - OVERLAP) * scale;
    Some((x.round() as i32, y.round() as i32))
}

/// Where a pet rests when nothing is moving it: docked, or its saved spot.
fn home_target(app: &AppHandle, label: &str) -> Option<(i32, i32)> {
    let place = with(app, |s| {
        let key = s.wins.iter().find(|w| w.label == label)?.key.clone();
        Some(s.config.places.get(&key).cloned().unwrap_or_default())
    })??;
    match (place.docked, place.x, place.y) {
        (false, Some(x), Some(y)) => Some((x, y)),
        _ => dock_target(app, label),
    }
}

/// The board moved or resized: docked pets go along.
pub(crate) fn follow(app: &AppHandle) {
    let docked: Vec<String> = with(app, |s| {
        if s.config.gravity {
            return Vec::new();
        }
        s.wins
            .iter()
            .filter(|w| w.motion == Motion::Still && w.drag.is_none() && s.config.docked(&w.key))
            .map(|w| w.label.clone())
            .collect()
    })
    .unwrap_or_default();
    for label in docked {
        if let (Some(window), Some((x, y))) =
            (app.get_webview_window(&label), dock_target(app, &label))
        {
            let _ = window.set_position(PhysicalPosition::new(x, y));
        }
    }
}

fn position(app: &AppHandle, label: &str) -> Option<(f64, f64, f64)> {
    let window = app.get_webview_window(label)?;
    let p = window.outer_position().ok()?;
    Some((p.x as f64, p.y as f64, window.scale_factor().ok()?))
}

/// Starts falling from wherever the window is, onto the taskbar of the screen below it.
fn start_fall(app: &AppHandle, label: &str) {
    let Some(window) = app.get_webview_window(label) else {
        return;
    };
    let (Ok(p), Ok(size), Ok(scale)) = (
        window.outer_position(),
        window.outer_size(),
        window.scale_factor(),
    ) else {
        return;
    };
    let monitor = window
        .monitor_from_point(
            p.x as f64 + size.width as f64 / 2.0,
            p.y as f64 + size.height as f64 / 2.0,
        )
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return;
    };
    let work = monitor.work_area();
    let left = work.position.x as f64;
    let area = Area {
        left,
        right: (left + work.size.width as f64 - size.width as f64).max(left),
        floor: work.position.y as f64 + work.size.height as f64 - size.height as f64
            + OVERLAP * scale,
    };
    with(app, |s| {
        if let Some(w) = s.win(label) {
            w.pos = ((p.x as f64).clamp(area.left, area.right), p.y as f64);
            w.scale = scale;
            w.area = area;
            w.motion = Motion::Fall { vy: 0.0 };
        }
    });
}

/// Flies to the board (`Hold`) or back home (`Home`, gravity turned off).
fn start_glide(app: &AppHandle, label: &str, then: Then) {
    let target = match then {
        Then::Hold => dock_target(app, label),
        Then::Home => home_target(app, label),
    };
    let (Some((x, y, scale)), Some(to)) = (position(app, label), target) else {
        return;
    };
    with(app, |s| {
        if let Some(w) = s.win(label) {
            w.pos = (x, y);
            w.scale = scale;
            w.motion = Motion::Glide {
                from: (x, y),
                to: (to.0 as f64, to.1 as f64),
                start: Instant::now(),
                then,
            };
        }
    });
}

/// One animation step; returns whether the window moved.
fn step(
    w: &mut Win,
    now: Instant,
    dt: f64,
    gravity: bool,
    random: &mut impl FnMut() -> f64,
) -> bool {
    let before = w.pos;
    match w.motion {
        Motion::Still => {}
        Motion::Hold { until } => {
            if now >= until {
                w.motion = if gravity {
                    Motion::Fall { vy: 0.0 }
                } else {
                    Motion::Still
                };
            }
        }
        Motion::Fall { vy } => {
            let vy = vy + GRAVITY * w.scale * dt;
            w.pos.1 += vy * dt;
            if w.pos.1 >= w.area.floor {
                w.pos.1 = w.area.floor;
                w.motion = Motion::Idle {
                    until: now + Duration::from_secs_f64(0.8 + random() * 1.2),
                };
            } else {
                w.motion = Motion::Fall { vy };
            }
        }
        Motion::Walk { mut dir, until } => {
            w.pos.0 += dir * WALK_SPEED * w.scale * dt;
            if w.pos.0 <= w.area.left || w.pos.0 >= w.area.right {
                w.pos.0 = w.pos.0.clamp(w.area.left, w.area.right);
                dir = -dir;
            }
            w.pos.1 = w.area.floor;
            w.motion = if now >= until {
                Motion::Idle {
                    until: now + Duration::from_secs_f64(1.5 + random() * 4.0),
                }
            } else {
                Motion::Walk { dir, until }
            };
        }
        Motion::Idle { until } => {
            if now >= until {
                w.motion = Motion::Walk {
                    dir: if random() < 0.5 { -1.0 } else { 1.0 },
                    until: now + Duration::from_secs_f64(2.0 + random() * 3.0),
                };
            }
        }
        Motion::Glide {
            from,
            to,
            start,
            then,
        } => {
            let k =
                (now.saturating_duration_since(start).as_secs_f64() / GLIDE.as_secs_f64()).min(1.0);
            let ease = 0.5 - 0.5 * (PI * k).cos();
            let arc = (PI * k).sin() * 40.0 * w.scale;
            w.pos = (
                from.0 + (to.0 - from.0) * ease,
                from.1 + (to.1 - from.1) * ease - arc,
            );
            if k >= 1.0 {
                w.motion = match then {
                    Then::Home => Motion::Still,
                    Then::Hold => Motion::Hold {
                        until: now + RETURN_HOLD,
                    },
                };
            }
        }
    }
    w.pos != before
}

#[cfg(windows)]
fn mouse_down() -> bool {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    unsafe { GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0 }
}

#[cfg(not(windows))]
fn mouse_down() -> bool {
    false
}

fn motion_payload((state, dir): (&'static str, i8)) -> serde_json::Value {
    serde_json::json!({ "state": state, "dir": dir })
}

type Move = (String, Option<(i32, i32)>, Option<(&'static str, i8)>);

/// Moves the pets: settles finished drags and runs falling, wandering and gliding.
pub(crate) fn spawn(app: AppHandle) {
    // One app-wide handler: Tauri hands every menu event to every window's own handler,
    // which would toggle an option once per open pet window.
    app.on_menu_event(|app, event| menu_clicked(app, event.id.as_ref()));
    std::thread::spawn(move || {
        let mut last = Instant::now();
        loop {
            std::thread::sleep(TICK);
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f64().min(0.1);
            last = now;
            let dropped: Vec<String> = with(&app, |s| {
                let up = !mouse_down();
                s.wins
                    .iter_mut()
                    .filter(|w| {
                        up && w
                            .drag
                            .is_some_and(|at| at.elapsed() >= Duration::from_millis(150))
                    })
                    .map(|w| {
                        w.drag = None;
                        w.label.clone()
                    })
                    .collect()
            })
            .unwrap_or_default();
            // A page that never reported in still shows up.
            let late: Vec<String> = with(&app, |s| {
                s.wins
                    .iter()
                    .filter(|w| !w.ready && w.created.elapsed() >= Duration::from_secs(4))
                    .map(|w| w.label.clone())
                    .collect()
            })
            .unwrap_or_default();
            for label in late {
                reveal(&app, &label);
            }
            for label in dropped {
                if let Err(err) = settle(&app, &label) {
                    let _ = app.emit("app-error", format!("桌宠位置保存失败：{err}"));
                }
            }
            let moves: Vec<Move> = with(&app, |s| {
                let gravity = s.config.gravity;
                let mut rng = s.rng;
                let mut random = || {
                    // xorshift64: plenty for picking walk directions.
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    (rng >> 11) as f64 / (1u64 << 53) as f64
                };
                let out = s
                    .wins
                    .iter_mut()
                    .filter(|w| w.drag.is_none())
                    .filter_map(|w| {
                        let moved = step(w, now, dt, gravity, &mut random);
                        let shown = w.motion.shown();
                        let changed = shown != w.shown;
                        w.shown = shown;
                        (moved || changed).then(|| {
                            (
                                w.label.clone(),
                                moved.then(|| (w.pos.0.round() as i32, w.pos.1.round() as i32)),
                                changed.then_some(shown),
                            )
                        })
                    })
                    .collect();
                s.rng = rng;
                out
            })
            .unwrap_or_default();
            for (label, at, shown) in moves {
                if let (Some((x, y)), Some(window)) = (at, app.get_webview_window(&label)) {
                    let _ = window.set_position(PhysicalPosition::new(x, y));
                }
                if let Some(shown) = shown {
                    let _ = app.emit_to(label.as_str(), "pet-motion", motion_payload(shown));
                }
            }
        }
    });
}

/// A drag ended: in gravity mode fall from there; otherwise dock when dropped near the
/// board's top edge, else remember the spot.
fn settle(app: &AppHandle, label: &str) -> Result<(), String> {
    let Some((x, y, _)) = position(app, label) else {
        return Ok(());
    };
    let (x, y) = (x as i32, y as i32);
    if with(app, |s| s.config.gravity).unwrap_or(false) {
        start_fall(app, label);
        return app.emit_to(label, "pet-dropped", false).map_err(error);
    }
    let near = match (dock_target(app, label), app.get_webview_window(MAIN)) {
        (Some((dx, dy)), Some(main)) => {
            let scale = main.scale_factor().map_err(error)?;
            let width = main.outer_size().map_err(error)?.width as f64;
            let limit = SNAP * scale;
            f64::from((y - dy).abs()) <= limit && f64::from((x - dx).abs()) <= width / 2.0 + limit
        }
        _ => false,
    };
    let place = if near {
        Place::default()
    } else {
        Place::at(x, y)
    };
    with(app, |s| {
        let key = s
            .wins
            .iter()
            .find(|w| w.label == label)
            .map(|w| w.key.clone());
        if let Some(key) = key {
            s.config.places.insert(key, place.clone());
        }
    });
    if near {
        follow(app);
    }
    save(app)?;
    app.emit_to(label, "pet-dropped", place.docked)
        .map_err(error)
}

fn toggle(app: &AppHandle, id: &str) -> Result<(), String> {
    let config = with(app, |s| {
        let c = &mut s.config;
        match id {
            "pet:split" => c.split = !c.split,
            "pet:pinned" => c.pinned = !c.pinned,
            "pet:top" => c.on_top = !c.on_top,
            "pet:gravity" => c.gravity = !c.gravity,
            _ => {}
        }
        c.clone()
    })
    .ok_or("桌宠状态不可用")?;
    save(app)?;
    let action = match id {
        // The new windows play these when they show.
        "pet:split" => "",
        "pet:pinned" if config.pinned => "pin",
        "pet:pinned" => "unpin",
        "pet:top" if config.on_top => "top",
        "pet:top" => "untop",
        _ if config.gravity => "gravity",
        _ => "float",
    };
    match id {
        "pet:top" => {
            for label in labels(app) {
                if let Some(window) = app.get_webview_window(&label) {
                    let _ = window.set_always_on_top(config.on_top);
                }
            }
        }
        "pet:gravity" => {
            for label in labels(app) {
                with(app, |s| {
                    if let Some(w) = s.win(&label) {
                        w.drag = None;
                    }
                });
                if config.gravity {
                    start_fall(app, &label);
                } else {
                    start_glide(app, &label, Then::Home);
                }
            }
        }
        // Splitting waits for the merged window to report its pets (pet_layout).
        "pet:split" if !config.split => merge(app)?,
        _ => {}
    }
    broadcast(app, "pet-options", config.options());
    if !action.is_empty() {
        broadcast(app, "pet-action", action);
    }
    Ok(())
}

fn menu_clicked(app: &AppHandle, id: &str) {
    if !id.starts_with("pet:") {
        return;
    }
    let (app, id) = (app.clone(), id.to_string());
    // Menu events run on the main thread, where building a window would deadlock.
    std::thread::spawn(move || {
        let result = match id.as_str() {
            "pet:open" => {
                crate::show_window(&app);
                Ok(())
            }
            "pet:dock" => pet_redock(app.clone()),
            _ => toggle(&app, &id),
        };
        if let Err(err) = result {
            let _ = app.emit("app-error", format!("桌宠设置失败：{err}"));
        }
    });
}

/// Split windows back into one: the merged window opens where the first pet stood.
fn merge(app: &AppHandle) -> Result<(), String> {
    let first = with(app, |s| {
        let first = s.wins.iter().find(|w| w.label != LABEL)?;
        Some((s.config.docked(&first.key), first.label.clone()))
    })
    .flatten();
    let Some((docked, first)) = first else {
        return Ok(());
    };
    let at = if docked {
        None
    } else {
        position(app, &first).map(|(x, y, _)| (x as i32, y as i32, false))
    };
    retire(app, &labels(app));
    with(app, |s| s.config.keys.clear());
    open_window(app, "", at, Some("merge"))?;
    save(app)
}

/// The window's pet key ("" merged), the options and the current motion.
#[tauri::command]
pub(crate) fn pet_hello(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<serde_json::Value, String> {
    with(&app, |s| {
        let w = s.wins.iter().find(|w| w.label == window.label());
        serde_json::json!({
            "key": w.map(|w| w.key.clone()).unwrap_or_default(),
            "options": s.config.options(),
            "motion": motion_payload(w.map(|w| w.motion.shown()).unwrap_or(("still", 1))),
        })
    })
    .ok_or_else(|| "桌宠状态不可用".into())
}

/// The page has drawn its pets: show the window.
#[tauri::command(async)]
pub(crate) fn pet_ready(app: AppHandle, window: WebviewWindow) {
    reveal(&app, window.label());
}

/// Split mode: the Agents that should each have a window, most urgent first. Reported by
/// the merged window when splitting, then by the first split window as Agents come and go.
#[tauri::command(async)]
pub(crate) fn pet_layout(
    app: AppHandle,
    window: WebviewWindow,
    keys: Vec<String>,
) -> Result<(), String> {
    let mut wanted: Vec<String> = Vec::new();
    for key in keys {
        if !key.is_empty() && !wanted.contains(&key) && wanted.len() < 6 {
            wanted.push(key);
        }
    }
    let splitting = window.label() == LABEL;
    let (leader, current, docked) = with(&app, |s| {
        let leader = s.config.split
            && !s.retiring.iter().any(|l| l == window.label())
            && (splitting || s.wins.first().is_some_and(|w| w.label == window.label()));
        let current: Vec<(String, String)> = s
            .wins
            .iter()
            .map(|w| (w.label.clone(), w.key.clone()))
            .collect();
        (leader, current, s.config.docked(""))
    })
    .unwrap_or_default();
    if !leader || wanted.is_empty() {
        return Ok(());
    }
    if !splitting && current.iter().map(|(_, k)| k).eq(wanted.iter()) {
        return Ok(());
    }
    // Splitting: each pet's window appears exactly where it stood in the merged window.
    let origin = if splitting {
        position(&app, LABEL)
    } else {
        None
    };
    if splitting {
        retire(&app, &[LABEL.to_string()]);
    }
    for (label, key) in &current {
        if label != LABEL && !wanted.contains(key) {
            close_window(&app, label);
        }
    }
    for (index, key) in wanted.iter().enumerate() {
        if current.iter().any(|(label, k)| k == key && label != LABEL) {
            continue;
        }
        let at = origin.map(|(x, y, scale)| {
            (
                (x + index as f64 * SLOT * scale).round() as i32,
                y as i32,
                docked,
            )
        });
        open_window(&app, key, at, splitting.then_some("split"))?;
    }
    with(&app, |s| {
        s.wins.sort_by_key(|w| {
            wanted
                .iter()
                .position(|k| *k == w.key)
                .unwrap_or(usize::MAX)
        });
        s.config.keys = wanted.clone();
    });
    follow(&app);
    save(&app)
}

/// The merged window asks for a width that fits its pets.
#[tauri::command]
pub(crate) fn pet_resize(app: AppHandle, window: WebviewWindow, width: f64) -> Result<(), String> {
    if window.label() != LABEL || !(60.0..=800.0).contains(&width) {
        return Err("无效的桌宠尺寸".into());
    }
    with(&app, |s| {
        if let Some(w) = s.win(LABEL) {
            w.width = width;
        }
    });
    window
        .set_size(tauri::LogicalSize::new(width, HEIGHT))
        .map_err(error)?;
    follow(&app);
    Ok(())
}

/// Starts a user drag, unless the pets are pinned. Returns whether it started.
#[tauri::command]
pub(crate) fn pet_drag(app: AppHandle, window: WebviewWindow) -> Result<bool, String> {
    let allowed = with(&app, |s| {
        if s.config.pinned {
            return false;
        }
        match s.win(window.label()) {
            Some(w) => {
                w.drag = Some(Instant::now());
                w.motion = Motion::Still;
                true
            }
            None => false,
        }
    })
    .unwrap_or(false);
    if allowed {
        window.start_dragging().map_err(error)?;
    }
    Ok(allowed)
}

/// Gravity mode: a pet that needs you flies back to the board for a while.
#[tauri::command]
pub(crate) fn pet_attention(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let label = window.label().to_string();
    let go = with(&app, |s| {
        let gravity = s.config.gravity;
        match s.win(&label) {
            Some(w) if gravity && w.drag.is_none() => match w.motion {
                Motion::Hold { .. } => {
                    w.motion = Motion::Hold {
                        until: Instant::now() + RETURN_HOLD,
                    };
                    false
                }
                Motion::Glide { .. } => false,
                _ => true,
            },
            _ => false,
        }
    })
    .unwrap_or(false);
    if go {
        start_glide(&app, &label, Then::Hold);
    }
    Ok(())
}

/// The right-click menu.
#[tauri::command(async)]
pub(crate) fn pet_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let english = app
        .state::<AppState>()
        .preferences
        .lock()
        .map(|p| p.english())
        .unwrap_or(false);
    let tr = |zh: &'static str, en: &'static str| if english { en } else { zh };
    let options = with(&app, |s| s.config.options()).ok_or("桌宠状态不可用")?;
    let check = |id: &str, text: &str, on: bool| {
        CheckMenuItem::with_id(&app, id, text, true, on, None::<&str>).map_err(error)
    };
    let split = check("pet:split", tr("拆分 Agent", "Split Agents"), options.split)?;
    let pinned = check("pet:pinned", tr("固定位置", "Pin in place"), options.pinned)?;
    let top = check("pet:top", tr("置顶", "Always on top"), options.on_top)?;
    let gravity = check(
        "pet:gravity",
        tr("重力模式", "Gravity mode"),
        options.gravity,
    )?;
    let separator = PredefinedMenuItem::separator(&app).map_err(error)?;
    let dock = MenuItem::with_id(
        &app,
        "pet:dock",
        tr("吸附回看板", "Put back on the board"),
        !options.gravity,
        None::<&str>,
    )
    .map_err(error)?;
    let open = MenuItem::with_id(
        &app,
        "pet:open",
        tr("打开看板", "Open the board"),
        true,
        None::<&str>,
    )
    .map_err(error)?;
    let items: [&dyn IsMenuItem<Wry>; 7] =
        [&split, &pinned, &top, &gravity, &separator, &dock, &open];
    let menu = Menu::with_items(&app, &items).map_err(error)?;
    window.popup_menu(&menu).map_err(error)
}

/// Put the pets back on the board (Settings, menu).
#[tauri::command]
pub(crate) fn pet_redock(app: AppHandle) -> Result<(), String> {
    with(&app, |s| {
        for place in s.config.places.values_mut() {
            *place = Place::default();
        }
    });
    save(&app)?;
    follow(&app);
    Ok(())
}

/// Clicking a pet brings up the board, opened on the task the pet stands for.
#[tauri::command]
pub(crate) fn pet_open(app: AppHandle, task_id: Option<i64>) -> Result<(), String> {
    crate::show_window(&app);
    app.emit_to(MAIN, "open-task", task_id).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(motion: Motion) -> Win {
        Win {
            label: LABEL.into(),
            key: String::new(),
            width: SLOT + PAD,
            pos: (100.0, 0.0),
            scale: 1.0,
            area: Area {
                left: 0.0,
                right: 500.0,
                floor: 300.0,
            },
            motion,
            drag: None,
            shown: ("still", 1),
            ready: true,
            created: Instant::now(),
            intro: None,
        }
    }

    #[test]
    fn falling_lands_on_the_floor_then_wanders_inside_the_work_area() {
        let mut w = win(Motion::Fall { vy: 0.0 });
        let mut now = Instant::now();
        let mut n = 0.0;
        let mut random = || {
            n = (n + 0.37) % 1.0;
            n
        };
        for _ in 0..200 {
            now += TICK;
            step(&mut w, now, TICK.as_secs_f64(), true, &mut random);
        }
        assert_eq!(w.pos.1, 300.0);
        let mut walked = false;
        for _ in 0..3000 {
            now += TICK;
            step(&mut w, now, TICK.as_secs_f64(), true, &mut random);
            walked |= matches!(w.motion, Motion::Walk { .. });
            assert!((0.0..=500.0).contains(&w.pos.0));
            assert_eq!(w.pos.1, 300.0);
        }
        assert!(walked);
    }

    #[test]
    fn returning_pet_holds_at_the_board_then_drops_again() {
        let start = Instant::now();
        let mut w = win(Motion::Glide {
            from: (100.0, 300.0),
            to: (200.0, 50.0),
            start,
            then: Then::Hold,
        });
        let mut random = || 0.5;
        step(&mut w, start + GLIDE, 0.03, true, &mut random);
        assert!((w.pos.0 - 200.0).abs() < 1e-6 && (w.pos.1 - 50.0).abs() < 1e-6);
        assert!(matches!(w.motion, Motion::Hold { .. }));
        step(&mut w, start + GLIDE + RETURN_HOLD, 0.03, true, &mut random);
        assert!(matches!(w.motion, Motion::Fall { .. }));
    }

    #[test]
    fn old_and_partial_settings_load_with_pets_on_top() {
        let config: Config = serde_json::from_str(r#"{"docked":false,"x":5,"y":6}"#).unwrap();
        assert!(config.on_top && !config.split && !config.gravity && !config.pinned);
        assert!(config.docked("anything"));
        let config: Config = serde_json::from_str(r#"{"gravity":true}"#).unwrap();
        assert!(config.gravity && config.on_top);
    }
}
