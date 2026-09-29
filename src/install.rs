//! Windows integration: autostart (Run key), clipboard, opening folders.

use crate::{config, info, win};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Screen Lighting Control");
const CF_UNICODETEXT: u32 = 13;

/// Whether the running exe is the installed copy.
pub fn is_installed() -> bool {
    let exe = config::exe_path();
    config::install_dir().is_some_and(|d| exe.parent() == Some(d.as_path()))
}

/// `GdiICMGammaRange` is set (Windows allows the full gamma range).
pub fn gamma_range_expanded() -> bool {
    let mut v: u32 = 0;
    let mut size = 4u32;
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ICM"),
            w!("GdiICMGammaRange"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
        .is_ok()
            && v >= 256
    }
}

/// Windows Night Light is currently active (it fights with SLC over the gamma ramp).
/// Night Light stores its state in a CloudStore blob; byte 18 is 0x15 when it is on.
pub fn night_light_on() -> bool {
    let mut buf = [0u8; 64];
    let mut size = buf.len() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\CloudStore\\Store\\DefaultAccount\\Current\\default$windows.data.bluelightreduction.bluelightreductionstate\\windows.data.bluelightreduction.bluelightreductionstate"),
            w!("Data"),
            RRF_RT_REG_BINARY,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .is_ok()
    };
    ok && size > 18 && buf[18] == 0x15
}

/// The command line used for autostart.
pub fn autostart_command() -> String {
    format!("\"{}\" --minimized", config::exe_path().display())
}

/// Adds or removes the per-user Run entry.
pub fn set_autostart(on: bool) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_err()
        {
            return false;
        }
        let ok = if on {
            let cmd = win::wide(&autostart_command());
            let bytes = std::slice::from_raw_parts(cmd.as_ptr() as *const u8, cmd.len() * 2);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes)).is_ok()
        } else {
            let r = RegDeleteValueW(key, RUN_VALUE);
            r.is_ok() || r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND
        };
        let _ = RegCloseKey(key);
        info!("autostart {} ({})", if on { "on" } else { "off" }, if ok { "ok" } else { "failed" });
        ok
    }
}

/// Current Run entry, if any.
pub fn autostart_entry() -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    Some(win::from_wide(&buf))
}

pub fn copy_to_clipboard(owner: HWND, text: &str) -> bool {
    let wide = win::wide(text);
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let ok = (|| {
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok()?;
            let p = GlobalLock(mem) as *mut u16;
            if p.is_null() {
                return None;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
            let _ = GlobalUnlock(mem);
            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(mem.0))).ok()
        })()
        .is_some();
        let _ = CloseClipboard();
        ok
    }
}

pub fn open_folder(path: &std::path::Path) {
    let p = win::wide(&path.display().to_string());
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(p.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}
