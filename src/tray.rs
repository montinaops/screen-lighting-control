//! System tray icon (Shell_NotifyIcon, version 4) and its context menu.

use crate::icon::{self, Glyph};
use crate::win::{self, WM_APP_TRAY};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::HiDpi::GetSystemMetricsForDpi;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconGetRect, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICONDATAW, NOTIFYICONIDENTIFIER, NOTIFYICON_VERSION_4,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyIcon, DestroyMenu, SetForegroundWindow, TrackPopupMenu, HICON,
    MENU_ITEM_FLAGS, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, SM_CXSMICON, TPM_BOTTOMALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON,
};

const TRAY_ID: u32 = 1;

pub struct Tray {
    hwnd: HWND,
    icon: Option<HICON>,
    glyph: Glyph,
    tip: String,
}

impl Tray {
    pub fn new(hwnd: HWND) -> Self {
        let mut t = Tray { hwnd, icon: None, glyph: Glyph::Normal, tip: "Screen Lighting Control".into() };
        t.add();
        t
    }

    fn base(&self) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        }
    }

    fn make_icon(&mut self) {
        if let Some(old) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(old);
            }
        }
        let dpi = unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(self.hwnd) }.max(96);
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.max(16) as u32;
        self.icon = icon::create(size, self.glyph);
    }

    /// Adds the icon; also used after Explorer restarts ("TaskbarCreated").
    pub fn add(&mut self) {
        self.make_icon();
        let mut nid = self.base();
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = self.icon.unwrap_or_default();
        win::copy_wide(&mut nid.szTip, &self.tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_ADD, &nid);
            nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            let _ = Shell_NotifyIconW(NIM_SETVERSION, &nid);
        }
    }

    pub fn set_glyph(&mut self, glyph: Glyph) {
        if glyph == self.glyph && self.icon.is_some() {
            return;
        }
        self.glyph = glyph;
        self.make_icon();
        let mut nid = self.base();
        nid.uFlags = NIF_ICON;
        nid.hIcon = self.icon.unwrap_or_default();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn set_tip(&mut self, tip: &str) {
        if tip == self.tip {
            return;
        }
        self.tip = tip.to_string();
        let mut nid = self.base();
        nid.uFlags = NIF_TIP | NIF_SHOWTIP;
        win::copy_wide(&mut nid.szTip, tip);
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Shows a balloon / toast notification from the tray icon.
    pub fn notify(&self, title: &str, text: &str) {
        let mut nid = self.base();
        nid.uFlags = windows::Win32::UI::Shell::NIF_INFO;
        win::copy_wide(&mut nid.szInfoTitle, title);
        win::copy_wide(&mut nid.szInfo, text);
        nid.dwInfoFlags = windows::Win32::UI::Shell::NIIF_INFO;
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    /// Screen rectangle of the icon (for anchoring the flyout and wheel hit-testing).
    pub fn rect(&self) -> Option<RECT> {
        let id = NOTIFYICONIDENTIFIER {
            cbSize: std::mem::size_of::<NOTIFYICONIDENTIFIER>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        unsafe { Shell_NotifyIconGetRect(&id).ok() }
    }

    pub fn remove(&mut self) {
        let nid = self.base();
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            if let Some(i) = self.icon.take() {
                let _ = DestroyIcon(i);
            }
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        self.remove();
    }
}

/// One entry of a popup menu.
pub enum MenuItem {
    Item { id: u32, text: String, checked: bool, enabled: bool },
    Sub { text: String, items: Vec<MenuItem> },
    Separator,
}

impl MenuItem {
    pub fn item(id: u32, text: &str) -> Self {
        MenuItem::Item { id, text: text.into(), checked: false, enabled: true }
    }
    pub fn check(id: u32, text: &str, checked: bool) -> Self {
        MenuItem::Item { id, text: text.into(), checked, enabled: true }
    }
}

unsafe fn build(items: &[MenuItem]) -> windows::core::Result<windows::Win32::UI::WindowsAndMessaging::HMENU> {
    let menu = CreatePopupMenu()?;
    for it in items {
        match it {
            MenuItem::Item { id, text, checked, enabled } => {
                let mut f: MENU_ITEM_FLAGS = MF_STRING;
                if *checked {
                    f |= MF_CHECKED;
                }
                if !*enabled {
                    f |= MF_GRAYED;
                }
                let w = win::wide(text);
                AppendMenuW(menu, f, *id as usize, windows::core::PCWSTR(w.as_ptr()))?;
            }
            MenuItem::Sub { text, items } => {
                let sub = build(items)?;
                let w = win::wide(text);
                AppendMenuW(menu, MF_POPUP, sub.0 as usize, windows::core::PCWSTR(w.as_ptr()))?;
            }
            MenuItem::Separator => {
                AppendMenuW(menu, MF_SEPARATOR, 0, windows::core::PCWSTR::null())?;
            }
        }
    }
    Ok(menu)
}

/// Shows a popup menu at `pt` and returns the chosen command id (0 = cancelled).
pub fn popup(hwnd: HWND, pt: POINT, items: &[MenuItem]) -> u32 {
    unsafe {
        let Ok(menu) = build(items) else { return 0 };
        // Required so the menu closes when the user clicks elsewhere.
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
            pt.x,
            pt.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        cmd.0 as u32
    }
}
