//! Overlay stage: one click-through, topmost, capture-excluded black window per monitor.
//! Used only for the dimming that hardware and gamma could not provide.

use crate::info;
use crate::win;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: windows::core::PCWSTR = w!("MONTINA.SLC.Overlay");

/// Converts an opacity (0–1) to the layered-window alpha byte.
pub fn alpha_byte(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

struct Overlay {
    hwnd: HWND,
    rect: RECT,
    alpha: u8,
}

/// All overlays, indexed like the monitor list.
pub struct Overlays {
    items: Vec<Option<Overlay>>,
    hook: Option<HWINEVENTHOOK>,
}

fn register_class() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_proc),
            hInstance: win::hinstance(),
            lpszClassName: CLASS,
            hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // Never hit-testable (belt and braces on top of WS_EX_TRANSPARENT).
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // Keep our exact monitor rectangle; the app repositions us after display changes.
        WM_DPICHANGED => LRESULT(0),
        // Owned by the app; ignore external close requests.
        WM_CLOSE => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Re-raises overlays whenever another window comes to the foreground (other topmost windows,
/// such as the taskbar, would otherwise cover them).
unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, _: HWND, _: i32, _: i32, _: u32, _: u32) {
    if let Some(list) = RAISE_LIST.with(|l| l.borrow().clone()) {
        raise(&list);
    }
}

thread_local! {
    static RAISE_LIST: std::cell::RefCell<Option<Vec<HWND>>> = const { std::cell::RefCell::new(None) };
}

fn raise(hwnds: &[HWND]) {
    for &h in hwnds {
        unsafe {
            let _ = SetWindowPos(
                h,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_NOSENDCHANGING,
            );
        }
    }
}

impl Overlays {
    pub fn new() -> Self {
        register_class();
        Overlays { items: Vec::new(), hook: None }
    }

    /// Adapts to a new monitor count (destroys everything; the next `set` recreates as needed).
    pub fn resize(&mut self, n: usize) {
        self.clear();
        self.items = (0..n).map(|_| None).collect();
    }

    /// Sets the opacity of monitor `i`'s overlay covering `rect`. Opacity 0 destroys the window,
    /// so an undimmed monitor costs nothing.
    pub fn set(&mut self, i: usize, rect: RECT, alpha: f32) {
        if i >= self.items.len() {
            self.items.resize_with(i + 1, || None);
        }
        let a = alpha_byte(alpha);
        match (&mut self.items[i], a) {
            (slot @ Some(_), 0) => {
                if let Some(o) = slot.take() {
                    unsafe {
                        let _ = DestroyWindow(o.hwnd);
                    }
                }
            }
            (None, 0) => {}
            (Some(o), a) => {
                if o.alpha != a {
                    unsafe {
                        let _ = SetLayeredWindowAttributes(o.hwnd, COLORREF(0), a, LWA_ALPHA);
                    }
                    o.alpha = a;
                }
                if o.rect != rect {
                    unsafe {
                        let _ = SetWindowPos(
                            o.hwnd,
                            Some(HWND_TOPMOST),
                            rect.left,
                            rect.top,
                            rect.right - rect.left,
                            rect.bottom - rect.top,
                            SWP_NOACTIVATE,
                        );
                    }
                    o.rect = rect;
                }
            }
            (slot @ None, a) => {
                *slot = create(rect, a).map(|hwnd| Overlay { hwnd, rect, alpha: a });
            }
        }
        self.update_hook();
    }

    pub fn hwnds(&self) -> Vec<HWND> {
        self.items.iter().flatten().map(|o| o.hwnd).collect()
    }

    /// Puts all overlays back on top (e.g. after the taskbar was recreated).
    pub fn raise_all(&self) {
        raise(&self.hwnds());
    }

    /// The foreground hook exists only while at least one overlay is visible.
    fn update_hook(&mut self) {
        let hwnds = self.hwnds();
        let want = !hwnds.is_empty();
        RAISE_LIST.with(|l| *l.borrow_mut() = want.then(|| hwnds.clone()));
        match (want, self.hook) {
            (true, None) => unsafe {
                let h = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_foreground),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                );
                if !h.is_invalid() {
                    self.hook = Some(h);
                }
            },
            (false, Some(h)) => unsafe {
                let _ = UnhookWinEvent(h);
                self.hook = None;
            },
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        for o in self.items.iter_mut().filter_map(|o| o.take()) {
            unsafe {
                let _ = DestroyWindow(o.hwnd);
            }
        }
        self.update_hook();
    }
}

impl Drop for Overlays {
    fn drop(&mut self) {
        self.clear();
    }
}

fn create(rect: RECT, alpha: u8) -> Option<HWND> {
    unsafe {
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            CLASS,
            w!(""),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            Some(win::hinstance()),
            None,
        )
        .ok()?;
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        // Hidden from screenshots / recordings / screen sharing (Windows 10 2004+).
        if SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_err() {
            info!("overlay: capture exclusion unavailable");
        }
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        Some(hwnd)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn alpha_bytes() {
        assert_eq!(super::alpha_byte(0.0), 0);
        assert_eq!(super::alpha_byte(1.0), 255);
        assert_eq!(super::alpha_byte(0.99), 252);
        assert_eq!(super::alpha_byte(-1.0), 0);
    }
}
