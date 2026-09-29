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
    /// SLC amber (matches the tray icon).
    pub accent: Color,
    pub on_accent: Color,
    pub danger: Color,
}

pub const DARK: Palette = Palette {
    dark: true,
    bg: rgb(0x1F1F1F),
    surface: rgb(0x2B2B2B),
    surface_hover: rgb(0x353535),
    border: rgb(0x3C3C3C),
    text: rgb(0xFFFFFF),
    subtext: rgb(0xA8A8A8),
    track: rgb(0x4A4A4A),
    accent: rgb(0xFFB32E),
    on_accent: rgb(0x1A1A1A),
    danger: rgb(0xF0605A),
};

pub const LIGHT: Palette = Palette {
    dark: false,
    bg: rgb(0xF3F3F3),
    surface: rgb(0xFFFFFF),
    surface_hover: rgb(0xF0F0F0),
    border: rgb(0xDDDDDD),
    text: rgb(0x1A1A1A),
    subtext: rgb(0x5F5F5F),
    track: rgb(0xCFCFCF),
    accent: rgb(0xE08E00),
    on_accent: rgb(0xFFFFFF),
    danger: rgb(0xC42B1C),
};

/// Whether Windows apps are set to light mode.
pub fn system_light() -> bool {
    let mut v: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let ok = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut v as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    ok.is_ok() && v != 0
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
