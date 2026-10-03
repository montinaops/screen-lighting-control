//! On-screen display: a small popup that confirms hotkey changes, then fades out.

use super::d2d::{self, Align, Color, Rect, Surface, Weight};
use super::theme::Palette;
use crate::{color, win};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY,
    PAINTSTRUCT,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: windows::core::PCWSTR = w!("MONTINA.SLC.Osd");
const WIDTH: f32 = 264.0;
/// Messages widen the OSD to fit their text, up to this.
const MAX_WIDTH: f32 = 440.0;
const TEXT_X: f32 = 56.0;
const TEXT_PAD_RIGHT: f32 = 18.0;
const HEIGHT: f32 = 68.0;
const BOTTOM_MARGIN: f32 = 96.0;
const VISIBLE_MS: u32 = 1200;
const FADE_STEP_MS: u32 = 16;
const OPACITY: u8 = 245;
const TIMER_HIDE: usize = 1;
const TIMER_FADE: usize = 2;

#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    Brightness(f32),
    Warmth(u32),
    /// A title and a subtitle (scene name, pause state, ...).
    Message(String, String),
}

pub struct Osd {
    hwnd: HWND,
    surface: Surface,
    content: Content,
    palette: Palette,
    alpha: u8,
    /// Current width in DIPs (see `width_for`).
    width: f32,
}

/// OSD width that fits `content` (messages can be wider than the default).
fn width_for(content: &Content) -> f32 {
    let Content::Message(t, sub) = content else { return WIDTH };
    let text = d2d::measure(t, 15.0, Weight::Semibold).max(d2d::measure(sub, 12.0, Weight::Regular));
    (TEXT_X + text.ceil() + 2.0 + TEXT_PAD_RIGHT).clamp(WIDTH, MAX_WIDTH)
}

/// The color a white point looks like (for the warmth swatch).
pub fn kelvin_color(k: u32) -> Color {
    let w = color::white_point(k);
    Color { r: w[0], g: w[1], b: w[2], a: 1.0 }
}

impl Osd {
    pub fn create(palette: Palette) -> Option<Box<Osd>> {
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: win::hinstance(),
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut osd = Box::new(Osd {
                hwnd: HWND::default(),
                surface: Surface::new(HWND::default()),
                content: Content::Brightness(100.0),
                palette,
                alpha: 0,
                width: WIDTH,
            });
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                CLASS,
                w!("SLC OSD"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(win::hinstance()),
                Some(&mut *osd as *mut Osd as *const core::ffi::c_void),
            )
            .ok()?;
            osd.hwnd = hwnd;
            osd.surface = Surface::new(hwnd);
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as *const _,
                std::mem::size_of_val(&corner) as u32,
            );
            Some(osd)
        }
    }

    pub fn set_palette(&mut self, p: Palette) {
        self.palette = p;
    }

    /// Shows `content` near the bottom of the monitor under the mouse and restarts the fade timer.
    pub fn show(&mut self, content: Content) {
        self.show_for(content, VISIBLE_MS);
    }

    /// The OSD has faded out (or was never shown).
    pub fn hidden(&self) -> bool {
        self.alpha == 0
    }

    /// Title of the message on display, if the OSD shows a message.
    pub fn title(&self) -> Option<&str> {
        match &self.content {
            Content::Message(t, _) => Some(t),
            _ => None,
        }
    }

    /// Updates the text of a visible OSD without moving it or restarting its timer.
    pub fn update(&mut self, content: Content) {
        self.content = content;
        unsafe {
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// Like `show`, but stays visible for `visible_ms` before fading.
    pub fn show_for(&mut self, content: Content, visible_ms: u32) {
        self.width = width_for(&content);
        self.content = content;
        unsafe {
            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
            let mut mi =
                MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(hmon, &mut mi);
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let s = dx as f32 / 96.0;
            let (w, h) = ((self.width * s) as i32, (HEIGHT * s) as i32);
            let wa = mi.rcWork;
            let x = wa.left + ((wa.right - wa.left) - w) / 2;
            let y = wa.bottom - h - (BOTTOM_MARGIN * s) as i32;
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            self.alpha = OPACITY;
            let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), self.alpha, LWA_ALPHA);
            let _ = KillTimer(Some(self.hwnd), TIMER_FADE);
            SetTimer(Some(self.hwnd), TIMER_HIDE, visible_ms, None);
            let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(self.hwnd), None, false);
            let _ = windows::Win32::Graphics::Gdi::UpdateWindow(self.hwnd);
        }
    }

    fn paint(&mut self) {
        let pal = self.palette;
        let content = self.content.clone();
        let width = self.width;
        self.surface.paint(|p| {
            p.clear(pal.surface);
            let full = Rect::new(0.0, 0.0, width, HEIGHT);
            p.stroke_round(full, 8.0, pal.border, 1.0);
            let (cx, cy) = (30.0, HEIGHT / 2.0);
            let text_x = TEXT_X;
            let text_w = width - text_x - TEXT_PAD_RIGHT;
            let (title, value, frac, fill) = match &content {
                Content::Brightness(b) => {
                    p.logo(cx, cy, 11.0, pal.accent, pal.surface, false);
                    ("Brightness".to_string(), format!("{b:.0}%"), b / 100.0, pal.accent)
                }
                Content::Warmth(k) => {
                    // Monotone UI: the swatch is a muted hint of the real color.
                    let kc = d2d::mix(kelvin_color(*k), pal.track, 0.45);
                    p.circle(cx, cy, 11.0, kc);
                    p.ring(cx, cy, 11.0, pal.border, 1.0);
                    let frac =
                        (*k - color::MIN_KELVIN) as f32 / (color::MAX_KELVIN - color::MIN_KELVIN) as f32;
                    (format!("Warmth · {}", color::preset_name(*k)), format!("{k}K"), frac, pal.accent)
                }
                Content::Message(t, sub) => {
                    p.logo(cx, cy, 11.0, pal.accent, pal.surface, false);
                    p.text(
                        t,
                        Rect::new(text_x, 12.0, text_w, 24.0),
                        15.0,
                        Weight::Semibold,
                        Align::Left,
                        pal.text,
                    );
                    p.text(
                        sub,
                        Rect::new(text_x, 36.0, text_w, 20.0),
                        12.0,
                        Weight::Regular,
                        Align::Left,
                        pal.subtext,
                    );
                    return;
                }
            };
            p.text(
                &title,
                Rect::new(text_x, 12.0, text_w, 24.0),
                14.0,
                Weight::Semibold,
                Align::Left,
                pal.text,
            );
            p.text(
                &value,
                Rect::new(text_x, 12.0, text_w, 24.0),
                14.0,
                Weight::Regular,
                Align::Right,
                pal.subtext,
            );
            let bar = Rect::new(text_x, 44.0, text_w, 6.0);
            p.fill_round(bar, 3.0, pal.track);
            p.fill_round(Rect::new(bar.x, bar.y, (bar.w * frac.clamp(0.0, 1.0)).max(6.0), bar.h), 3.0, fill);
        });
    }

    fn on_timer(&mut self, id: usize) {
        unsafe {
            match id {
                TIMER_HIDE => {
                    let _ = KillTimer(Some(self.hwnd), TIMER_HIDE);
                    SetTimer(Some(self.hwnd), TIMER_FADE, FADE_STEP_MS, None);
                }
                TIMER_FADE => {
                    self.alpha = self.alpha.saturating_sub(28);
                    if self.alpha == 0 {
                        let _ = KillTimer(Some(self.hwnd), TIMER_FADE);
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                    } else {
                        let _ = SetLayeredWindowAttributes(self.hwnd, COLORREF(0), self.alpha, LWA_ALPHA);
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for Osd {
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
    let osd = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Osd;
    if osd.is_null() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            (*osd).paint();
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            (*osd).on_timer(wp.0);
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        // Owned by the app; only the app destroys it (e.g. `taskkill` without /F must not).
        WM_CLOSE => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
