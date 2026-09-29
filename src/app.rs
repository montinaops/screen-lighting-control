//! The application: owns all state and the hidden controller window, and reacts to events.

use crate::color;
use crate::engine::{self, gamma};
use crate::info;
use crate::monitors::{self, Monitor};
use crate::tray::{self, MenuItem, Tray};
use crate::win::{self, CONTROLLER_CLASS, WM_APP_ACTIVATE, WM_APP_TRAY};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::NIN_SELECT;
use windows::Win32::UI::WindowsAndMessaging::*;

// Context-menu command ids.
const CMD_EXIT: u32 = 1;
const CMD_RESET: u32 = 2;
/// Warmth presets: CMD_KELVIN_BASE + index into `color::PRESETS`.
const CMD_KELVIN_BASE: u32 = 100;

// Timer ids.
const TIMER_DISPLAY_CHANGE: usize = 1;
const DISPLAY_CHANGE_DEBOUNCE_MS: u32 = 500;

pub struct App {
    hwnd: HWND,
    tray: Option<Tray>,
    msg_taskbar_created: u32,
    monitors: Vec<Monitor>,
    gamma: Vec<gamma::State>,
    kelvin: u32,
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
                monitors: Vec::new(),
                gamma: Vec::new(),
                kelvin: color::NEUTRAL_KELVIN,
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
            app.tray = Some(Tray::new(hwnd));
            app.refresh_monitors();
            info!("controller window created");
            Ok(app)
        }
    }

    fn refresh_monitors(&mut self) {
        self.monitors = monitors::enumerate();
        self.gamma = vec![gamma::State::default(); self.monitors.len()];
        for m in &self.monitors {
            info!("monitor {} '{}' internal={} hdr={}", m.device, m.name, m.internal, m.hdr);
        }
        self.apply();
    }

    /// Pushes the current state to every monitor.
    fn apply(&mut self) {
        for (m, st) in self.monitors.iter().zip(self.gamma.iter_mut()) {
            let a = gamma::apply(&m.device, self.kelvin, 1.0, st);
            if a.limited || a.failed {
                info!(
                    "gamma on {}: warmth {:.2} scale {:.2} failed={}",
                    m.device, a.warmth, a.scale, a.failed
                );
            }
        }
        let tip = format!("SLC — {}K {}", self.kelvin, color::preset_name(self.kelvin));
        if let Some(t) = self.tray.as_mut() {
            t.set_tip(&tip);
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
        let items = vec![
            MenuItem::disabled(0, "Screen Lighting Control"),
            MenuItem::Separator,
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
            _ => {}
        }
    }

    fn reset(&mut self) {
        info!("reset requested");
        self.kelvin = color::NEUTRAL_KELVIN;
        engine::reset_all(&self.monitors);
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
            WM_DESTROY => {
                engine::reset_all(&self.monitors);
                self.tray = None;
                unsafe { PostQuitMessage(0) };
                Some(LRESULT(0))
            }
            m if m == self.msg_taskbar_created && m != 0 => {
                info!("taskbar recreated; re-adding tray icon");
                if let Some(t) = self.tray.as_mut() {
                    t.add();
                }
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
