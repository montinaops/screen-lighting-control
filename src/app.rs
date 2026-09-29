//! The application: owns all state and the hidden controller window, and reacts to events.

use crate::color;
use crate::config::{Ini, Paths};
use crate::engine::{self, gamma, hardware, overlay::Overlays, Event, Events};
use crate::hotkeys;
use crate::info;
use crate::model::{self, SceneEffect, Settings};
use crate::monitors::{self, Monitor};
use crate::tray::{self, MenuItem, Tray};
use crate::ui::{osd, osd::Osd, theme};
use crate::win::{self, CONTROLLER_CLASS, WM_APP_ACTIVATE, WM_APP_ENGINE, WM_APP_TRAY};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::*;

// Context-menu command ids.
const CMD_EXIT: u32 = 1;
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
            app.tray = Some(Tray::new(hwnd));
            app.refresh_monitors();
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            SetTimer(Some(hwnd), TIMER_GAMMA_CHECK, GAMMA_CHECK_MS, None);
            SetTimer(Some(hwnd), TIMER_TICK, TICK_MS, None);
            app.register_hotkeys();
            if recovered {
                if let Some(t) = &app.tray {
                    t.notify(
                        "Screen Lighting Control",
                        "SLC did not close properly last time, so your screens were reset to neutral colors.",
                    );
                }
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
    fn set_master(&mut self, value: f32) {
        let delta = value.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS) - self.master();
        for s in &mut self.screens {
            s.brightness = (s.brightness + delta).clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS);
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
    fn apply(&mut self) {
        let paused = self.paused_until.is_some();
        for i in 0..self.screens.len() {
            let s = &mut self.screens[i];
            // A disabled monitor gets neutral software effects (hardware is left alone).
            let active = s.enabled && !paused;
            let (brightness, kelvin) = if active {
                (s.brightness, self.kelvin)
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
            let want = (kelvin, split.software);
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
            let alpha = engine::overlay_alpha(split.software, s.gamma_scale);
            self.overlays.set(i, s.mon.rect, alpha);
        }
        let tip =
            format!("SLC — {:.0}% · {}K {}", self.master(), self.kelvin, color::preset_name(self.kelvin));
        if let Some(t) = self.tray.as_mut() {
            t.set_tip(&tip);
        }
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
        let ids: Vec<i32> = (1..=model::HOTKEY_ACTIONS.len() as i32)
            .chain((0..self.settings.scenes.len() as i32).map(|i| HOTKEY_SCENE_BASE + i))
            .collect();
        hotkeys::unregister_all(self.hwnd, ids);
        self.osd = None;
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
                self.commit();
                self.show_osd(osd::Content::Brightness(self.master()));
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

    fn set_kelvin(&mut self, k: u32) {
        self.kelvin = k;
        self.settings.kelvin = k;
    }

    fn apply_scene(&mut self, index: usize) {
        let Some(scene) = self.settings.scenes.get(index).cloned() else { return };
        self.scene_index = index;
        self.resume_if_paused();
        if let Some(b) = scene.brightness {
            self.set_master(b);
        }
        if let Some(k) = scene.kelvin {
            self.set_kelvin(k);
        }
        if scene.effect != SceneEffect::None {
            info!("scene effect {:?} not available yet", scene.effect);
        }
        self.commit();
        let sub = match (scene.brightness, scene.kelvin) {
            (Some(b), Some(k)) => format!("{b:.0}% · {k}K"),
            (None, Some(k)) => format!("{k}K"),
            (Some(b), None) => format!("{b:.0}%"),
            (None, None) => String::new(),
        };
        self.show_osd(osd::Content::Message(scene.name, sub));
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
    }

    fn on_tray(&mut self, event: u32, anchor: POINT) {
        match event {
            WM_CONTEXTMENU | WM_RBUTTONUP => self.show_menu(anchor),
            e if e == NIN_SELECT || e == WM_LBUTTONUP => {
                // Flyout arrives in a later milestone; show the menu for now.
                self.show_menu(anchor);
            }
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
            MenuItem::disabled(0, "Screen Lighting Control"),
            MenuItem::Separator,
            MenuItem::Sub { text: "Brightness".into(), items: brightness },
            MenuItem::Sub { text: "Warmth".into(), items: warmth },
            MenuItem::Sub { text: "Scenes".into(), items: scenes },
            pause,
            MenuItem::Separator,
            MenuItem::item(CMD_RESET, "Reset everything"),
            MenuItem::Separator,
            MenuItem::item(CMD_EXIT, "Exit"),
        ];
        match tray::popup(self.hwnd, at, &items) {
            CMD_EXIT => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            CMD_RESET => self.reset(),
            CMD_PAUSE_HOUR => self.pause(Some(60)),
            CMD_PAUSE_FOREVER => self.pause(None),
            CMD_RESUME => self.resume(),
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
                crate::cli::Request::Brightness { value, monitor } => self.set_brightness(monitor, value),
                crate::cli::Request::Kelvin(k) => self.set_kelvin(color::clamp_kelvin(k as i64)),
                crate::cli::Request::Scene(name) => {
                    match self.settings.scenes.iter().position(|s| s.name.eq_ignore_ascii_case(&name)) {
                        Some(i) => self.apply_scene(i),
                        None => return false,
                    }
                }
                crate::cli::Request::Pause(m) => self.pause((m > 0).then_some(m as u64)),
                crate::cli::Request::Resume => self.resume(),
                crate::cli::Request::Exit => {
                    unsafe {
                        let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    return true;
                }
            }
        }
        self.commit();
        true
    }

    fn reset(&mut self) {
        info!("reset requested");
        self.paused_until = None;
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
