//! "Identify displays": a large number shown in the middle of each monitor for a few seconds.

use super::d2d::{Align, Rect, Surface, Weight};
use super::theme::Palette;
use crate::monitors::Monitor;
use crate::win;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, PAINTSTRUCT};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: windows::core::PCWSTR = w!("MONTINA.SLC.Identify");
const SIZE: f32 = 180.0;
const SHOW_MS: u32 = 2500;

struct Tile {
    surface: Surface,
    number: usize,
    palette: Palette,
}

pub fn show(monitors: &[Monitor], palette: Palette) {
    unsafe {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc),
            hInstance: win::hinstance(),
            lpszClassName: CLASS,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        for (i, m) in monitors.iter().enumerate() {
            let (mut dx, mut dy) = (96u32, 96u32);
            let _ = GetDpiForMonitor(m.hmon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            let px = (SIZE * dx as f32 / 96.0) as i32;
            let x = m.rect.left + (m.width() - px) / 2;
            let y = m.rect.top + (m.height() - px) / 2;
            let tile = Box::into_raw(Box::new(Tile {
                surface: Surface::new(HWND::default()),
                number: i + 1,
                palette,
            }));
            let Ok(hwnd) = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                CLASS,
                w!(""),
                WS_POPUP,
                x,
                y,
                px,
                px,
                None,
                None,
                Some(win::hinstance()),
                Some(tile as *const core::ffi::c_void),
            ) else {
                drop(Box::from_raw(tile));
                continue;
            };
            (*tile).surface = Surface::new(hwnd);
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 235, LWA_ALPHA);
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(hwnd), 1, SHOW_MS, None);
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let tile = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Tile;
    match msg {
        WM_PAINT if !tile.is_null() => {
            let mut ps = PAINTSTRUCT::default();
            BeginPaint(hwnd, &mut ps);
            let t = &mut *tile;
            let (n, pal) = (t.number, t.palette);
            t.surface.paint(|p| {
                p.clear(pal.surface);
                p.stroke_round(Rect::new(0.0, 0.0, SIZE, SIZE), 12.0, pal.accent, 4.0);
                p.text(
                    &n.to_string(),
                    Rect::new(0.0, 0.0, SIZE, SIZE),
                    96.0,
                    Weight::Semibold,
                    Align::Center,
                    pal.text,
                );
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_TIMER => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_NCDESTROY if !tile.is_null() => {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            drop(Box::from_raw(tile));
            LRESULT(0)
        }
        WM_NCHITTEST => LRESULT(HTTRANSPARENT as isize),
        WM_ERASEBKGND => LRESULT(1),
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
