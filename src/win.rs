//! Small Win32 helpers shared by all modules.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

/// Private window messages.
pub const WM_APP_TRAY: u32 = 0x8000 + 1;
/// Another instance asked us to show ourselves.
pub const WM_APP_ACTIVATE: u32 = 0x8000 + 2;
/// Worker threads queued results (see `engine::Events`).
pub const WM_APP_ENGINE: u32 = 0x8000 + 3;

/// `COPYDATASTRUCT::dwData` tag for forwarded command lines ("SLC").
pub const COPYDATA_FORWARD: usize = 0x534C43;

/// Window class of the hidden controller window (also used to find the running instance).
pub const CONTROLLER_CLASS: PCWSTR = windows::core::w!("MONTINA.SLC.Controller");
pub const INSTANCE_MUTEX: PCWSTR = windows::core::w!("Local\\MONTINA.SLC.Instance");

/// NUL-terminated UTF-16 copy of `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 buffer (possibly NUL-terminated) to `String`.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Copies `s` into a fixed-size UTF-16 array, truncating and always NUL-terminating.
pub fn copy_wide<const N: usize>(dst: &mut [u16; N], s: &str) {
    let mut i = 0;
    for c in s.encode_utf16() {
        if i + 1 >= N {
            break;
        }
        dst[i] = c;
        i += 1;
    }
    dst[i] = 0;
}

pub fn hinstance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default() }
}

#[inline]
pub fn loword(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}

#[inline]
pub fn hiword(v: usize) -> u32 {
    ((v >> 16) & 0xFFFF) as u32
}

/// Signed x/y from an LPARAM or WPARAM that packs two 16-bit coordinates.
#[inline]
pub fn point_from(v: usize) -> (i32, i32) {
    ((v & 0xFFFF) as u16 as i16 as i32, ((v >> 16) & 0xFFFF) as u16 as i16 as i32)
}

pub fn post(hwnd: HWND, msg: u32, w: usize, l: isize) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(hwnd), msg, WPARAM(w), LPARAM(l));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_roundtrip() {
        let w = wide("SLC é");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(from_wide(&w), "SLC é");
    }

    #[test]
    fn copy_wide_truncates() {
        let mut buf = [0u16; 4];
        copy_wide(&mut buf, "abcdef");
        assert_eq!(from_wide(&buf), "abc");
    }

    #[test]
    fn packed_points_are_signed() {
        let v = ((-5i16 as u16 as usize) << 16) | (10u16 as usize);
        assert_eq!(point_from(v), (10, -5));
    }
}
