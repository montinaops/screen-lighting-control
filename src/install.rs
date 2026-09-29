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

/// Sets `GdiICMGammaRange = 256` (needs administrator rights). Takes effect after signing in again.
pub fn expand_gamma_range() -> Result<(), String> {
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\\ICM"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|e| format!("cannot open the ICM key (administrator rights needed): {e}"))?;
        let ok = set_dword(key, "GdiICMGammaRange", 256);
        let _ = RegCloseKey(key);
        if ok {
            info!("GdiICMGammaRange set to 256");
            Ok(())
        } else {
            Err("cannot write GdiICMGammaRange".into())
        }
    }
}

/// Starts `slc.exe <args>` elevated (UAC prompt). Returns false if the user declined.
pub fn run_elevated(args: &str) -> bool {
    let exe = win::wide(&config::exe_path().display().to_string());
    let a = win::wide(args);
    let r = unsafe {
        ShellExecuteW(None, w!("runas"), PCWSTR(exe.as_ptr()), PCWSTR(a.as_ptr()), None, SW_SHOWNORMAL)
    };
    r.0 as isize > 32
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

// ----- install / uninstall (PRODUCT §10) ------------------------------------------------------

const UNINSTALL_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\MONTINA.SLC");
const SHORTCUT_NAME: &str = "Screen Lighting Control.lnk";

fn start_menu_shortcut() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|d| {
        std::path::PathBuf::from(d).join("Microsoft\\Windows\\Start Menu\\Programs").join(SHORTCUT_NAME)
    })
}

fn set_string(key: HKEY, name: &str, value: &str) -> bool {
    let n = win::wide(name);
    let v = win::wide(value);
    unsafe {
        let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
        RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_SZ, Some(bytes)).is_ok()
    }
}

fn set_dword(key: HKEY, name: &str, value: u32) -> bool {
    let n = win::wide(name);
    unsafe { RegSetValueExW(key, PCWSTR(n.as_ptr()), None, REG_DWORD, Some(&value.to_le_bytes())).is_ok() }
}

fn create_shortcut(lnk: &std::path::Path, target: &std::path::Path) -> windows::core::Result<()> {
    use windows::core::Interface;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        let t = win::wide(&target.display().to_string());
        link.SetPath(PCWSTR(t.as_ptr()))?;
        link.SetIconLocation(PCWSTR(t.as_ptr()), 0)?;
        link.SetDescription(w!("Screen Lighting Control — screen brightness and warmth"))?;
        if let Some(dir) = target.parent() {
            let d = win::wide(&dir.display().to_string());
            link.SetWorkingDirectory(PCWSTR(d.as_ptr()))?;
        }
        let file: IPersistFile = link.cast()?;
        let l = win::wide(&lnk.display().to_string());
        file.Save(PCWSTR(l.as_ptr()), true)
    }
}

/// Copies slc.exe into `%LOCALAPPDATA%\Programs\SLC`, migrates portable settings, and registers the
/// Start menu shortcut, uninstall entry and autostart. Returns the installed exe path.
pub fn install() -> Result<std::path::PathBuf, String> {
    let dir = config::install_dir().ok_or("LOCALAPPDATA is not set")?;
    let target = dir.join("slc.exe");
    let exe = config::exe_path();
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    if exe != target {
        // An older installed copy may be running: rename it out of the way, then copy.
        if target.exists() {
            let old = dir.join("slc.old.exe");
            let _ = std::fs::remove_file(&old);
            let _ = std::fs::rename(&target, &old);
        }
        std::fs::copy(&exe, &target).map_err(|e| format!("cannot copy slc.exe: {e}"))?;
    }
    // Move portable settings into the profile the first time.
    if let Some(roaming) = config::roaming_dir() {
        let portable_ini = exe.parent().map(|d| d.join("slc.ini"));
        let roaming_ini = roaming.join("slc.ini");
        if let Some(p) = portable_ini.filter(|p| p.exists() && *p != roaming_ini) {
            if !roaming_ini.exists() {
                let _ = std::fs::create_dir_all(&roaming);
                let _ = std::fs::copy(&p, &roaming_ini);
            }
        }
    }
    if let Some(lnk) = start_menu_shortcut() {
        if let Err(e) = create_shortcut(&lnk, &target) {
            info!("shortcut failed: {e}");
        }
    }
    unsafe {
        let mut key = HKEY::default();
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            UNINSTALL_KEY,
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .is_ok()
        {
            let t = target.display().to_string();
            set_string(key, "DisplayName", "Screen Lighting Control");
            set_string(key, "DisplayVersion", crate::VERSION);
            set_string(key, "Publisher", "MONTINA-Ops");
            set_string(key, "DisplayIcon", &format!("{t},0"));
            set_string(key, "InstallLocation", &dir.display().to_string());
            set_string(key, "UninstallString", &format!("\"{t}\" --uninstall"));
            set_string(key, "QuietUninstallString", &format!("\"{t}\" --uninstall --quiet"));
            set_dword(key, "NoModify", 1);
            set_dword(key, "NoRepair", 1);
            let kb = std::fs::metadata(&target).map(|m| m.len() / 1024).unwrap_or(0) as u32;
            set_dword(key, "EstimatedSize", kb.max(1));
            let _ = RegCloseKey(key);
        }
    }
    // Autostart points at the installed copy.
    let cmd = format!("\"{}\" --minimized", target.display());
    set_run_value(&cmd);
    info!("installed to {}", target.display());
    Ok(target)
}

fn set_run_value(cmd: &str) {
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
        .is_ok()
        {
            let v = win::wide(cmd);
            let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 2);
            let _ = RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes));
            let _ = RegCloseKey(key);
        }
    }
}

/// Removes the shortcut, uninstall entry and autostart; deletes the program folder after this
/// process exits; optionally deletes the settings folder.
pub fn uninstall(remove_settings: bool) -> Result<(), String> {
    set_autostart(false);
    if let Some(lnk) = start_menu_shortcut() {
        let _ = std::fs::remove_file(lnk);
    }
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, UNINSTALL_KEY);
    }
    if remove_settings {
        if let Some(r) = config::roaming_dir() {
            let _ = std::fs::remove_dir_all(r);
        }
    }
    if let Some(dir) = config::install_dir().filter(|d| d.exists()) {
        let exe = config::exe_path();
        if exe.parent() == Some(dir.as_path()) {
            // We are running from the folder: let cmd.exe remove it once we have exited.
            let script = format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
            spawn_hidden("cmd.exe", &script);
        } else {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    info!("uninstalled");
    Ok(())
}

/// Starts a program without a console window.
pub fn spawn_hidden(program: &str, args: &str) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new(program)
        .raw_arg(args)
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Starts `exe` with `args` (detached).
pub fn launch(exe: &std::path::Path, args: &[&str]) -> bool {
    use std::process::Stdio;
    // Detach stdio so a console that started us is not kept open by the new process.
    std::process::Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Simple modal message box. Returns true for Yes/OK.
pub fn ask(text: &str, yes_no: bool) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::*;
    let t = win::wide(text);
    let style = if yes_no { MB_YESNO | MB_ICONQUESTION } else { MB_OK | MB_ICONINFORMATION };
    let r = unsafe {
        MessageBoxW(None, PCWSTR(t.as_ptr()), w!("Screen Lighting Control"), style | MB_SETFOREGROUND)
    };
    r == IDYES || r == IDOK
}
