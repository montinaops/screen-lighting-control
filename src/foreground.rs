//! Foreground-app detection for per-app rules and fullscreen pausing.

use crate::win;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Posted to the controller when the foreground window changes (lParam = HWND).
pub const WM_APP_FOREGROUND: u32 = 0x8000 + 13;

thread_local! {
    static TARGET: std::cell::Cell<isize> = const { std::cell::Cell::new(0) };
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    let target = TARGET.with(|t| t.get());
    if target != 0 {
        win::post(HWND(target as *mut _), WM_APP_FOREGROUND, 0, hwnd.0 as isize);
    }
}

/// Foreground-change hook; exists only while rules need it.
#[derive(Default)]
pub struct Watcher {
    hook: Option<HWINEVENTHOOK>,
}

impl Watcher {
    pub fn set_enabled(&mut self, controller: HWND, on: bool) {
        match (on, self.hook) {
            (true, None) => unsafe {
                TARGET.with(|t| t.set(controller.0 as isize));
                let h = SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_SYSTEM_FOREGROUND,
                    None,
                    Some(on_event),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT,
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
}

impl Drop for Watcher {
    fn drop(&mut self) {
        if let Some(h) = self.hook.take() {
            unsafe {
                let _ = UnhookWinEvent(h);
            }
        }
    }
}

/// Lowercase executable file name of the process owning `hwnd`, and whether it is SLC itself.
pub fn exe_of(hwnd: HWND) -> Option<(String, bool)> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let own = pid == GetCurrentProcessId();
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let name = path.rsplit('\\').next()?.to_ascii_lowercase();
        Some((name, own))
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// True if `hwnd` covers its whole monitor (a fullscreen game, video or presentation).
pub fn is_fullscreen(hwnd: HWND) -> bool {
    if hwnd.is_invalid() {
        return false;
    }
    let class = class_of(hwnd);
    if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return false;
    }
    unsafe {
        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_err() {
            return false;
        }
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        wr.left <= m.left && wr.top <= m.top && wr.right >= m.right && wr.bottom >= m.bottom
    }
}
