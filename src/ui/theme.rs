//! Light/dark palettes. Follows the Windows app theme unless overridden in settings.

use super::d2d::{rgb, Color};
use crate::model;
use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Window background.
    pub bg: Color,
    /// Cards / raised controls.
    pub surface: Color,
    pub surface_hover: Color,
    pub border: Color,
    pub text: Color,
    pub subtext: Color,
    pub track: Color,
    /// Strongest ink of the theme (monotone design: white on dark, near-black on light).
    pub accent: Color,
    pub on_accent: Color,
    pub danger: Color,
}

/// Monotone palettes: grayscale only; the "accent" is simply the strongest ink of each theme.
pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x161616),
    surface: rgb(0x202020),
    surface_hover: rgb(0x2A2A2A),
    border: rgb(0x303030),
    text: rgb(0xF2F2F2),
    subtext: rgb(0x9A9A9A),
    track: rgb(0x3A3A3A),
    accent: rgb(0xF2F2F2),
    on_accent: rgb(0x161616),
    danger: rgb(0xE5484D),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xF7F7F7),
    surface: rgb(0xFFFFFF),
    surface_hover: rgb(0xF0F0F0),
    border: rgb(0xE2E2E2),
    text: rgb(0x161616),
    subtext: rgb(0x6B6B6B),
    track: rgb(0xD6D6D6),
    accent: rgb(0x161616),
    on_accent: rgb(0xFFFFFF),
    danger: rgb(0xCD2B31),
};

/// The logo's gradient for a palette: white → silver on dark, graphite → near-black on light
/// (the same inks as the tray icon, see `glyph::WHITE` / `glyph::INK`).
pub fn logo_ink(p: &Palette) -> (Color, Color) {
    let ink = if p.dark { crate::glyph::WHITE } else { crate::glyph::INK };
    (rgb(ink.from), rgb(ink.to))
}

/// Whether the taskbar (system surfaces) uses the light theme — decides the tray icon's ink.
pub fn taskbar_light() -> bool {
    reg_dword("SystemUsesLightTheme")
}

fn reg_dword(name: &str) -> bool {
    let mut v: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let n = crate::win::wide(name);
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            windows::core::PCWSTR(n.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && v != 0
}

/// Whether Windows apps are set to light mode.
pub fn system_light() -> bool {
    reg_dword("AppsUseLightTheme")
}

/// Makes standard Win32 popup menus (the tray menu) follow the theme. Uses uxtheme's app-mode
/// switch (ordinals 135/136: SetPreferredAppMode / FlushMenuThemes, Windows 10 1903+), the same
/// mechanism Explorer and Notepad++ use; silently does nothing where it isn't available.
pub fn apply_menu_theme(theme: model::Theme) {
    use windows::core::PCSTR;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    // PreferredAppMode: 1 = AllowDark (follow system), 2 = ForceDark, 3 = ForceLight.
    let mode: i32 = match theme {
        model::Theme::System => 1,
        model::Theme::Dark => 2,
        model::Theme::Light => 3,
    };
    unsafe {
        let Ok(ux) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(f) = GetProcAddress(ux, PCSTR(135 as *const u8)) {
            let set: extern "system" fn(i32) -> i32 = std::mem::transmute(f);
            set(mode);
        }
        if let Some(f) = GetProcAddress(ux, PCSTR(136 as *const u8)) {
            let flush: extern "system" fn() = std::mem::transmute(f);
            flush();
        }
    }
}

pub fn palette(theme: model::Theme) -> Palette {
    match theme {
        model::Theme::Light => LIGHT,
        model::Theme::Dark => DARK,
        model::Theme::System => {
            if system_light() {
                LIGHT
            } else {
                DARK
            }
        }
    }
}
