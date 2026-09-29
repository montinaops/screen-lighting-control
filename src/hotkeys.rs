//! Global hotkeys: parsing/formatting bindings like `Win+Alt+Up` and registering them.

use crate::info;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub mods: u32,
    pub vk: u32,
}

const NAMED_KEYS: &[(&str, u16)] = &[
    ("Up", 0x26),
    ("Down", 0x28),
    ("Left", 0x25),
    ("Right", 0x27),
    ("Home", 0x24),
    ("End", 0x23),
    ("PgUp", 0x21),
    ("PageUp", 0x21),
    ("PgDn", 0x22),
    ("PageDown", 0x22),
    ("Insert", 0x2D),
    ("Ins", 0x2D),
    ("Delete", 0x2E),
    ("Del", 0x2E),
    ("Space", 0x20),
    ("Enter", 0x0D),
    ("Tab", 0x09),
    ("Pause", 0x13),
    ("Plus", 0xBB),
    ("Minus", 0xBD),
    ("Comma", 0xBC),
    ("Period", 0xBE),
    ("NumPlus", 0x6B),
    ("NumMinus", 0x6D),
];

fn key_from_name(name: &str) -> Option<u32> {
    if let Some((_, vk)) = NAMED_KEYS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*vk as u32);
    }
    let upper = name.to_ascii_uppercase();
    if upper.len() == 1 {
        let c = upper.as_bytes()[0];
        if c.is_ascii_alphanumeric() {
            return Some(c as u32);
        }
    }
    if let Some(n) = upper.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    if let Some(n) = upper.strip_prefix("NUM").and_then(|n| n.parse::<u32>().ok()) {
        if n <= 9 {
            return Some(0x60 + n);
        }
    }
    None
}

fn key_name(vk: u32) -> String {
    if let Some((n, _)) = NAMED_KEYS.iter().find(|(_, v)| *v as u32 == vk) {
        return n.to_string();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        _ => format!("0x{vk:02X}"),
    }
}

/// Parses `Ctrl+Alt+Shift+Win+Key` (any order, case-insensitive). At least one modifier is required.
pub fn parse(s: &str) -> Option<Binding> {
    let mut mods = 0u32;
    let mut vk = None;
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "alt" => mods |= MOD_ALT.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "windows" | "meta" => mods |= MOD_WIN.0,
            _ => {
                if vk.is_some() {
                    return None;
                }
                vk = Some(key_from_name(part)?);
            }
        }
    }
    let vk = vk?;
    (mods != 0).then_some(Binding { mods, vk })
}

pub fn format(b: Binding) -> String {
    let mut out = String::new();
    for (flag, name) in
        [(MOD_CONTROL.0, "Ctrl"), (MOD_WIN.0, "Win"), (MOD_ALT.0, "Alt"), (MOD_SHIFT.0, "Shift")]
    {
        if b.mods & flag != 0 {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&key_name(b.vk));
    out
}

/// Registers `(id, binding, repeat)` entries on `hwnd`, unregistering ids first.
/// Returns the ids that could not be registered (taken by another program or invalid).
pub fn register_all(hwnd: HWND, entries: &[(i32, String, bool)]) -> Vec<i32> {
    let mut failed = Vec::new();
    for (id, text, repeat) in entries {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), *id);
        }
        if text.trim().is_empty() {
            continue;
        }
        let Some(b) = parse(text) else {
            info!("hotkey {id}: invalid binding '{text}'");
            failed.push(*id);
            continue;
        };
        let mut mods = HOT_KEY_MODIFIERS(b.mods);
        if !repeat {
            mods |= MOD_NOREPEAT;
        }
        if unsafe { RegisterHotKey(Some(hwnd), *id, mods, b.vk) }.is_err() {
            info!("hotkey {id}: '{text}' is taken by another program");
            failed.push(*id);
        }
    }
    failed
}

pub fn unregister_all(hwnd: HWND, ids: impl IntoIterator<Item = i32>) {
    for id in ids {
        unsafe {
            let _ = UnregisterHotKey(Some(hwnd), id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_defaults() {
        let b = parse("Win+Alt+Up").unwrap();
        assert_eq!(b.mods, MOD_WIN.0 | MOD_ALT.0);
        assert_eq!(b.vk, 0x26);
        assert_eq!(parse("win + alt + pgdn").unwrap().vk, 0x22);
        assert_eq!(parse("Ctrl+Shift+F12").unwrap().vk, 0x7B);
        assert_eq!(parse("Alt+7").unwrap().vk, '7' as u32);
    }

    #[test]
    fn rejects_invalid() {
        assert!(parse("Up").is_none(), "needs a modifier");
        assert!(parse("Ctrl+Up+Down").is_none());
        assert!(parse("Ctrl+Bogus").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn format_is_canonical() {
        let b = parse("alt+win+pageup").unwrap();
        assert_eq!(format(b), "Win+Alt+PgUp");
        assert_eq!(parse(&format(b)), Some(b));
        assert_eq!(format(parse("ctrl+num5").unwrap()), "Ctrl+Num5");
    }
}
