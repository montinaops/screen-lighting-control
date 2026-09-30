//! The tray flyout: quick controls anchored above the notification area.
//!
//! The flyout never changes state itself: it reports [`Action`]s to the controller window, which
//! applies them and sends back a fresh [`State`] to draw.

use super::d2d::{Align, Rect, Surface, Weight};
use super::theme::Palette;
use super::widgets::{self, glyph};
use crate::{color, engine, win};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, InvalidateRect, MonitorFromPoint, MONITORINFO,
    MONITOR_DEFAULTTOPRIMARY, PAINTSTRUCT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: windows::core::PCWSTR = w!("MONTINA.SLC.Flyout");
const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WIDTH: f32 = 340.0;
const PAD: f32 = 16.0;

/// Posted to the controller with a boxed `Action` in lParam.
pub const WM_APP_FLYOUT_ACTION: u32 = 0x8000 + 10;

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Master(f32),
    Kelvin(u32),
    Monitor(usize, f32),
    Scene(usize),
    TogglePause,
    ReturnToSchedule,
    KeepDeepDim,
    OpenSettings,
    /// A slider drag ended (the app may save now).
    Commit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorRow {
    pub name: String,
    pub brightness: f32,
    pub hardware: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub master: f32,
    pub kelvin: u32,
    pub monitors: Vec<MonitorRow>,
    pub scenes: Vec<String>,
    pub active_scene: Option<usize>,
    pub paused: bool,
    /// e.g. "Automatic · Night" or "Manual".
    pub schedule_text: String,
    /// Manual override of the schedule is active ("Return to schedule" link).
    pub overriding: bool,
    /// Seconds left to confirm a < 5% brightness.
    pub deep_dim_countdown: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Hit {
    Master,
    Kelvin,
    Monitor(usize),
    Scene(usize),
    Pause,
    Settings,
    ReturnSchedule,
    Keep,
}

#[derive(Default)]
struct Layout {
    height: f32,
    master: Rect,
    kelvin: Rect,
    schedule: Rect,
    schedule_link: Option<Rect>,
    monitors: Vec<(Rect, Rect)>,
    scenes: Vec<Rect>,
    banner: Option<(Rect, Rect)>,
    pause: Rect,
    settings: Rect,
}

fn layout(s: &State) -> Layout {
    let mut l = Layout::default();
    let inner = WIDTH - 2.0 * PAD;
    let mut y = 14.0 + 28.0 + 10.0; // header
    if s.deep_dim_countdown.is_some() {
        let banner = Rect::new(PAD, y, inner, 44.0);
        let keep = Rect::new(banner.right() - 72.0, banner.y + 8.0, 64.0, 28.0);
        l.banner = Some((banner, keep));
        y += 44.0 + 12.0;
    }
    l.master = Rect::new(PAD, y + 22.0, inner, widgets::SLIDER_H);
    y += 22.0 + widgets::SLIDER_H + 12.0;
    l.kelvin = Rect::new(PAD, y + 22.0, inner, widgets::SLIDER_H);
    y += 22.0 + widgets::SLIDER_H + 2.0;
    l.schedule = Rect::new(PAD, y, inner, 20.0);
    if s.overriding {
        let w = super::d2d::measure("Return to schedule", 12.0, Weight::Regular) + 4.0;
        l.schedule_link = Some(Rect::new(PAD + inner - w, y, w, 20.0));
    }
    y += 20.0 + 10.0;
    if s.monitors.len() > 1 {
        y += 24.0; // "Displays" caption
        for _ in &s.monitors {
            let label = Rect::new(PAD, y, inner, 18.0);
            let slider = Rect::new(PAD, y + 18.0, inner, widgets::SLIDER_H - 4.0);
            l.monitors.push((label, slider));
            y += 18.0 + widgets::SLIDER_H - 4.0 + 8.0;
        }
        y += 2.0;
    }
    if !s.scenes.is_empty() {
        y += 24.0; // "Scenes" caption
        let (mut x, h) = (PAD, 28.0);
        for name in &s.scenes {
            let w = widgets::chip_width(name);
            if x + w > PAD + inner {
                x = PAD;
                y += h + 8.0;
            }
            l.scenes.push(Rect::new(x, y, w, h));
            x += w + 8.0;
        }
        y += h + 14.0;
    }
    y += 1.0; // separator
    let bw = (inner - 8.0) / 2.0;
    l.pause = Rect::new(PAD, y + 12.0, bw, 32.0);
    l.settings = Rect::new(PAD + bw + 8.0, y + 12.0, bw, 32.0);
    l.height = y + 12.0 + 32.0 + 14.0;
    l
}

fn hit_test(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    let grow = |r: Rect| Rect::new(r.x - 10.0, r.y - 4.0, r.w + 20.0, r.h + 8.0);
    if let Some((_, keep)) = l.banner {
        if keep.contains(x, y) {
            return Some(Hit::Keep);
        }
    }
    if grow(l.master).contains(x, y) {
        return Some(Hit::Master);
    }
    if grow(l.kelvin).contains(x, y) {
        return Some(Hit::Kelvin);
    }
    if let Some(r) = l.schedule_link {
        if r.contains(x, y) {
            return Some(Hit::ReturnSchedule);
        }
    }
    for (i, (_, s)) in l.monitors.iter().enumerate() {
        if grow(*s).contains(x, y) {
            return Some(Hit::Monitor(i));
        }
    }
    for (i, r) in l.scenes.iter().enumerate() {
        if r.contains(x, y) {
            return Some(Hit::Scene(i));
        }
    }
    if l.pause.contains(x, y) {
        return Some(Hit::Pause);
    }
    if l.settings.contains(x, y) {
        return Some(Hit::Settings);
    }
    None
}

pub fn kelvin_from_frac(f: f32) -> u32 {
    let k = color::MIN_KELVIN as f32 + f * (color::MAX_KELVIN - color::MIN_KELVIN) as f32;
    ((k / 50.0).round() * 50.0) as u32
}

fn kelvin_frac(k: u32) -> f32 {
    (k.saturating_sub(color::MIN_KELVIN)) as f32 / (color::MAX_KELVIN - color::MIN_KELVIN) as f32
}

fn brightness_from_frac(f: f32) -> f32 {
    (engine::MIN_BRIGHTNESS + f * (engine::MAX_BRIGHTNESS - engine::MIN_BRIGHTNESS)).round()
}

pub struct Flyout {
    hwnd: HWND,
    controller: HWND,
    surface: Surface,
    state: State,
    palette: Palette,
    layout: Layout,
    hot: Option<Hit>,
    drag: Option<Hit>,
    /// Tick when the flyout was hidden (a click on the tray icon right after should not reopen it).
    hidden_at: u64,
}

impl Flyout {
    pub fn create(controller: HWND, palette: Palette) -> Option<Box<Flyout>> {
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_DROPSHADOW,
                lpfnWndProc: Some(proc),
                hInstance: win::hinstance(),
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut f = Box::new(Flyout {
                hwnd: HWND::default(),
                controller,
                surface: Surface::new(HWND::default()),
                state: State::default(),
                palette,
                layout: Layout::default(),
                hot: None,
                drag: None,
                hidden_at: 0,
            });
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                CLASS,
                w!("Screen Lighting Control"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(win::hinstance()),
                Some(&mut *f as *mut Flyout as *const core::ffi::c_void),
            )
            .ok()?;
            f.hwnd = hwnd;
            f.surface = Surface::new(hwnd);
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of_val(&corner) as u32,
            );
            Some(f)
        }
    }

    pub fn visible(&self) -> bool {
        unsafe { IsWindowVisible(self.hwnd).as_bool() }
    }

    /// True right after the flyout closed because the user clicked the tray icon.
    pub fn just_hidden(&self) -> bool {
        now_ms().saturating_sub(self.hidden_at) < 300
    }

    pub fn set_state(&mut self, state: State, palette: Palette) {
        let resize = state.monitors.len() != self.state.monitors.len()
            || state.scenes != self.state.scenes
            || state.deep_dim_countdown.is_some() != self.state.deep_dim_countdown.is_some()
            || state.overriding != self.state.overriding;
        self.state = state;
        self.palette = palette;
        self.layout = layout(&self.state);
        if resize && self.visible() {
            self.position(None);
        }
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Shows the flyout above `anchor` (the tray icon rectangle), or near the cursor.
    pub fn show(&mut self, anchor: Option<RECT>) {
        self.layout = layout(&self.state);
        self.position(anchor);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
        }
    }

    fn position(&mut self, anchor: Option<RECT>) {
        unsafe {
            let pt = match anchor {
                Some(r) => POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 },
                None => {
                    let mut wr = RECT::default();
                    if self.visible() && GetWindowRect(self.hwnd, &mut wr).is_ok() {
                        POINT { x: (wr.left + wr.right) / 2, y: wr.bottom - 1 }
                    } else {
                        let mut p = POINT::default();
                        let _ = GetCursorPos(&mut p);
                        p
                    }
                }
            };
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
            let mut mi =
                MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(hmon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = windows::Win32::UI::HiDpi::GetDpiForMonitor(
                hmon,
                windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
                &mut dx,
                &mut dy,
            );
            let s = dx as f32 / 96.0;
            let (w, h) = ((WIDTH * s).round() as i32, (self.layout.height * s).round() as i32);
            let wa = mi.rcWork;
            let margin = (12.0 * s) as i32;
            let x = (pt.x - w / 2).clamp(wa.left + margin, wa.right - w - margin);
            // Above the taskbar when it is at the bottom; otherwise hug the work area edge nearest the anchor.
            let y = if pt.y >= wa.bottom {
                wa.bottom - h - margin
            } else if pt.y <= wa.top {
                wa.top + margin
            } else {
                (pt.y - h - margin).clamp(wa.top + margin, wa.bottom - h - margin)
            };
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        }
    }

    pub fn hide(&mut self) {
        if self.visible() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.hidden_at = now_ms();
        }
        self.drag = None;
    }

    fn send(&self, a: Action) {
        let ptr = Box::into_raw(Box::new(a));
        unsafe {
            if PostMessageW(Some(self.controller), WM_APP_FLYOUT_ACTION, WPARAM(0), LPARAM(ptr as isize))
                .is_err()
            {
                drop(Box::from_raw(ptr));
            }
        }
    }

    fn dip(&self, lp: LPARAM) -> (f32, f32) {
        let (x, y) = win::point_from(lp.0 as usize);
        let s = self.surface.scale();
        (x as f32 / s, y as f32 / s)
    }

    fn drag_to(&mut self, hit: Hit, x: f32) {
        let l = &self.layout;
        match hit {
            Hit::Master => self.send(Action::Master(brightness_from_frac(widgets::slider_frac(l.master, x)))),
            Hit::Kelvin => self.send(Action::Kelvin(kelvin_from_frac(widgets::slider_frac(l.kelvin, x)))),
            Hit::Monitor(i) => {
                if let Some((_, r)) = l.monitors.get(i) {
                    self.send(Action::Monitor(i, brightness_from_frac(widgets::slider_frac(*r, x))));
                }
            }
            _ => {}
        }
    }

    fn on_down(&mut self, x: f32, y: f32) {
        let Some(hit) = hit_test(&self.layout, x, y) else { return };
        match hit {
            Hit::Master | Hit::Kelvin | Hit::Monitor(_) => {
                self.drag = Some(hit);
                unsafe { SetCapture(self.hwnd) };
                self.drag_to(hit, x);
            }
            Hit::Scene(i) => self.send(Action::Scene(i)),
            Hit::Pause => self.send(Action::TogglePause),
            Hit::Settings => {
                self.hide();
                self.send(Action::OpenSettings);
            }
            Hit::ReturnSchedule => self.send(Action::ReturnToSchedule),
            Hit::Keep => self.send(Action::KeepDeepDim),
        }
    }

    fn on_wheel(&mut self, x: f32, y: f32, delta: i16) {
        let step = if delta > 0 { 1.0 } else { -1.0 };
        match hit_test(&self.layout, x, y) {
            Some(Hit::Kelvin) => {
                let k = color::clamp_kelvin(self.state.kelvin as i64 + (step * 100.0) as i64);
                self.send(Action::Kelvin(k));
            }
            Some(Hit::Monitor(i)) => {
                if let Some(m) = self.state.monitors.get(i) {
                    self.send(Action::Monitor(i, (m.brightness + step * 2.0).clamp(1.0, 100.0)));
                }
            }
            _ => self.send(Action::Master((self.state.master + step * 2.0).clamp(1.0, 100.0))),
        }
        self.send(Action::Commit);
    }

    fn paint(&mut self) {
        let pal = self.palette;
        let st = self.state.clone();
        let l = std::mem::take(&mut self.layout);
        let hot = self.drag.or(self.hot);
        self.surface.paint(|p| {
            p.clear(pal.bg);
            // Header.
            p.logo(PAD + 9.0, 28.0, 8.5, pal.accent, pal.bg, false);
            p.text(
                "Screen Lighting",
                Rect::new(PAD + 26.0, 14.0, 180.0, 28.0),
                15.0,
                Weight::Semibold,
                Align::Left,
                pal.text,
            );
            let status =
                if st.paused { "Paused".to_string() } else { format!("{:.0}% · {}K", st.master, st.kelvin) };
            p.text(
                &status,
                Rect::new(PAD, 14.0, WIDTH - 2.0 * PAD, 28.0),
                12.5,
                Weight::Regular,
                Align::Right,
                pal.subtext,
            );

            if let (Some((banner, keep)), Some(secs)) = (l.banner, st.deep_dim_countdown) {
                p.fill_round(banner, 8.0, pal.surface_hover);
                p.stroke_round(banner, 8.0, pal.border, 1.0);
                p.text(
                    glyph::WARNING,
                    Rect::new(banner.x + 12.0, banner.y, 18.0, banner.h),
                    14.0,
                    Weight::Icon,
                    Align::Left,
                    pal.text,
                );
                p.text(
                    &format!("Very dark. Keep it? Reverting in {secs} s"),
                    Rect::new(banner.x + 38.0, banner.y, banner.w - 38.0 - 80.0, banner.h),
                    12.5,
                    Weight::Regular,
                    Align::Left,
                    pal.text,
                );
                widgets::button(p, keep, None, "Keep", &pal, hot == Some(Hit::Keep), true);
            }

            let dimmed = |c| if st.paused { super::d2d::mix(c, pal.bg, 0.5) } else { c };
            let lv = |r: Rect| Rect::new(r.x, r.y - 22.0, r.w, 20.0);
            widgets::label_value(p, lv(l.master), "Brightness", &format!("{:.0}%", st.master), &pal);
            widgets::slider(p, l.master, (st.master - 1.0) / 99.0, &pal, hot == Some(Hit::Master), false);
            widgets::label_value(
                p,
                lv(l.kelvin),
                &format!("Warmth · {}", color::preset_name(st.kelvin)),
                &format!("{}K", st.kelvin),
                &pal,
            );
            widgets::slider(p, l.kelvin, kelvin_frac(st.kelvin), &pal, hot == Some(Hit::Kelvin), true);
            p.text(&st.schedule_text, l.schedule, 12.0, Weight::Regular, Align::Left, dimmed(pal.subtext));
            if let Some(link) = l.schedule_link {
                let c = if hot == Some(Hit::ReturnSchedule) { pal.text } else { pal.accent };
                p.text("Return to schedule", link, 12.0, Weight::Regular, Align::Right, c);
            }

            if !l.monitors.is_empty() {
                let cap_y = l.monitors[0].0.y - 24.0;
                p.text(
                    "Displays",
                    Rect::new(PAD, cap_y, 200.0, 20.0),
                    12.0,
                    Weight::Semibold,
                    Align::Left,
                    pal.subtext,
                );
                for (i, ((label, slider), m)) in l.monitors.iter().zip(&st.monitors).enumerate() {
                    let tag = if m.hardware { "backlight + software" } else { "software" };
                    p.text(
                        &format!("{}  ·  {tag}", m.name),
                        *label,
                        12.0,
                        Weight::Regular,
                        Align::Left,
                        pal.text,
                    );
                    p.text(
                        &format!("{:.0}%", m.brightness),
                        *label,
                        12.0,
                        Weight::Regular,
                        Align::Right,
                        pal.subtext,
                    );
                    widgets::slider(
                        p,
                        *slider,
                        (m.brightness - 1.0) / 99.0,
                        &pal,
                        hot == Some(Hit::Monitor(i)),
                        false,
                    );
                }
            }

            if !l.scenes.is_empty() {
                let cap_y = l.scenes[0].y - 24.0;
                p.text(
                    "Scenes",
                    Rect::new(PAD, cap_y, 200.0, 20.0),
                    12.0,
                    Weight::Semibold,
                    Align::Left,
                    pal.subtext,
                );
                for (i, (r, name)) in l.scenes.iter().zip(&st.scenes).enumerate() {
                    widgets::chip(p, *r, name, &pal, hot == Some(Hit::Scene(i)), st.active_scene == Some(i));
                }
            }

            widgets::separator(p, 0.0, WIDTH, l.pause.y - 12.0, pal.border);
            let (icon, label) = if st.paused { (glyph::PLAY, "Resume") } else { (glyph::PAUSE, "Pause") };
            widgets::button(p, l.pause, Some(icon), label, &pal, hot == Some(Hit::Pause), false);
            widgets::button(
                p,
                l.settings,
                Some(glyph::SETTINGS),
                "Settings",
                &pal,
                hot == Some(Hit::Settings),
                false,
            );
        });
        self.layout = l;
    }

    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_PAINT => unsafe {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(self.hwnd, &mut ps);
                self.paint();
                let _ = EndPaint(self.hwnd, &ps);
                Some(LRESULT(0))
            },
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_ACTIVATE => {
                if win::loword(wp.0) == WA_INACTIVE && self.drag.is_none() {
                    self.hide();
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN if wp.0 as u16 == VK_ESCAPE.0 => {
                self.hide();
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = self.dip(lp);
                self.on_down(x, y);
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = self.dip(lp);
                if let Some(h) = self.drag {
                    self.drag_to(h, x);
                } else {
                    let hot = hit_test(&self.layout, x, y);
                    if hot != self.hot {
                        self.hot = hot;
                        unsafe {
                            let _ = InvalidateRect(Some(self.hwnd), None, false);
                            let mut tme = windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<
                                    windows::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT,
                                >() as u32,
                                dwFlags: windows::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                                hwndTrack: self.hwnd,
                                dwHoverTime: 0,
                            };
                            let _ = windows::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.hot = None;
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if self.drag.take().is_some() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    self.send(Action::Commit);
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                // Wheel coordinates are in screen space.
                let (sx, sy) = win::point_from(lp.0 as usize);
                let mut pt = POINT { x: sx, y: sy };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
                }
                let s = self.surface.scale();
                self.on_wheel(pt.x as f32 / s, pt.y as f32 / s, win::hiword(wp.0) as u16 as i16);
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                self.position(None);
                Some(LRESULT(0))
            }
            WM_CLOSE => {
                self.hide();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

fn now_ms() -> u64 {
    unsafe { windows::Win32::System::SystemInformation::GetTickCount64() }
}

impl Drop for Flyout {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let f = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Flyout;
    if !f.is_null() {
        if let Some(r) = (*f).handle(msg, wp, lp) {
            return r;
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_mappings() {
        assert_eq!(kelvin_from_frac(0.0), color::MIN_KELVIN);
        assert_eq!(kelvin_from_frac(1.0), color::MAX_KELVIN);
        assert_eq!(kelvin_from_frac(0.5) % 50, 0);
        assert_eq!(brightness_from_frac(0.0), 1.0);
        assert_eq!(brightness_from_frac(1.0), 100.0);
    }

    #[test]
    fn layout_grows_with_content() {
        let base = State { scenes: vec!["A".into()], ..Default::default() };
        let a = layout(&base);
        let b = layout(&State {
            monitors: vec![MonitorRow::default(), MonitorRow::default()],
            deep_dim_countdown: Some(9),
            ..base.clone()
        });
        assert!(b.height > a.height + 80.0);
        assert_eq!(b.monitors.len(), 2);
        assert!(b.banner.is_some());
        // Buttons stay inside the window.
        assert!(a.settings.right() <= WIDTH - PAD + 0.01);
    }
}
