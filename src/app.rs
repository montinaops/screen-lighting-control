//! The application: owns all state and the hidden controller window, and reacts to events.

use crate::color;
use crate::engine::{self, gamma, hardware, overlay::Overlays, Event, Events};
use crate::info;
use crate::monitors::{self, Monitor};
use crate::tray::{self, MenuItem, Tray};
use crate::win::{self, CONTROLLER_CLASS, WM_APP_ACTIVATE, WM_APP_ENGINE, WM_APP_TRAY};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::*;

// Context-menu command ids.
const CMD_EXIT: u32 = 1;
const CMD_RESET: u32 = 2;
/// Warmth presets: CMD_KELVIN_BASE + index into `color::PRESETS`.
const CMD_KELVIN_BASE: u32 = 100;
/// Brightness presets: CMD_BRIGHTNESS_BASE + index into `BRIGHTNESS_STEPS`.
const CMD_BRIGHTNESS_BASE: u32 = 200;
const BRIGHTNESS_STEPS: &[f32] = &[100.0, 80.0, 60.0, 40.0, 25.0, 15.0, 10.0, 5.0, 2.0];

// Timer ids.
const TIMER_DISPLAY_CHANGE: usize = 1;
const DISPLAY_CHANGE_DEBOUNCE_MS: u32 = 500;

/// Runtime state of one monitor.
struct Screen {
    mon: Monitor,
    /// User brightness 1–100.
    brightness: f32,
    /// Percent of the slider driven by hardware (PRODUCT §4.1).
    hw_share: f32,
    /// Hardware capabilities once probed (`None` = software only).
    hw: Option<hardware::Caps>,
    /// Last hardware level sent to the worker.
    hw_sent: Option<f32>,
    /// Last gamma job sent (kelvin, requested scale).
    gamma_sent: Option<(u32, f32)>,
    /// Scale the ramp achieves (predicted until the worker confirms).
    gamma_scale: f32,
    /// Learned deviation bound for predictions.
    gamma_bound: f32,
}

impl Screen {
    fn new(mon: Monitor, brightness: f32) -> Self {
        Screen {
            mon,
            brightness,
            hw_share: DEFAULT_HW_SHARE,
            hw: None,
            hw_sent: None,
            gamma_sent: None,
            gamma_scale: 1.0,
            gamma_bound: gamma::DEFAULT_BOUND,
        }
    }
}

const DEFAULT_HW_SHARE: f32 = 50.0;

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
    kelvin: u32,
    /// Brightness from hardware has been adopted for these monitor ids (don't jump on re-probe).
    adopted: std::collections::HashSet<String>,
}

impl App {
    /// Creates the controller window and tray icon. The returned box must stay alive for
    /// the duration of the message loop (its address is stored in the window).
    pub fn create() -> windows::core::Result<Box<App>> {
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
                kelvin: color::NEUTRAL_KELVIN,
                adopted: Default::default(),
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
            info!("controller window created");
            Ok(app)
        }
    }

    fn refresh_monitors(&mut self) {
        let old: Vec<(String, f32, f32)> =
            self.screens.iter().map(|s| (s.mon.id.clone(), s.brightness, s.hw_share)).collect();
        self.gen += 1;
        self.screens = monitors::enumerate()
            .into_iter()
            .map(|m| {
                let prev = old.iter().find(|o| o.0 == m.id);
                let mut s = Screen::new(m, prev.map(|p| p.1).unwrap_or(engine::MAX_BRIGHTNESS));
                if let Some(p) = prev {
                    s.hw_share = p.2;
                }
                s
            })
            .collect();
        self.overlays.resize(self.screens.len());
        for s in &self.screens {
            info!("monitor {} '{}' internal={} hdr={}", s.mon.device, s.mon.name, s.mon.internal, s.mon.hdr);
        }
        if let Some(w) = &self.hw_worker {
            w.probe(self.gen, self.screens.iter().map(|s| (s.mon.hmon.0 as isize, s.mon.internal)).collect());
        }
        self.apply();
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
        let white = color::white_point(self.kelvin);
        for i in 0..self.screens.len() {
            let s = &mut self.screens[i];
            let split = engine::split(s.brightness, s.hw_share, s.hw.is_some());
            if let (Some(level), Some(w)) = (split.hardware, &self.hw_worker) {
                if s.hw_sent.is_none_or(|l| (l - level).abs() > 0.01) {
                    w.set(self.gen, i, level);
                    s.hw_sent = Some(level);
                }
            }
            let want = (self.kelvin, split.software);
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
                        s.hw_sent = c.map(|c| c.current);
                        if let Some(c) = c {
                            // First sight of this monitor: adopt its current backlight as our brightness,
                            // so starting SLC never changes the screen.
                            if self.adopted.insert(s.mon.id.clone()) && s.brightness >= engine::MAX_BRIGHTNESS
                            {
                                let knee = engine::MAX_BRIGHTNESS - s.hw_share;
                                s.brightness = knee + c.current / 100.0 * s.hw_share;
                            }
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
        self.gamma_worker = None;
        self.hw_worker = None;
        self.overlays.clear();
        let mons: Vec<Monitor> = self.screens.iter().map(|s| s.mon.clone()).collect();
        engine::reset_all(&mons);
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
        let items = vec![
            MenuItem::disabled(0, "Screen Lighting Control"),
            MenuItem::Separator,
            MenuItem::Sub { text: "Brightness".into(), items: brightness },
            MenuItem::Sub { text: "Warmth".into(), items: warmth },
            MenuItem::item(CMD_RESET, "Reset everything"),
            MenuItem::Separator,
            MenuItem::item(CMD_EXIT, "Exit"),
        ];
        match tray::popup(self.hwnd, at, &items) {
            CMD_EXIT => unsafe {
                let _ = DestroyWindow(self.hwnd);
            },
            CMD_RESET => self.reset(),
            c if (CMD_KELVIN_BASE..CMD_KELVIN_BASE + color::PRESETS.len() as u32).contains(&c) => {
                self.kelvin = color::PRESETS[(c - CMD_KELVIN_BASE) as usize].0;
                self.apply();
            }
            c if (CMD_BRIGHTNESS_BASE..CMD_BRIGHTNESS_BASE + BRIGHTNESS_STEPS.len() as u32).contains(&c) => {
                self.set_master(BRIGHTNESS_STEPS[(c - CMD_BRIGHTNESS_BASE) as usize]);
                self.apply();
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
                crate::cli::Request::Kelvin(k) => self.kelvin = color::clamp_kelvin(k as i64),
                other => info!("not supported yet: {other:?}"),
            }
        }
        self.apply();
        true
    }

    fn reset(&mut self) {
        info!("reset requested");
        self.kelvin = color::NEUTRAL_KELVIN;
        self.set_master(engine::MAX_BRIGHTNESS);
        self.screens.iter_mut().for_each(|s| s.gamma_sent = None);
        self.apply();
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
            WM_APP_ENGINE => {
                self.on_engine_events();
                Some(LRESULT(0))
            }
            WM_DESTROY => {
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
