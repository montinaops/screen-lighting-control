//! The application: owns all state and the hidden controller window, and reacts to events.

use crate::color;
use crate::config::{Ini, Paths};
use crate::engine::magnify::Filter;
use crate::engine::{self, gamma, hardware, overlay::Overlays, Event, Events};
use crate::foreground;
use crate::hotkeys;
use crate::info;
use crate::model::{self, RuleAction, SceneEffect, Settings};
use crate::monitors::{self, Monitor};
use crate::schedule;
use crate::tray::{self, MenuItem, Tray};
use crate::ui::flyout::{self, Flyout};
use crate::ui::settings::{self as settings_ui, SettingsWindow};
use crate::ui::{osd, osd::Osd, theme};
use crate::win::{self, CONTROLLER_CLASS, WM_APP_ACTIVATE, WM_APP_ENGINE, WM_APP_TRAY};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::Shell::NIN_SELECT;

/// NIN_SELECT | NINF_KEY (Enter/Space on the focused tray icon).
const NIN_KEYSELECT: u32 = 0x0401;
use windows::Win32::UI::WindowsAndMessaging::*;

// Context-menu command ids.
const CMD_EXIT: u32 = 1;
const CMD_OPEN_PANEL: u32 = 10;
const CMD_SETTINGS: u32 = 11;
const CMD_RESET: u32 = 2;
/// Warmth presets: CMD_KELVIN_BASE + index into `color::PRESETS`.
const CMD_KELVIN_BASE: u32 = 100;
/// Brightness presets: CMD_BRIGHTNESS_BASE + index into `BRIGHTNESS_STEPS`.
const CMD_BRIGHTNESS_BASE: u32 = 200;
/// Scenes: CMD_SCENE_BASE + index into `settings.scenes`.
const CMD_SCENE_BASE: u32 = 300;
const CMD_PAUSE_HOUR: u32 = 3;
const CMD_PAUSE_FOREVER: u32 = 4;
const CMD_RESUME: u32 = 5;
const CMD_SCHEDULE_TOGGLE: u32 = 6;
const CMD_SCHEDULE_RESUME: u32 = 7;
/// Color filters: CMD_FILTER_BASE + index into `Filter::ALL`.
const CMD_FILTER_BASE: u32 = 400;
const CMD_MOVIE: u32 = 9;
/// Movie mode length (PRODUCT §5.4).
const MOVIE_MINUTES: u64 = 150;
const MOVIE_DEFAULT_K: u32 = 3400;
/// Schedule-driven warmth changes smaller than this are skipped (invisible, saves gamma writes).
const SCHEDULE_MIN_STEP_K: u32 = 25;

/// Hotkey ids: 1 + index into `model::HOTKEY_ACTIONS`; scene hotkeys: HOTKEY_SCENE_BASE + scene index.
const HOTKEY_SCENE_BASE: i32 = 100;
const BRIGHTNESS_STEP: f32 = 5.0;
const KELVIN_STEP: i64 = 250;
const PANIC_PAUSE_MINUTES: u64 = 60;
const BRIGHTNESS_STEPS: &[f32] = &[100.0, 80.0, 60.0, 40.0, 25.0, 15.0, 10.0, 5.0, 2.0];

// Timer ids.
const TIMER_DISPLAY_CHANGE: usize = 1;
const DISPLAY_CHANGE_DEBOUNCE_MS: u32 = 500;
const TIMER_SAVE: usize = 2;
const SAVE_DELAY_MS: u32 = 1000;
/// Detects other programs (games, drivers, Night Light) overwriting our ramps.
const TIMER_GAMMA_CHECK: usize = 3;
const GAMMA_CHECK_MS: u32 = 5000;
/// Deep-dim confirmation countdown (1 s).
const TIMER_DEEP_DIM: usize = 6;
/// Watches whether the pointer left the tray icon (to remove the wheel hook).
const TIMER_WHEEL_HOOK: usize = 7;
/// Below this brightness the first use on a monitor must be confirmed (PRODUCT §12).
const DEEP_DIM: f32 = 5.0;
const DEEP_DIM_SECONDS: u32 = 10;
/// Posted by the tray wheel hook; wParam = wheel delta (i16).
const WM_APP_TRAY_WHEEL: u32 = 0x8000 + 11;
/// Ends a warmth preview from the settings window.
const TIMER_PREVIEW: usize = 8;
const PREVIEW_MS: u32 = 5000;
/// Trims the working set once things are idle (after startup, and after closing a window).
const TIMER_TRIM: usize = 9;
const TRIM_DELAY_MS: u32 = 3000;
/// Eye break: 20 minutes of activity, 20 seconds of looking away (PRODUCT v1.2).
const EYE_WORK_MS: u64 = 20 * 60_000;
const EYE_BREAK_SECONDS: u32 = 20;
/// An eye break due this close before a computer break is skipped (the computer break wins).
const EYE_BEFORE_COMPUTER_MS: u64 = 2 * 60_000;
/// A pause in input this long counts as a natural break.
const NATURAL_BREAK_MS: u64 = 5 * 60_000;
const TIMER_BREAK: usize = 11;
/// Light sensor polling while automatic brightness is on.
const TIMER_AMBIENT: usize = 12;
/// Shortly after the first start, offer to lift Windows' color-range limit (once).
const TIMER_RANGE_OFFER: usize = 13;
const RANGE_OFFER_DELAY_MS: u32 = 1500;
const AMBIENT_POLL_MS: u32 = 2000;
/// The tooltip counts down to bedtime in the last hours.
const BEDTIME_TOOLTIP_MIN: f64 = 180.0;

/// Idle check: slow poll while active, fast steps while fading / waiting for input.
const TIMER_IDLE: usize = 10;
const IDLE_POLL_MS: u32 = 5000;
const IDLE_STEP_MS: u32 = 100;
const IDLE_WATCH_MS: u32 = 250;
/// Fade step per IDLE_STEP_MS (2 s from full to dimmed).
const IDLE_FADE_STEP: f32 = 0.05;

/// Periodic housekeeping (pause expiry, schedule).
const TIMER_TICK: usize = 5;
const TICK_MS: u32 = 30_000;
/// After resume from sleep, monitors need a moment before DDC/CI and gamma stick.
const TIMER_RESUME: usize = 4;
const RESUME_DELAY_MS: u32 = 3000;
/// Allowed difference when comparing ramps read back from the driver (LUT precision).
const RAMP_TOLERANCE: u16 = 512;

/// Runtime state of one monitor.
struct Screen {
    mon: Monitor,
    /// Settings key (hash of the stable monitor id).
    key: String,
    /// Effects disabled for this monitor by the user.
    enabled: bool,
    /// User brightness 1–100.
    brightness: f32,
    /// Percent of the slider driven by hardware (PRODUCT §4.1).
    hw_share: f32,
    /// Hardware capabilities once probed (`None` = software only).
    hw: Option<hardware::Caps>,
    /// Until the first probe finishes, a monitor that had hardware control last time is assumed to
    /// still have it (avoids briefly dimming twice at startup).
    hw_assumed: bool,
    /// Last hardware level sent to the worker.
    hw_sent: Option<f32>,
    /// Last gamma job sent (kelvin, requested scale).
    gamma_sent: Option<(u32, f32)>,
    /// Scale the ramp achieves (predicted until the worker confirms).
    gamma_scale: f32,
    /// Learned deviation bound for predictions.
    gamma_bound: f32,
    /// Fingerprint of the ramp the worker applied (to detect other apps overwriting it).
    gamma_expected: Option<[u16; 6]>,
    /// Windows limited the last ramp (shown in the Displays page).
    gamma_limited: bool,
}

impl Screen {
    fn new(
        mon: Monitor,
        key: String,
        brightness: f32,
        hw_share: f32,
        enabled: bool,
        hw_assumed: bool,
    ) -> Self {
        Screen {
            hw_assumed,
            mon,
            key,
            enabled,
            brightness,
            hw_share,
            hw: None,
            hw_sent: None,
            gamma_sent: None,
            gamma_scale: 1.0,
            gamma_bound: gamma::DEFAULT_BOUND,
            gamma_expected: None,
            gamma_limited: false,
        }
    }
}

pub struct App {
    hwnd: HWND,
    tray: Option<Tray>,
    msg_taskbar_created: u32,
    screens: Vec<Screen>,
    /// Monitor-list generation; results from workers for an older list are ignored.
    gen: u64,
    overlays: Overlays,
    gamma_worker: Option<gamma::Worker>,
    hw_worker: Option<hardware::Worker>,
    events: Option<Events>,
    /// Effective warmth being shown.
    kelvin: u32,
    settings: Settings,
    paths: Paths,
    osd: Option<Box<Osd>>,
    /// Pause end (GetTickCount64 ms); `Some(u64::MAX)` = until resumed.
    paused_until: Option<u64>,
    scene_index: usize,
    /// Hotkeys that could not be registered (shown in the tooltip / settings).
    hotkey_conflicts: Vec<String>,
    /// Manual warmth that overrides the schedule until its phase changes (like f.lux).
    override_k: Option<(u32, schedule::Phase)>,
    /// Night factor (0–1) from the schedule, for the optional night brightness ceiling.
    night: f32,
    flyout: Option<Box<Flyout>>,
    /// Seconds left to confirm a very low brightness.
    deep_dim_left: Option<u32>,
    active_scene: Option<usize>,
    wheel_hook: Option<windows::Win32::UI::WindowsAndMessaging::HHOOK>,
    settings_win: Option<Box<SettingsWindow>>,
    /// Temporary warmth shown while scrubbing the schedule timeline or a color slider.
    preview_k: Option<u32>,
    /// Global hotkeys are suspended while the settings window captures a new shortcut.
    capturing: bool,
    magnifier: engine::magnify::Magnifier,
    /// Full-screen color filter (Darkroom, Grayscale, Amber, Red).
    filter: Filter,
    /// Movie mode: (end tick, warmth).
    movie: Option<(u64, u32)>,
    watcher: foreground::Watcher,
    /// Rule of the app currently in the foreground: (exe, action).
    rule: Option<(String, RuleAction)>,
    /// A fullscreen app is in the foreground and "pause in fullscreen" is on.
    fullscreen: bool,
    /// Recently seen foreground apps (for adding rules in Settings).
    recent_apps: Vec<String>,
    /// Idle dimming progress: 0 = normal, 1 = fully dimmed.
    idle_fade: f32,
    /// Start of the current stretch of continuous activity (for eye breaks).
    active_since: u64,
    /// Start of the current stretch of activity (for computer breaks).
    work_since: u64,
    /// The running break and its seconds left.
    break_left: Option<(Break, u32)>,
    /// When the bedtime reminder last showed (at most once per 12 h).
    bedtime_shown: u64,
    cursor: engine::cursor::CursorDimmer,
    /// Light sensor (opened while automatic brightness is on).
    sensor: Option<crate::ambient::Sensor>,
    /// Smoothed illuminance.
    lux: Option<f32>,
    /// The sensor (not the user) is changing brightness right now.
    ambient_driving: bool,
}

thread_local! {
    /// Tray icon rectangle and controller window for the low-level wheel hook.
    static WHEEL_TARGET: std::cell::Cell<(RECT, isize)> = const { std::cell::Cell::new((RECT { left: 0, top: 0, right: 0, bottom: 0 }, 0)) };
}

unsafe extern "system" fn wheel_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp.0 as u32 == WM_MOUSEWHEEL {
        let info = &*(lp.0 as *const MSLLHOOKSTRUCT);
        let (r, hwnd) = WHEEL_TARGET.with(|t| t.get());
        let p = info.pt;
        if hwnd != 0 && p.x >= r.left && p.x < r.right && p.y >= r.top && p.y < r.bottom {
            let delta = (info.mouseData >> 16) as u16 as i16;
            win::post(HWND(hwnd as *mut _), WM_APP_TRAY_WHEEL, delta as u16 as usize, 0);
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

/// A program to start after the message loop ends (install / uninstall from the settings window).
static RELAUNCH: std::sync::Mutex<Option<(std::path::PathBuf, Vec<String>)>> = std::sync::Mutex::new(None);

pub fn take_relaunch() -> Option<(std::path::PathBuf, Vec<String>)> {
    RELAUNCH.lock().ok()?.take()
}

/// The color filter a scene effect turns on (`Filter::None` for other effects).
fn scene_filter(e: SceneEffect) -> Filter {
    match e {
        SceneEffect::Darkroom => Filter::Darkroom,
        SceneEffect::Grayscale => Filter::Grayscale,
        SceneEffect::Amber => Filter::Amber,
        SceneEffect::Red => Filter::Red,
        SceneEffect::None | SceneEffect::Movie => Filter::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Break {
    /// 20-20-20: look into the distance for 20 seconds.
    Eye,
    /// Step away from the computer for a few minutes.
    Computer,
}

impl Break {
    fn title(self) -> &'static str {
        match self {
            Break::Eye => "Eye break",
            Break::Computer => "Computer break",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Due {
    Nothing,
    Start(Break),
    /// An eye break is due, but a computer break follows shortly: skip it.
    SkipEye,
}

/// Which break (if any) starts now. `active_since` / `work_since` start the eye / computer stretches.
fn break_due(s: &Settings, now: u64, active_since: u64, work_since: u64, running: Option<Break>) -> Due {
    if running == Some(Break::Computer) {
        return Due::Nothing;
    }
    let computer_at = work_since + s.break_every as u64 * 60_000;
    if s.computer_breaks && now >= computer_at {
        // The computer break wins, even over a running eye break.
        return Due::Start(Break::Computer);
    }
    if !s.eye_breaks || running.is_some() || now.saturating_sub(active_since) < EYE_WORK_MS {
        return Due::Nothing;
    }
    if s.computer_breaks && computer_at - now <= EYE_BEFORE_COMPUTER_MS {
        Due::SkipEye
    } else {
        Due::Start(Break::Eye)
    }
}

fn break_content(kind: Break, seconds: u32) -> osd::Content {
    let text = match kind {
        Break::Eye => format!("Look at something ~6 m away · {seconds} s"),
        Break::Computer => format!("Step away from the screen · {}:{:02}", seconds / 60, seconds % 60),
    };
    osd::Content::Message(kind.title().into(), text)
}

/// "1 h 20 min" / "45 min".
fn fmt_minutes(m: f64) -> String {
    let m = m.round() as u32;
    if m >= 60 {
        format!("{} h {:02} min", m / 60, m % 60)
    } else {
        format!("{m} min")
    }
}

fn rule_text(a: &RuleAction) -> String {
    match a {
        RuleAction::Disable => "effects off".into(),
        RuleAction::NoOverlay => "no overlay".into(),
        RuleAction::Scene(s) => format!("scene {s}"),
    }
}

fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

impl App {
    /// Creates the controller window and tray icon. The returned box must stay alive for
    /// the duration of the message loop (its address is stored in the window).
    pub fn create(paths: Paths, recovered: bool) -> windows::core::Result<Box<App>> {
        let settings = Ini::load(&paths.settings).map(|i| Settings::from_ini(&i)).unwrap_or_default();
        info!("settings: {} (portable={})", paths.settings.display(), paths.portable);
        let hinst = win::hinstance();
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: hinst,
                lpszClassName: CONTROLLER_CLASS,
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut app = Box::new(App {
                hwnd: HWND::default(),
                tray: None,
                msg_taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
                screens: Vec::new(),
                gen: 0,
                overlays: Overlays::new(),
                gamma_worker: None,
                hw_worker: None,
                events: None,
                kelvin: settings.kelvin,
                osd: Osd::create(theme::palette(settings.theme)),
                settings,
                paths,
                paused_until: None,
                scene_index: 0,
                hotkey_conflicts: Vec::new(),
                override_k: None,
                night: 0.0,
                flyout: None,
                deep_dim_left: None,
                active_scene: None,
                wheel_hook: None,
                settings_win: None,
                preview_k: None,
                capturing: false,
                magnifier: Default::default(),
                filter: Filter::None,
                movie: None,
                watcher: Default::default(),
                rule: None,
                fullscreen: false,
                recent_apps: Vec::new(),
                idle_fade: 0.0,
                active_since: now_ms(),
                work_since: now_ms(),
                break_left: None,
                bedtime_shown: 0,
                cursor: Default::default(),
                sensor: None,
                lux: None,
                ambient_driving: false,
            });
            // A hidden top-level window (not message-only) so that broadcasts such as
            // WM_DISPLAYCHANGE, WM_SETTINGCHANGE and TaskbarCreated reach us.
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                CONTROLLER_CLASS,
                w!("SLC"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(hinst),
                Some(&mut *app as *mut App as *const core::ffi::c_void),
            )?;
            app.hwnd = hwnd;
            let events = Events::new(hwnd, WM_APP_ENGINE);
            app.gamma_worker = Some(gamma::Worker::start(events.clone()));
            app.hw_worker = Some(hardware::Worker::start(events.clone()));
            app.events = Some(events);
            theme::apply_menu_theme(app.settings.theme);
            app.tray = Some(Tray::new(hwnd));
            app.flyout = Flyout::create(hwnd, theme::palette(app.settings.theme));
            app.update_schedule(true);
            app.refresh_monitors();
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            SetTimer(Some(hwnd), TIMER_GAMMA_CHECK, GAMMA_CHECK_MS, None);
            SetTimer(Some(hwnd), TIMER_TICK, TICK_MS, None);
            SetTimer(Some(hwnd), TIMER_TRIM, TRIM_DELAY_MS, None);
            app.register_hotkeys();
            app.update_watcher();
            app.update_idle_timer();
            app.update_ambient();
            SetTimer(Some(hwnd), TIMER_RANGE_OFFER, RANGE_OFFER_DELAY_MS, None);
            if crate::install::night_light_on() {
                info!("Windows Night Light is on");
                if let Some(t) = &app.tray {
                    t.notify(
                        "Windows Night Light is on",
                        "It changes screen colors too and will fight with SLC. Turn it off in Settings › System › Display.",
                    );
                }
            }
            if recovered {
                if let Some(t) = &app.tray {
                    t.notify(
                        "Screen Lighting Control",
                        "SLC did not close properly last time, so your screens were reset to neutral colors.",
                    );
                }
            }
            if let Some(r) = app.tray.as_ref().and_then(|t| t.rect()) {
                info!("tray icon at {},{} {}x{}", r.left, r.top, r.right - r.left, r.bottom - r.top);
            }
            info!("controller window created");
            Ok(app)
        }
    }

    fn refresh_monitors(&mut self) {
        self.sync_settings();
        self.gen += 1;
        let settings = &mut self.settings;
        self.screens = monitors::enumerate()
            .into_iter()
            .map(|m| {
                let key = monitors::settings_key(&m.id);
                let ms = settings.monitor_mut(&key);
                ms.name = m.name.clone();
                let (b, h, e, a) = (ms.brightness, ms.hw_share, ms.enabled, ms.original_hw.is_some());
                Screen::new(m, key, b, h, e, a)
            })
            .collect();
        self.overlays.resize(self.screens.len());
        for s in &self.screens {
            info!(
                "monitor {} '{}' key={} internal={} hdr={}",
                s.mon.device, s.mon.name, s.key, s.mon.internal, s.mon.hdr
            );
        }
        if let Some(w) = &self.hw_worker {
            w.probe(self.gen, self.screens.iter().map(|s| (s.mon.hmon.0 as isize, s.mon.internal)).collect());
        }
        self.apply();
    }

    /// Copies runtime per-monitor values into the settings model.
    fn sync_settings(&mut self) {
        for s in &self.screens {
            let ms = self.settings.monitor_mut(&s.key);
            ms.brightness = s.brightness;
            ms.hw_share = s.hw_share;
            ms.enabled = s.enabled;
        }
    }

    /// Something the user changed: apply it and save soon.
    fn commit(&mut self) {
        self.apply();
        unsafe { SetTimer(Some(self.hwnd), TIMER_SAVE, SAVE_DELAY_MS, None) };
    }

    fn save(&mut self) {
        self.sync_settings();
        if let Err(e) = self.settings.to_ini().save(&self.paths.settings) {
            info!("saving settings failed: {e}");
        }
    }

    /// Forgets what was sent to the devices so the next `apply` re-sends everything.
    fn invalidate(&mut self) {
        for s in &mut self.screens {
            s.gamma_sent = None;
            s.gamma_expected = None;
        }
    }

    /// Re-applies our ramp on any monitor where another program replaced it.
    fn check_gamma(&mut self) {
        let mut changed = false;
        for s in &mut self.screens {
            let Some(expected) = s.gamma_expected else { continue };
            let Some(now) = gamma::read(&s.mon.device) else { continue };
            let now = gamma::fingerprint(&now);
            if now.iter().zip(expected).any(|(a, b)| a.abs_diff(b) > RAMP_TOLERANCE) {
                info!("gamma on {} was changed by another program; re-applying", s.mon.device);
                s.gamma_sent = None;
                s.gamma_expected = None;
                changed = true;
            }
        }
        if changed {
            self.apply();
        }
    }

    /// Average brightness across monitors (what the master control shows).
    fn master(&self) -> f32 {
        if self.screens.is_empty() {
            return engine::MAX_BRIGHTNESS;
        }
        self.screens.iter().map(|s| s.brightness).sum::<f32>() / self.screens.len() as f32
    }

    /// Moves every monitor by the same amount so the average becomes `value`
    /// (keeps relative offsets until a monitor hits a limit).
    fn update_ambient(&mut self) {
        if self.settings.ambient {
            if self.sensor.is_none() {
                self.sensor = crate::ambient::Sensor::open();
                info!("light sensor: {}", if self.sensor.is_some() { "found" } else { "none" });
            }
            if self.sensor.is_some() {
                unsafe { SetTimer(Some(self.hwnd), TIMER_AMBIENT, AMBIENT_POLL_MS, None) };
                self.on_ambient_tick();
                return;
            }
        }
        self.sensor = None;
        self.lux = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_AMBIENT);
        }
    }

    fn on_ambient_tick(&mut self) {
        let Some(reading) = self.sensor.as_ref().and_then(|s| s.lux()) else { return };
        let lux = crate::ambient::smooth(self.lux, reading);
        self.lux = Some(lux);
        if self.paused_until.is_some() || self.rule.is_some() {
            return;
        }
        let target = (crate::ambient::curve(lux) + self.settings.ambient_offset)
            .clamp(engine::MIN_BRIGHTNESS.max(5.0), engine::MAX_BRIGHTNESS);
        if (target - self.master()).abs() >= crate::ambient::HYSTERESIS {
            self.ambient_driving = true;
            self.set_master(target);
            self.ambient_driving = false;
            self.commit();
        }
    }

    fn set_master(&mut self, value: f32) {
        let delta = value.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS) - self.master();
        for s in &mut self.screens {
            s.brightness = (s.brightness + delta).clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS);
        }
        // A manual change while the sensor drives brightness teaches the offset (like phones do).
        if self.settings.ambient && !self.ambient_driving {
            if let Some(lux) = self.lux {
                self.settings.ambient_offset = (value - crate::ambient::curve(lux)).clamp(-50.0, 50.0);
            }
        }
        // Setting an extreme should land every monitor exactly on it.
        if value >= engine::MAX_BRIGHTNESS || value <= engine::MIN_BRIGHTNESS {
            let v = value.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS);
            self.screens.iter_mut().for_each(|s| s.brightness = v);
        }
    }

    fn set_brightness(&mut self, monitor: Option<usize>, value: f32) {
        match monitor {
            Some(i) => {
                if let Some(s) = self.screens.get_mut(i) {
                    s.brightness = value.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS);
                }
            }
            None => self.set_master(value),
        }
    }

    /// Pushes the current state to every monitor: split → hardware / gamma (async) → overlay.
    #[allow(clippy::needless_range_loop)] // indexes screens, overlays and workers in lockstep
    fn apply(&mut self) {
        let rule_off = matches!(self.rule, Some((_, RuleAction::Disable))) || self.fullscreen;
        let paused = self.paused_until.is_some() || rule_off;
        let no_overlay = matches!(self.rule, Some((_, RuleAction::NoOverlay)));
        let rule_scene = match &self.rule {
            Some((_, RuleAction::Scene(name))) => {
                self.settings.scenes.iter().find(|s| &s.name == name).cloned()
            }
            _ => None,
        };
        let scene_b = rule_scene.as_ref().and_then(|s| s.brightness);
        let scene_k = rule_scene.as_ref().and_then(|s| s.kelvin);
        let ceilings: Vec<f32> =
            self.screens.iter().map(|s| self.effective_brightness(scene_b.unwrap_or(s.brightness))).collect();
        let mut max_alpha: f32 = 0.0;
        for i in 0..self.screens.len() {
            let s = &mut self.screens[i];
            // A disabled monitor gets neutral software effects (hardware is left alone).
            let active = s.enabled && !paused;
            let (brightness, kelvin) = if active {
                (ceilings[i], self.preview_k.or(scene_k).unwrap_or(self.kelvin))
            } else {
                (engine::MAX_BRIGHTNESS, color::NEUTRAL_KELVIN)
            };
            let white = color::white_point(kelvin);
            let has_hw = (s.hw.is_some() || s.hw_assumed) && active;
            let split = engine::split(brightness, s.hw_share, has_hw);
            if let (Some(level), Some(w), false) = (split.hardware, &self.hw_worker, s.hw_assumed) {
                if s.hw_sent.is_none_or(|l| (l - level).abs() > 0.01) {
                    w.set(self.gen, i, level);
                    s.hw_sent = Some(level);
                }
            }
            // HDR displays ignore or misapply gamma ramps: hardware + overlay only (PRODUCT §13).
            let want = if s.mon.hdr { (color::NEUTRAL_KELVIN, 1.0) } else { (kelvin, split.software) };
            if s.gamma_sent != Some(want) {
                if let Some(w) = &self.gamma_worker {
                    w.submit(
                        i,
                        gamma::Job {
                            gen: self.gen,
                            device: s.mon.device.clone(),
                            kelvin: want.0,
                            scale: want.1,
                        },
                    );
                }
                // Predict what Windows will accept so the overlay is right immediately.
                s.gamma_scale = gamma::plan(white, split.software, s.gamma_bound - 0.002).0;
                s.gamma_sent = Some(want);
            }
            let alpha = if no_overlay { 0.0 } else { engine::overlay_alpha(split.software, s.gamma_scale) };
            max_alpha = max_alpha.max(alpha);
            self.overlays.set(i, s.mon.rect, alpha);
        }
        let mut tip = if let Some((exe, action)) = &self.rule {
            format!("SLC — rule for {exe}: {}", rule_text(action))
        } else if self.fullscreen {
            "SLC — paused while a fullscreen app is active".to_string()
        } else if paused {
            "SLC — paused".to_string()
        } else {
            format!("SLC — {:.0}% · {}K {}", self.master(), self.kelvin, color::preset_name(self.kelvin))
        };
        if !self.hotkey_conflicts.is_empty() {
            tip.push_str(&format!("\nHotkey in use: {}", self.hotkey_conflicts.join(", ")));
        }
        if self.settings.schedule.enabled {
            let (_, _, t, _) = schedule::now_local();
            let mins = schedule::minutes_to_bedtime(&self.settings.schedule, t);
            if mins <= BEDTIME_TOOLTIP_MIN {
                tip.push_str(&format!("\nBedtime in {}", fmt_minutes(mins)));
            }
        }
        // The cursor sits above the overlay: darken it by the same amount when asked to.
        self.cursor.set((self.settings.dim_cursor && max_alpha > 0.0).then_some(1.0 - max_alpha));
        let dark = self.filter != Filter::None && !paused;
        if !self.magnifier.set(if paused { None } else { self.filter.matrix() }) && dark {
            let name = self.filter.name();
            self.filter = Filter::None;
            self.show_osd(osd::Content::Message(
                format!("{name} unavailable"),
                "Windows refused the color filter".into(),
            ));
        }
        if let Some(t) = self.tray.as_mut() {
            t.set_tip(&tip);
            t.set_glyph(if paused {
                crate::icon::Glyph::Paused
            } else if dark {
                crate::icon::Glyph::Darkroom
            } else {
                crate::icon::Glyph::Normal
            });
        }
        self.update_flyout();
        self.update_settings_window();
    }

    /// Results from the worker threads.
    fn on_engine_events(&mut self) {
        let Some(events) = self.events.clone() else { return };
        let mut reapply = false;
        for e in events.drain() {
            match e {
                Event::Gamma { gen, index, kelvin, scale, applied, bound } if gen == self.gen => {
                    let Some(s) = self.screens.get_mut(index) else { continue };
                    s.gamma_bound = bound;
                    if s.gamma_sent == Some((kelvin, scale)) {
                        s.gamma_expected = (!applied.failed).then(|| {
                            gamma::fingerprint(&color::build_ramp(
                                color::white_point(kelvin),
                                applied.scale,
                                applied.warmth,
                            ))
                        });
                        let got = if applied.failed { 1.0 } else { applied.scale };
                        if (got - s.gamma_scale).abs() > 0.001 {
                            s.gamma_scale = got;
                            reapply = true;
                        }
                    }
                    s.gamma_limited = applied.limited;
                    if applied.limited || applied.failed {
                        info!(
                            "gamma on {}: warmth {:.2} scale {:.2} failed={}",
                            s.mon.device, applied.warmth, applied.scale, applied.failed
                        );
                    }
                }
                Event::HardwareProbed { gen, caps } if gen == self.gen => {
                    for (s, c) in self.screens.iter_mut().zip(caps) {
                        s.hw = c;
                        s.hw_assumed = false;
                        s.hw_sent = c.map(|c| c.current);
                        let Some(c) = c else { continue };
                        let ms = self.settings.monitor_mut(&s.key);
                        if ms.original_hw.is_none() {
                            // First sight of this monitor: remember its backlight and adopt it as our
                            // brightness, so starting SLC never changes the screen.
                            ms.original_hw = Some(c.current);
                            let knee = engine::MAX_BRIGHTNESS - s.hw_share;
                            s.brightness = knee + c.current / 100.0 * s.hw_share;
                            info!(
                                "adopted {} backlight {:.0}% as brightness {:.0}",
                                s.mon.name, c.current, s.brightness
                            );
                            unsafe { SetTimer(Some(self.hwnd), TIMER_SAVE, SAVE_DELAY_MS, None) };
                        }
                    }
                    reapply = true;
                }
                Event::HardwareSet { gen, index, ok } if gen == self.gen && !ok => {
                    info!("hardware brightness write failed on monitor {index}");
                }
                _ => {}
            }
        }
        if reapply {
            self.apply();
        }
    }

    /// Stops the workers and removes every software effect (synchronously).
    fn shutdown(&mut self) {
        self.save();
        hotkeys::unregister_all(self.hwnd, self.hotkey_ids());
        if let Some(h) = self.wheel_hook.take() {
            unsafe {
                let _ = UnhookWindowsHookEx(h);
            }
        }
        self.magnifier.set(None);
        self.cursor.set(None);
        self.osd = None;
        self.flyout = None;
        self.settings_win = None;
        self.gamma_worker = None;
        self.hw_worker = None;
        self.overlays.clear();
        let mons: Vec<Monitor> = self.screens.iter().map(|s| s.mon.clone()).collect();
        engine::reset_all(&mons);
        crate::safety::end_session();
    }

    fn register_hotkeys(&mut self) {
        let mut entries: Vec<(i32, String, bool)> = model::HOTKEY_ACTIONS
            .iter()
            .enumerate()
            .map(|(i, (action, _, _))| {
                let repeat = matches!(*action, "brightness_up" | "brightness_down" | "warmer" | "cooler");
                (i as i32 + 1, self.settings.hotkey(action).to_string(), repeat)
            })
            .collect();
        // The panic hotkey goes first so it wins any conflict with our own bindings.
        entries.sort_by_key(|(id, _, _)| model::HOTKEY_ACTIONS[*id as usize - 1].0 != "panic");
        for (i, sc) in self.settings.scenes.iter().enumerate() {
            entries.push((HOTKEY_SCENE_BASE + i as i32, sc.hotkey.clone(), false));
        }
        let failed = hotkeys::register_all(self.hwnd, &entries);
        self.hotkey_conflicts =
            failed.iter().filter_map(|id| entries.iter().find(|e| e.0 == *id).map(|e| e.1.clone())).collect();
        info!("hotkeys registered ({} conflicts)", self.hotkey_conflicts.len());
    }

    fn on_hotkey(&mut self, id: i32) {
        if id >= HOTKEY_SCENE_BASE {
            self.apply_scene((id - HOTKEY_SCENE_BASE) as usize);
            return;
        }
        let Some((action, _, _)) = model::HOTKEY_ACTIONS.get((id - 1) as usize) else { return };
        match *action {
            "brightness_up" | "brightness_down" => {
                let step = if *action == "brightness_up" { BRIGHTNESS_STEP } else { -BRIGHTNESS_STEP };
                // Snap to the step grid so repeated presses land on round numbers.
                let target = ((self.master() + step) / BRIGHTNESS_STEP).round() * BRIGHTNESS_STEP;
                self.resume_if_paused();
                self.set_master(target);
                self.active_scene = None;
                self.commit();
                self.show_osd(osd::Content::Brightness(self.master()));
                self.check_deep_dim();
            }
            "warmer" | "cooler" => {
                let step = if *action == "warmer" { -KELVIN_STEP } else { KELVIN_STEP };
                self.resume_if_paused();
                self.set_kelvin(color::clamp_kelvin(self.kelvin as i64 + step));
                self.commit();
                self.show_osd(osd::Content::Warmth(self.kelvin));
            }
            "pause" => {
                if self.paused_until.is_some() {
                    self.resume();
                } else {
                    self.pause(None);
                }
            }
            "panic" => self.panic(),
            "darkroom" => {
                if let Some(i) = self.settings.scenes.iter().position(|s| s.effect == SceneEffect::Darkroom) {
                    self.apply_scene(i);
                }
            }
            "next_scene" | "prev_scene" => {
                let n = self.settings.scenes.len();
                if n > 0 {
                    self.scene_index = if *action == "next_scene" {
                        (self.scene_index + 1) % n
                    } else {
                        (self.scene_index + n - 1) % n
                    };
                    self.apply_scene(self.scene_index);
                }
            }
            _ => {}
        }
    }

    fn show_osd(&mut self, content: osd::Content) {
        if !self.settings.osd {
            return;
        }
        if let Some(o) = self.osd.as_mut() {
            o.set_palette(theme::palette(self.settings.theme));
            o.show(content);
        }
    }

    /// A manual warmth change. With the schedule on it becomes an override until the next phase.
    fn set_kelvin(&mut self, k: u32) {
        self.movie = None;
        self.kelvin = k;
        self.settings.kelvin = k;
        if self.settings.schedule.enabled {
            let phase = schedule::target_now(&self.settings.schedule).phase;
            self.override_k = Some((k, phase));
            info!("warmth override {k}K during {phase:?}");
        }
    }

    /// Follows the schedule (called on the tick). Returns true if something visible changed.
    fn update_schedule(&mut self, force: bool) -> bool {
        if let Some((until, k)) = self.movie {
            if now_ms() < until {
                let changed = self.kelvin != k;
                self.kelvin = k;
                return changed;
            }
            info!("movie mode ended");
            self.movie = None;
            if self.settings.schedule.enabled {
                self.override_k = None;
            }
        }
        let sc = &self.settings.schedule;
        if !sc.enabled {
            self.override_k = None;
            let changed = self.kelvin != self.settings.kelvin || self.night != 0.0;
            self.kelvin = self.settings.kelvin;
            self.night = 0.0;
            return changed;
        }
        let t = schedule::target_now(sc);
        if let Some((_, phase)) = self.override_k {
            if phase != t.phase {
                info!("schedule phase changed to {:?}; override ended", t.phase);
                self.override_k = None;
            }
        }
        let k = self.override_k.map(|o| o.0).unwrap_or(t.kelvin);
        let night_changed = (t.night - self.night).abs() >= 0.02 || (t.night == 0.0) != (self.night == 0.0);
        let k_changed = self.kelvin.abs_diff(k) >= SCHEDULE_MIN_STEP_K || (force && self.kelvin != k);
        if night_changed {
            self.night = t.night;
        }
        if k_changed {
            self.kelvin = k;
        }
        k_changed || night_changed
    }

    fn update_idle_timer(&mut self) {
        unsafe {
            if self.settings.idle_dim {
                SetTimer(Some(self.hwnd), TIMER_IDLE, IDLE_POLL_MS, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), TIMER_IDLE);
            }
        }
        if !self.settings.idle_dim && self.idle_fade > 0.0 {
            self.idle_fade = 0.0;
            self.apply();
        }
    }

    fn on_idle_timer(&mut self) {
        let idle = crate::idle::idle_ms();
        let threshold = self.settings.idle_minutes as u64 * 60_000;
        let next = if self.idle_fade > 0.0 && idle < IDLE_WATCH_MS as u64 * 2 {
            // Input: back to normal at once.
            info!("input after idle; restoring brightness");
            self.idle_fade = 0.0;
            self.apply();
            IDLE_POLL_MS
        } else if idle >= threshold
            && self.paused_until.is_none()
            && !self.fullscreen
            && !crate::idle::audio_playing()
            && !foreground::is_fullscreen(unsafe { GetForegroundWindow() })
        {
            if self.idle_fade < 1.0 {
                if self.idle_fade == 0.0 {
                    info!("idle for {} s; dimming", idle / 1000);
                }
                self.idle_fade = (self.idle_fade + IDLE_FADE_STEP).min(1.0);
                self.apply();
                IDLE_STEP_MS
            } else {
                IDLE_WATCH_MS
            }
        } else if self.idle_fade > 0.0 {
            IDLE_WATCH_MS
        } else {
            IDLE_POLL_MS
        };
        unsafe { SetTimer(Some(self.hwnd), TIMER_IDLE, next, None) };
    }

    /// Brightness actually shown for a user value (applies the scheduled night ceiling and idle dimming).
    fn effective_brightness(&self, b: f32) -> f32 {
        let b = if self.idle_fade > 0.0 {
            b.min(crate::idle::ceiling(self.settings.idle_level, self.idle_fade))
        } else {
            b
        };
        match self.settings.schedule.night_brightness {
            Some(nb) if self.settings.schedule.enabled && self.night > 0.0 => {
                b.min(engine::MAX_BRIGHTNESS + (nb - engine::MAX_BRIGHTNESS) * self.night)
            }
            _ => b,
        }
    }

    fn apply_scene(&mut self, index: usize) {
        let Some(scene) = self.settings.scenes.get(index).cloned() else { return };
        self.scene_index = index;
        self.active_scene = Some(index);
        self.resume_if_paused();
        if let Some(b) = scene.brightness {
            self.set_master(b);
        }
        if let (Some(k), false) = (scene.kelvin, scene.effect == SceneEffect::Movie) {
            self.stop_movie();
            self.set_kelvin(k);
        }
        match scene.effect {
            SceneEffect::Movie => self.start_movie(scene.kelvin.unwrap_or(MOVIE_DEFAULT_K)),
            SceneEffect::None => self.filter = Filter::None,
            e => {
                let f = scene_filter(e);
                self.filter = if self.filter == f { Filter::None } else { f };
                info!("color filter: {:?}", self.filter);
            }
        }
        self.commit();
        let sub = match scene.effect {
            SceneEffect::Darkroom | SceneEffect::Grayscale | SceneEffect::Amber | SceneEffect::Red => {
                if self.filter == scene_filter(scene.effect) {
                    "On".to_string()
                } else {
                    "Off".to_string()
                }
            }
            SceneEffect::Movie => {
                format!("{}K for {} h {} min", self.kelvin, MOVIE_MINUTES / 60, MOVIE_MINUTES % 60)
            }
            SceneEffect::None => String::new(),
        };
        let sub = if !sub.is_empty() {
            sub
        } else {
            match (scene.brightness, scene.kelvin) {
                (Some(b), Some(k)) => format!("{b:.0}% · {k}K"),
                (None, Some(k)) => format!("{k}K"),
                (Some(b), None) => format!("{b:.0}%"),
                (None, None) => String::new(),
            }
        };
        if !self.flyout.as_ref().is_some_and(|f| f.visible()) {
            self.show_osd(osd::Content::Message(scene.name, sub));
        }
        self.check_deep_dim();
    }

    fn start_movie(&mut self, k: u32) {
        self.movie = Some((now_ms() + MOVIE_MINUTES * 60_000, k));
        self.kelvin = k;
        info!("movie mode {k}K for {MOVIE_MINUTES} min");
    }

    fn stop_movie(&mut self) {
        if self.movie.take().is_some() {
            self.update_schedule(true);
        }
    }

    /// Pauses all effects for `minutes` (None = until resumed).
    fn pause(&mut self, minutes: Option<u64>) {
        self.paused_until = Some(minutes.map(|m| now_ms() + m * 60_000).unwrap_or(u64::MAX));
        info!("paused for {minutes:?} min");
        self.apply();
        let sub = match minutes {
            Some(m) => format!("for {m} minutes"),
            None => "until resumed".to_string(),
        };
        self.show_osd(osd::Content::Message("Paused".into(), sub));
    }

    fn resume(&mut self) {
        if self.paused_until.take().is_some() {
            info!("resumed");
            self.invalidate();
            self.apply();
            self.show_osd(osd::Content::Message("Resumed".into(), String::new()));
        }
    }

    fn resume_if_paused(&mut self) {
        if self.paused_until.take().is_some() {
            self.invalidate();
        }
    }

    /// Full brightness, neutral color, then pause for an hour (PRODUCT §9).
    fn panic(&mut self) {
        info!("panic restore");
        self.paused_until = None;
        self.filter = Filter::None;
        self.movie = None;
        self.set_master(engine::MAX_BRIGHTNESS);
        self.set_kelvin(color::NEUTRAL_KELVIN);
        self.invalidate();
        self.commit();
        self.paused_until = Some(now_ms() + PANIC_PAUSE_MINUTES * 60_000);
        self.apply();
        self.show_osd(osd::Content::Message("Restored".into(), "Full brightness · paused for 1 hour".into()));
    }

    fn on_tick(&mut self) {
        if let Some(until) = self.paused_until {
            if until != u64::MAX && now_ms() >= until {
                self.resume();
            }
        }
        if self.update_schedule(false) {
            self.apply();
        }
        self.check_reminders();
    }

    fn check_reminders(&mut self) {
        let now = now_ms();
        let idle = crate::idle::idle_ms();
        if idle >= NATURAL_BREAK_MS {
            self.active_since = now;
        }
        // Being away for a whole break counts as one.
        if self.break_left.is_none() && idle >= self.settings.break_minutes as u64 * 60_000 {
            self.work_since = now;
        }
        let busy = self.paused_until.is_some()
            || self.fullscreen
            || foreground::is_fullscreen(unsafe { GetForegroundWindow() });
        if !busy {
            let running = self.break_left.map(|(k, _)| k);
            match break_due(&self.settings, now, self.active_since, self.work_since, running) {
                Due::Start(kind) => self.start_break(kind),
                Due::SkipEye => self.active_since = now,
                Due::Nothing => {}
            }
        }
        if self.settings.bedtime_reminder && now.saturating_sub(self.bedtime_shown) > 12 * 3_600_000 {
            let (_, _, t, _) = schedule::now_local();
            let mins = schedule::minutes_to_bedtime(&self.settings.schedule, t);
            if mins <= self.settings.bedtime_minutes as f64 {
                self.bedtime_shown = now;
                info!("bedtime reminder ({mins:.0} min)");
                if let Some(o) = self.osd.as_mut() {
                    o.set_palette(theme::palette(self.settings.theme));
                    o.show_for(
                        osd::Content::Message(
                            format!("Bedtime in {}", fmt_minutes(mins)),
                            "Time to start winding down".into(),
                        ),
                        8000,
                    );
                }
            }
        }
    }

    fn start_break(&mut self, kind: Break) {
        let seconds = match kind {
            Break::Eye => EYE_BREAK_SECONDS,
            Break::Computer => self.settings.break_minutes * 60,
        };
        info!("{} ({seconds} s)", kind.title().to_lowercase());
        let now = now_ms();
        self.active_since = now;
        if kind == Break::Computer {
            self.work_since = now;
        }
        self.break_left = Some((kind, seconds));
        if let Some(o) = self.osd.as_mut() {
            o.set_palette(theme::palette(self.settings.theme));
            o.show_for(break_content(kind, seconds), (seconds + 2) * 1000);
        }
        unsafe { SetTimer(Some(self.hwnd), TIMER_BREAK, 1000, None) };
    }

    fn on_break_tick(&mut self) {
        let Some((kind, left)) = self.break_left else { return };
        let left = left.saturating_sub(1);
        if left == 0 {
            self.break_left = None;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_BREAK);
            }
            let next = match kind {
                Break::Eye => "see you in 20 minutes".to_string(),
                Break::Computer => {
                    // The next stretches start now.
                    let now = now_ms();
                    self.active_since = now;
                    self.work_since = now;
                    format!("next break in {}", fmt_minutes(self.settings.break_every as f64))
                }
            };
            if let Some(o) = self.osd.as_mut() {
                o.show_for(
                    osd::Content::Message("Break done".into(), format!("Back to work — {next}")),
                    4000,
                );
            }
        } else {
            self.break_left = Some((kind, left));
            if let Some(o) = self.osd.as_mut() {
                // Another OSD (brightness, scene) may have taken over: wait for it to fade, then show again.
                if o.hidden() {
                    o.show_for(break_content(kind, left), (left + 2) * 1000);
                } else if o.title() == Some(kind.title()) {
                    o.update(break_content(kind, left));
                }
            }
        }
    }

    fn flyout_state(&self) -> flyout::State {
        let sc = &self.settings.schedule;
        let schedule_text = if let Some((exe, action)) = &self.rule {
            format!("Rule for {exe}: {}", rule_text(action))
        } else if self.fullscreen {
            "Paused while a fullscreen app is active".to_string()
        } else if self.filter != Filter::None {
            format!("{} is on · tap it again to turn it off", self.filter.name())
        } else if let Some((until, k)) = self.movie {
            let mins = until.saturating_sub(now_ms()) / 60_000;
            format!("Movie mode · {k}K for {} h {:02} min more", mins / 60, mins % 60)
        } else if !sc.enabled {
            "Manual warmth · schedule off".to_string()
        } else if let Some((_, phase)) = self.override_k {
            format!("Manual until the {} ends", format!("{phase:?}").to_lowercase())
        } else {
            // Never show the city/country here: the flyout may be visible while streaming or sharing.
            let phase = schedule::target_now(sc).phase;
            format!("Automatic · {phase:?}")
        };
        flyout::State {
            master: self.master(),
            kelvin: self.kelvin,
            monitors: self
                .screens
                .iter()
                .map(|s| flyout::MonitorRow {
                    name: s.mon.name.clone(),
                    brightness: s.brightness,
                    hardware: s.hw.is_some(),
                })
                .collect(),
            scenes: self.settings.scenes.iter().map(|s| s.name.clone()).collect(),
            active_scene: if self.filter != Filter::None {
                self.settings.scenes.iter().position(|s| scene_filter(s.effect) == self.filter)
            } else {
                self.active_scene
            },
            paused: self.paused_until.is_some(),
            schedule_text,
            overriding: self.override_k.is_some(),
            deep_dim_countdown: self.deep_dim_left,
        }
    }

    fn update_flyout(&mut self) {
        let visible = self.flyout.as_ref().is_some_and(|f| f.visible());
        if visible {
            let st = self.flyout_state();
            let pal = theme::palette(self.settings.theme);
            if let Some(f) = self.flyout.as_mut() {
                f.set_state(st, pal);
            }
        }
    }

    fn toggle_flyout(&mut self) {
        let st = self.flyout_state();
        let pal = theme::palette(self.settings.theme);
        let anchor = self.tray.as_ref().and_then(|t| t.rect());
        if let Some(f) = self.flyout.as_mut() {
            if f.visible() || f.just_hidden() {
                f.hide();
            } else {
                f.set_state(st, pal);
                f.show(anchor);
            }
        }
    }

    fn on_flyout_action(&mut self, a: flyout::Action) {
        use flyout::Action;
        match a {
            Action::Master(v) => {
                self.resume_if_paused();
                self.active_scene = None;
                self.set_master(v);
                self.commit();
            }
            Action::Kelvin(k) => {
                self.resume_if_paused();
                self.active_scene = None;
                self.set_kelvin(k);
                self.commit();
            }
            Action::Monitor(i, v) => {
                self.resume_if_paused();
                self.active_scene = None;
                self.set_brightness(Some(i), v);
                self.commit();
            }
            Action::Scene(i) => self.apply_scene(i),
            Action::TogglePause => {
                if self.paused_until.is_some() {
                    self.resume();
                } else {
                    self.pause(None);
                }
            }
            Action::ReturnToSchedule => {
                self.override_k = None;
                self.update_schedule(true);
                self.apply();
            }
            Action::KeepDeepDim => {
                for s in &self.screens {
                    if s.brightness < DEEP_DIM {
                        self.settings.monitor_mut(&s.key).deep_dim_ok = true;
                    }
                }
                self.deep_dim_left = None;
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_DEEP_DIM);
                }
                self.commit();
            }
            Action::OpenSettings => self.open_settings(),
            Action::Commit => self.check_deep_dim(),
        }
    }

    fn settings_view(&self) -> settings_ui::View {
        let range_expanded = crate::install::gamma_range_expanded();
        settings_ui::View {
            settings: {
                let mut s = self.settings.clone();
                for sc in &self.screens {
                    let m = s.monitor_mut(&sc.key);
                    m.brightness = sc.brightness;
                }
                s
            },
            monitors: self
                .screens
                .iter()
                .map(|s| settings_ui::MonitorInfo {
                    key: s.key.clone(),
                    name: s.mon.name.clone(),
                    method: match s.hw.map(|c| c.kind) {
                        Some(hardware::Kind::Ddc) => "DDC/CI backlight",
                        Some(hardware::Kind::Panel) => "Laptop panel",
                        None => "Software only",
                    }
                    .to_string(),
                    hdr: s.mon.hdr,
                    gamma_limited: s.gamma_limited && !range_expanded,
                })
                .collect(),
            settings_path: self.paths.settings.display().to_string(),
            portable: self.paths.portable,
            installed: crate::install::is_installed(),
            hotkey_conflicts: self.hotkey_conflicts.clone(),
            range_expanded,
            night_light_on: crate::install::night_light_on(),
            recent_apps: self.recent_apps.clone(),
            sensor_available: self.sensor.is_some() || crate::ambient::Sensor::open().is_some(),
            lux: self.lux,
            current_brightness: self.master(),
            current_kelvin: self.kelvin,
        }
    }

    fn update_settings_window(&mut self) {
        if self.settings_win.is_some() {
            let view = self.settings_view();
            let pal = theme::palette(self.settings.theme);
            if let Some(w) = self.settings_win.as_mut() {
                w.update(view, pal);
            }
        }
    }

    fn open_settings(&mut self) {
        if let Some(f) = self.flyout.as_mut() {
            f.hide();
        }
        if let Some(w) = self.settings_win.as_mut() {
            w.show_page(settings_ui::Page::General);
            return;
        }
        let first_page = if self.settings.schedule.enabled && !self.settings.schedule.has_location() {
            settings_ui::Page::Schedule
        } else {
            settings_ui::Page::General
        };
        self.settings_win = SettingsWindow::create(
            self.hwnd,
            self.settings_view(),
            theme::palette(self.settings.theme),
            first_page,
        );
        self.update_watcher();
        info!("settings window opened: {}", self.settings_win.is_some());
    }

    /// The foreground hook runs only when something needs it.
    fn update_watcher(&mut self) {
        let on =
            !self.settings.rules.is_empty() || self.settings.pause_fullscreen || self.settings_win.is_some();
        self.watcher.set_enabled(self.hwnd, on);
        if !on && (self.rule.is_some() || self.fullscreen) {
            self.rule = None;
            self.fullscreen = false;
            self.invalidate();
            self.apply();
        } else if on {
            let fg = unsafe { GetForegroundWindow() };
            self.on_foreground(fg);
        }
    }

    fn on_foreground(&mut self, hwnd: HWND) {
        let Some((exe, own)) = foreground::exe_of(hwnd) else { return };
        if own {
            return; // our flyout/settings: keep whatever rule applied before
        }
        if exe != "explorer.exe" && !self.recent_apps.contains(&exe) {
            self.recent_apps.insert(0, exe.clone());
            self.recent_apps.truncate(8);
            self.update_settings_window();
        }
        let rule =
            self.settings.rules.iter().find(|r| r.exe == exe).map(|r| (r.exe.clone(), r.action.clone()));
        let fullscreen = self.settings.pause_fullscreen && foreground::is_fullscreen(hwnd);
        if rule != self.rule || fullscreen != self.fullscreen {
            info!("foreground {exe}: rule={rule:?} fullscreen={fullscreen}");
            let was_off = matches!(self.rule, Some((_, RuleAction::Disable))) || self.fullscreen;
            self.rule = rule;
            self.fullscreen = fullscreen;
            let now_off = matches!(self.rule, Some((_, RuleAction::Disable))) || self.fullscreen;
            if was_off && !now_off {
                self.invalidate();
            }
            self.apply();
        }
    }

    /// Settings were edited in the settings window: apply everything that depends on them.
    fn settings_changed(
        &mut self,
        hotkeys_before: Vec<(String, String)>,
        scene_keys_before: Vec<String>,
        autostart_before: bool,
    ) {
        for s in &mut self.screens {
            if let Some(m) = self.settings.monitor(&s.key) {
                s.hw_share = m.hw_share;
                s.enabled = m.enabled;
            }
        }
        let scene_keys: Vec<String> = self.settings.scenes.iter().map(|s| s.hotkey.clone()).collect();
        if !self.capturing && (hotkeys_before != self.settings.hotkeys || scene_keys_before != scene_keys) {
            self.register_hotkeys();
        }
        theme::apply_menu_theme(self.settings.theme);
        if autostart_before != self.settings.autostart {
            crate::install::set_autostart(self.settings.autostart);
        }
        self.update_schedule(true);
        self.rule = None;
        self.fullscreen = false;
        self.update_watcher();
        self.update_idle_timer();
        self.update_ambient();
        self.commit();
    }

    fn on_settings_action(&mut self, a: settings_ui::Action) {
        use settings_ui::Action;
        match a {
            Action::Edit(f) => {
                let hk = self.settings.hotkeys.clone();
                let sk = self.settings.scenes.iter().map(|s| s.hotkey.clone()).collect();
                let auto = self.settings.autostart;
                let breaks = self.settings.computer_breaks;
                self.sync_settings();
                f(&mut self.settings);
                if self.settings.computer_breaks && !breaks {
                    // Count the first stretch from when the reminder was turned on.
                    self.work_since = now_ms();
                }
                self.settings_changed(hk, sk, auto);
            }
            Action::Preview(k) => {
                if let Some(k) = k {
                    self.preview_k = Some(k);
                    self.apply();
                }
                unsafe {
                    SetTimer(
                        Some(self.hwnd),
                        TIMER_PREVIEW,
                        if k.is_some() { PREVIEW_MS } else { 1500 },
                        None,
                    )
                };
            }
            Action::Capturing(on) => {
                self.capturing = on;
                if on {
                    hotkeys::unregister_all(self.hwnd, self.hotkey_ids());
                } else {
                    self.register_hotkeys();
                    self.update_settings_window();
                }
            }
            Action::SaveCurrentAsScene(name) => {
                let hk = self.settings.hotkeys.clone();
                let sk = self.settings.scenes.iter().map(|s| s.hotkey.clone()).collect();
                let auto = self.settings.autostart;
                self.settings.scenes.push(model::Scene {
                    name,
                    brightness: Some(self.master().round()),
                    kelvin: Some(self.kelvin),
                    effect: SceneEffect::None,
                    hotkey: String::new(),
                });
                self.settings_changed(hk, sk, auto);
            }
            Action::Identify => {
                let mons: Vec<Monitor> = self.screens.iter().map(|s| s.mon.clone()).collect();
                crate::ui::identify::show(&mons, theme::palette(self.settings.theme));
            }
            Action::CopyDiagnostics => {
                let text = self.diagnostics();
                let ok = crate::install::copy_to_clipboard(self.hwnd, &text);
                self.show_osd(osd::Content::Message(
                    if ok { "Diagnostics copied" } else { "Could not copy" }.into(),
                    String::new(),
                ));
            }
            Action::OpenSettingsFolder => {
                if let Some(dir) = self.paths.settings.parent() {
                    crate::install::open_folder(dir);
                }
            }
            Action::Install => {
                // Close first so the installed copy can take over the single-instance slot.
                self.save();
                match crate::install::install() {
                    Ok(target) => {
                        *RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()) = Some((target, Vec::new()));
                        unsafe {
                            let _ = DestroyWindow(self.hwnd);
                        }
                    }
                    Err(e) => {
                        crate::install::ask(&format!("Installation failed: {e}"), false);
                    }
                }
            }
            Action::Uninstall => {
                if crate::install::ask("Remove Screen Lighting Control from this computer?", true) {
                    *RELAUNCH.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((crate::config::exe_path(), vec!["--uninstall".into(), "--yes".into()]));
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                }
            }
            Action::ExpandRange => {
                if !crate::install::run_elevated("--expand-range") {
                    info!("expand range: elevation declined");
                }
            }
            Action::Closed => {
                info!("settings window closed");
                self.settings_win = None;
                self.update_watcher();
                unsafe { SetTimer(Some(self.hwnd), TIMER_TRIM, TRIM_DELAY_MS, None) };
            }
        }
    }

    fn hotkey_ids(&self) -> Vec<i32> {
        (1..=model::HOTKEY_ACTIONS.len() as i32)
            .chain((0..self.settings.scenes.len() as i32 + 16).map(|i| HOTKEY_SCENE_BASE + i))
            .collect()
    }

    fn diagnostics(&self) -> String {
        let mut out = format!("Screen Lighting Control {}\r\n", crate::VERSION);
        out.push_str(&format!(
            "settings: {} (portable={})\r\n",
            self.paths.settings.display(),
            self.paths.portable
        ));
        out.push_str(&format!(
            "kelvin={} master={:.0} paused={}\r\n",
            self.kelvin,
            self.master(),
            self.paused_until.is_some()
        ));
        for (i, s) in self.screens.iter().enumerate() {
            out.push_str(&format!(
                "monitor #{} '{}' {} hw={:?} brightness={:.0} share={:.0} gamma_scale={:.2} limited={} hdr={}\r\n",
                i + 1,
                s.mon.name,
                s.mon.device,
                s.hw,
                s.brightness,
                s.hw_share,
                s.gamma_scale,
                s.gamma_limited,
                s.mon.hdr
            ));
        }
        out.push_str(&format!("hotkey conflicts: {:?}\r\n--- log ---\r\n", self.hotkey_conflicts));
        out.push_str(&crate::log::snapshot());
        out
    }

    /// Starts the "keep this?" countdown the first time a monitor goes below 5% (PRODUCT §12).
    fn check_deep_dim(&mut self) {
        if self.deep_dim_left.is_some() {
            return;
        }
        let needs = self.screens.iter().any(|s| {
            s.brightness < DEEP_DIM && !self.settings.monitor(&s.key).is_some_and(|m| m.deep_dim_ok)
        });
        if needs {
            self.deep_dim_left = Some(DEEP_DIM_SECONDS);
            unsafe { SetTimer(Some(self.hwnd), TIMER_DEEP_DIM, 1000, None) };
            if !self.flyout.as_ref().is_some_and(|f| f.visible()) {
                self.show_osd(osd::Content::Message(
                    "Very dark".into(),
                    format!("Open SLC and press Keep within {DEEP_DIM_SECONDS} s"),
                ));
            }
            self.update_flyout();
        }
    }

    fn on_deep_dim_tick(&mut self) {
        let Some(left) = self.deep_dim_left else { return };
        if left > 1 {
            self.deep_dim_left = Some(left - 1);
            self.update_flyout();
            return;
        }
        self.deep_dim_left = None;
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_DEEP_DIM);
        }
        let mut reverted = false;
        for s in &mut self.screens {
            if s.brightness < DEEP_DIM && !self.settings.monitor(&s.key).is_some_and(|m| m.deep_dim_ok) {
                s.brightness = DEEP_DIM;
                reverted = true;
            }
        }
        if reverted {
            info!("deep dim not confirmed; reverted to {DEEP_DIM}%");
            self.commit();
            self.show_osd(osd::Content::Message(
                "Reverted".into(),
                format!("Brightness set back to {DEEP_DIM:.0}%"),
            ));
        }
    }

    fn on_tray_hover(&mut self) {
        let Some(rect) = self.tray.as_ref().and_then(|t| t.rect()) else { return };
        WHEEL_TARGET.with(|t| t.set((rect, self.hwnd.0 as isize)));
        if self.wheel_hook.is_none() {
            self.wheel_hook =
                unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_hook), Some(win::hinstance()), 0).ok() };
            unsafe { SetTimer(Some(self.hwnd), TIMER_WHEEL_HOOK, 250, None) };
        }
    }

    fn check_wheel_hook(&mut self) {
        let mut pt = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut pt);
        }
        let (r, _) = WHEEL_TARGET.with(|t| t.get());
        let inside = pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
        if !inside {
            if let Some(h) = self.wheel_hook.take() {
                unsafe {
                    let _ = UnhookWindowsHookEx(h);
                }
            }
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_WHEEL_HOOK);
            }
        }
    }

    fn on_tray(&mut self, event: u32, anchor: POINT) {
        match event {
            // WM_RBUTTONUP is followed by WM_CONTEXTMENU for the same click: use only the latter.
            WM_CONTEXTMENU => {
                if let Some(f) = self.flyout.as_mut() {
                    f.hide();
                }
                self.show_menu(anchor)
            }
            // One left click delivers both WM_LBUTTONUP and NIN_SELECT (NOTIFYICON_VERSION_4): react to
            // NIN_SELECT only, or the flyout would open and immediately close again.
            e if e == NIN_SELECT || e == NIN_KEYSELECT => self.toggle_flyout(),
            WM_MOUSEMOVE => self.on_tray_hover(),
            _ => {}
        }
    }

    fn show_menu(&mut self, at: POINT) {
        let warmth = color::PRESETS
            .iter()
            .enumerate()
            .rev()
            .map(|(i, (k, name))| {
                MenuItem::check(CMD_KELVIN_BASE + i as u32, &format!("{name} ({k}K)"), *k == self.kelvin)
            })
            .collect();
        let brightness = BRIGHTNESS_STEPS
            .iter()
            .enumerate()
            .map(|(i, b)| {
                MenuItem::check(
                    CMD_BRIGHTNESS_BASE + i as u32,
                    &format!("{b:.0}%"),
                    (*b - self.master()).abs() < 0.5,
                )
            })
            .collect();
        let scenes = self
            .settings
            .scenes
            .iter()
            .enumerate()
            .map(|(i, sc)| MenuItem::item(CMD_SCENE_BASE + i as u32, &sc.name))
            .collect();
        let pause = if self.paused_until.is_some() {
            MenuItem::item(CMD_RESUME, "Resume")
        } else {
            MenuItem::Sub {
                text: "Pause".into(),
                items: vec![
                    MenuItem::item(CMD_PAUSE_HOUR, "For 1 hour"),
                    MenuItem::item(CMD_PAUSE_FOREVER, "Until resumed"),
                ],
            }
        };
        let items = vec![
            MenuItem::item(CMD_OPEN_PANEL, "Open Screen Lighting Control"),
            MenuItem::item(CMD_SETTINGS, "Settings…"),
            MenuItem::Separator,
            MenuItem::Sub { text: "Brightness".into(), items: brightness },
            MenuItem::Sub { text: "Warmth".into(), items: warmth },
            MenuItem::Sub { text: "Scenes".into(), items: scenes },
            MenuItem::check(CMD_SCHEDULE_TOGGLE, "Automatic schedule", self.settings.schedule.enabled),
            MenuItem::Sub {
                text: "Color filter".into(),
                items: Filter::ALL
                    .iter()
                    .enumerate()
                    .map(|(i, f)| MenuItem::check(CMD_FILTER_BASE + i as u32, f.name(), self.filter == *f))
                    .collect(),
            },
            MenuItem::check(CMD_MOVIE, "Movie mode (2.5 h)", self.movie.is_some()),
        ];
        let mut items = items;
        if self.override_k.is_some() {
            items.push(MenuItem::item(CMD_SCHEDULE_RESUME, "Return to schedule"));
        }
        items.extend([
            pause,
            MenuItem::Separator,
            MenuItem::item(CMD_RESET, "Reset everything"),
            MenuItem::Separator,
            MenuItem::item(CMD_EXIT, "Exit"),
        ]);
        match tray::popup(self.hwnd, at, &items) {
            CMD_EXIT => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            CMD_RESET => self.reset(),
            CMD_OPEN_PANEL => {
                if !self.flyout.as_ref().is_some_and(|f| f.visible()) {
                    self.toggle_flyout();
                }
            }
            CMD_SETTINGS => self.open_settings(),
            CMD_PAUSE_HOUR => self.pause(Some(60)),
            CMD_PAUSE_FOREVER => self.pause(None),
            CMD_RESUME => self.resume(),
            CMD_SCHEDULE_TOGGLE => {
                self.settings.schedule.enabled = !self.settings.schedule.enabled;
                self.update_schedule(true);
                self.commit();
            }
            c if (CMD_FILTER_BASE..CMD_FILTER_BASE + Filter::ALL.len() as u32).contains(&c) => {
                self.resume_if_paused();
                self.filter = Filter::ALL[(c - CMD_FILTER_BASE) as usize];
                self.apply();
            }
            CMD_MOVIE => {
                if self.movie.is_some() {
                    self.stop_movie();
                } else {
                    self.start_movie(MOVIE_DEFAULT_K);
                }
                self.apply();
            }
            CMD_SCHEDULE_RESUME => {
                self.override_k = None;
                self.update_schedule(true);
                self.apply();
            }
            c if (CMD_SCENE_BASE..CMD_SCENE_BASE + self.settings.scenes.len() as u32).contains(&c) => {
                self.apply_scene((c - CMD_SCENE_BASE) as usize)
            }
            c if (CMD_KELVIN_BASE..CMD_KELVIN_BASE + color::PRESETS.len() as u32).contains(&c) => {
                self.resume_if_paused();
                self.set_kelvin(color::PRESETS[(c - CMD_KELVIN_BASE) as usize].0);
                self.commit();
            }
            c if (CMD_BRIGHTNESS_BASE..CMD_BRIGHTNESS_BASE + BRIGHTNESS_STEPS.len() as u32).contains(&c) => {
                self.resume_if_paused();
                self.set_master(BRIGHTNESS_STEPS[(c - CMD_BRIGHTNESS_BASE) as usize]);
                self.commit();
                self.check_deep_dim();
            }
            _ => {}
        }
    }

    /// Handles a command line forwarded from another `slc.exe` process.
    fn on_forward(&mut self, line: &str) -> bool {
        info!("forwarded: {line}");
        let Ok(reqs) = crate::cli::parse_forward(line) else { return false };
        for r in reqs {
            match r {
                crate::cli::Request::Brightness { value, monitor } => {
                    self.resume_if_paused();
                    self.set_brightness(monitor, value);
                    self.active_scene = None;
                }
                crate::cli::Request::Kelvin(k) => self.set_kelvin(color::clamp_kelvin(k as i64)),
                crate::cli::Request::Scene(name) => {
                    match self.settings.scenes.iter().position(|s| s.name.eq_ignore_ascii_case(&name)) {
                        Some(i) => self.apply_scene(i),
                        None => return false,
                    }
                }
                crate::cli::Request::Pause(m) => self.pause((m > 0).then_some(m as u64)),
                crate::cli::Request::Resume => self.resume(),
                crate::cli::Request::Filter(name) => {
                    match Filter::ALL.iter().find(|f| {
                        f.name().to_ascii_lowercase().starts_with(&name)
                            || format!("{f:?}").eq_ignore_ascii_case(&name)
                    }) {
                        Some(f) => {
                            self.resume_if_paused();
                            self.filter = *f;
                        }
                        None => return false,
                    }
                }
                crate::cli::Request::Settings(page) => {
                    self.open_settings();
                    if let (Some(p), Some(w)) =
                        (page.and_then(|p| settings_ui::Page::from_name(&p)), self.settings_win.as_mut())
                    {
                        w.show_page(p);
                    }
                    return true;
                }
                crate::cli::Request::Exit => {
                    unsafe {
                        let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return true;
                }
            }
        }
        self.commit();
        self.check_deep_dim();
        true
    }

    fn reset(&mut self) {
        info!("reset requested");
        self.paused_until = None;
        self.filter = Filter::None;
        self.movie = None;
        self.set_kelvin(color::NEUTRAL_KELVIN);
        self.set_master(engine::MAX_BRIGHTNESS);
        self.invalidate();
        self.commit();
    }

    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_APP_TRAY => {
                // NOTIFYICON_VERSION_4: LOWORD(lParam) = event, wParam = anchor x/y.
                let (x, y) = win::point_from(wp.0);
                self.on_tray(win::loword(lp.0 as usize), POINT { x, y });
                Some(LRESULT(0))
            }
            WM_COPYDATA => {
                let cds = unsafe { &*(lp.0 as *const windows::Win32::System::DataExchange::COPYDATASTRUCT) };
                if cds.dwData != win::COPYDATA_FORWARD || cds.lpData.is_null() {
                    return Some(LRESULT(0));
                }
                let bytes =
                    unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) };
                let line = String::from_utf8_lossy(bytes).into_owned();
                Some(LRESULT(self.on_forward(&line) as isize))
            }
            WM_APP_ACTIVATE => {
                info!("activated by another instance");
                if !self.flyout.as_ref().is_some_and(|f| f.visible()) {
                    self.toggle_flyout();
                }
                Some(LRESULT(0))
            }
            flyout::WM_APP_FLYOUT_ACTION => {
                let a = unsafe { Box::from_raw(lp.0 as *mut flyout::Action) };
                self.on_flyout_action(*a);
                Some(LRESULT(0))
            }
            foreground::WM_APP_FOREGROUND => {
                self.on_foreground(HWND(lp.0 as *mut _));
                Some(LRESULT(0))
            }
            WM_APP_TRAY_WHEEL => {
                let delta = wp.0 as u16 as i16;
                let step = if delta > 0 { 2.0 } else { -2.0 };
                self.resume_if_paused();
                self.set_master(self.master() + step);
                self.active_scene = None;
                self.commit();
                self.show_osd(osd::Content::Brightness(self.master()));
                Some(LRESULT(0))
            }
            settings_ui::WM_APP_SETTINGS_ACTION => {
                let a = unsafe { Box::from_raw(lp.0 as *mut settings_ui::Action) };
                self.on_settings_action(*a);
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_RANGE_OFFER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_RANGE_OFFER);
                }
                crate::install::offer_expand_range_once(&self.paths.state);
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_AMBIENT => {
                self.on_ambient_tick();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_BREAK => {
                self.on_break_tick();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_IDLE => {
                self.on_idle_timer();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_TRIM => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_TRIM);
                    // Idle tray app: give memory pages back to the system (they return on demand).
                    let _ = windows::Win32::System::Threading::SetProcessWorkingSetSize(
                        windows::Win32::System::Threading::GetCurrentProcess(),
                        usize::MAX,
                        usize::MAX,
                    );
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_PREVIEW => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_PREVIEW);
                }
                if self.preview_k.take().is_some() {
                    self.apply();
                }
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_DEEP_DIM => {
                self.on_deep_dim_tick();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_WHEEL_HOOK => {
                self.check_wheel_hook();
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE | WM_DPICHANGED => {
                // Several of these arrive per change; re-enumerate once things settle.
                unsafe { SetTimer(Some(self.hwnd), TIMER_DISPLAY_CHANGE, DISPLAY_CHANGE_DEBOUNCE_MS, None) };
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_DISPLAY_CHANGE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_DISPLAY_CHANGE);
                }
                info!("display configuration changed");
                self.refresh_monitors();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_SAVE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_SAVE);
                }
                self.save();
                Some(LRESULT(0))
            }
            WM_HOTKEY => {
                self.on_hotkey(wp.0 as i32);
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_TICK => {
                self.on_tick();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_GAMMA_CHECK => {
                self.check_gamma();
                // Videos and games can go fullscreen without a foreground change.
                if self.settings.pause_fullscreen {
                    let fg = unsafe { GetForegroundWindow() };
                    self.on_foreground(fg);
                }
                Some(LRESULT(0))
            }
            WM_SETTINGCHANGE => {
                // "ImmersiveColorSet": the Windows light/dark theme changed.
                let name = if lp.0 != 0 {
                    unsafe { windows::core::PCWSTR(lp.0 as *const u16).to_string().unwrap_or_default() }
                } else {
                    String::new()
                };
                if name == "ImmersiveColorSet" {
                    theme::apply_menu_theme(self.settings.theme);
                    let pal = theme::palette(self.settings.theme);
                    if let Some(o) = self.osd.as_mut() {
                        o.set_palette(pal);
                    }
                    self.update_flyout();
                    self.update_settings_window();
                }
                None
            }
            WM_TIMECHANGE => {
                self.update_schedule(true);
                self.apply();
                Some(LRESULT(0))
            }
            WM_TIMER if wp.0 == TIMER_RESUME => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_RESUME);
                }
                info!("re-applying after resume");
                self.refresh_monitors();
                Some(LRESULT(0))
            }
            WM_POWERBROADCAST => {
                if wp.0 as u32 == PBT_APMRESUMEAUTOMATIC {
                    unsafe { SetTimer(Some(self.hwnd), TIMER_RESUME, RESUME_DELAY_MS, None) };
                }
                Some(LRESULT(1))
            }
            WM_WTSSESSION_CHANGE => {
                if wp.0 as u32 == WTS_SESSION_UNLOCK {
                    info!("session unlocked; re-applying");
                    self.invalidate();
                    self.apply();
                }
                Some(LRESULT(0))
            }
            WM_QUERYENDSESSION => Some(LRESULT(1)),
            WM_ENDSESSION => {
                if wp.0 != 0 {
                    info!("session ending");
                    self.shutdown();
                }
                Some(LRESULT(0))
            }
            WM_APP_ENGINE => {
                self.on_engine_events();
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                unsafe {
                    let _ = WTSUnRegisterSessionNotification(self.hwnd);
                }
                self.shutdown();
                self.tray = None;
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            m if m == self.msg_taskbar_created && m != 0 => {
                info!("taskbar recreated; re-adding tray icon");
                if let Some(t) = self.tray.as_mut() {
                    t.add();
                }
                self.overlays.raise_all();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let app = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !app.is_null() {
        if let Some(r) = (*app).handle(msg, wp, lp) {
            return r;
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

/// Runs the message loop until WM_QUIT.
pub fn run_loop() -> i32 {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam.0 as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

    fn settings(eye: bool, computer: bool) -> Settings {
        Settings { eye_breaks: eye, computer_breaks: computer, break_every: 60, ..Settings::default() }
    }

    #[test]
    fn computer_break_after_the_interval() {
        let s = settings(false, true);
        assert_eq!(break_due(&s, 59 * MIN, 0, 0, None), Due::Nothing);
        assert_eq!(break_due(&s, 60 * MIN, 0, 0, None), Due::Start(Break::Computer));
        assert_eq!(break_due(&s, 70 * MIN, 0, 0, Some(Break::Computer)), Due::Nothing);
        assert_eq!(break_due(&settings(false, false), 600 * MIN, 0, 0, None), Due::Nothing);
    }

    #[test]
    fn computer_break_wins_over_eye_break() {
        let s = settings(true, true);
        // Both due: the computer break starts, even during an eye break.
        assert_eq!(break_due(&s, 60 * MIN, 40 * MIN, 0, None), Due::Start(Break::Computer));
        assert_eq!(break_due(&s, 60 * MIN, 40 * MIN, 0, Some(Break::Eye)), Due::Start(Break::Computer));
        // An eye break right before a computer break is skipped; earlier ones run.
        assert_eq!(break_due(&s, 59 * MIN, 39 * MIN, 0, None), Due::SkipEye);
        assert_eq!(break_due(&s, 40 * MIN, 20 * MIN, 0, None), Due::Start(Break::Eye));
        // Eye breaks alone don't care about the computer clock.
        assert_eq!(break_due(&settings(true, false), 59 * MIN, 39 * MIN, 0, None), Due::Start(Break::Eye));
    }
}
