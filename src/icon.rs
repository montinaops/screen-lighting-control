//! Win32 icons (HICON) from the procedural glyph.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

pub use crate::glyph::{render, Glyph};

/// Monochrome mark in `color` (tray and small UI icons). The caller owns it (DestroyIcon).
pub fn create(size: u32, glyph: Glyph, color: u32) -> Option<HICON> {
    from_pixels(size, &render(size, glyph, color))
}

/// The app tile (window icons). The caller owns it (DestroyIcon).
pub fn create_tile(size: u32) -> Option<HICON> {
    from_pixels(size, &crate::glyph::render_tile(size))
}

fn from_pixels(size: u32, pixels: &[u32]) -> Option<HICON> {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size as i32,
                biHeight: -(size as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let screen = GetDC(None::<HWND>);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None::<HWND>, screen);
        let color = color.ok()?;
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let mask_bytes = vec![0u8; (size.div_ceil(16) * 2 * size) as usize];
        let mask = CreateBitmap(size as i32, size as i32, 1, 1, Some(mask_bytes.as_ptr() as _));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crescent_shape_and_transparent_corners() {
        let s = 32;
        let px = render(s, Glyph::Normal, crate::glyph::WHITE);
        assert_eq!(px[0] >> 24, 0, "corner must be transparent");
        // Lower-left of the disc is lit, upper-right is covered by the second disc.
        assert_eq!(px[(20 * s + 8) as usize] >> 24, 255, "crescent body");
        assert_eq!(px[(12 * s + 20) as usize] >> 24, 0, "covered part");
        assert_eq!(px[(20 * s + 8) as usize] & 0xFFFFFF, 0xFFFFFF, "monochrome white");
    }

    #[test]
    fn states() {
        let paused = render(32, Glyph::Paused, crate::glyph::INK);
        assert!((paused[(20 * 32 + 8) as usize] >> 24) < 128, "paused is translucent");
        let full = render(32, Glyph::Darkroom, crate::glyph::WHITE);
        assert_eq!(full[(12 * 32 + 20) as usize] >> 24, 255, "full disc when a filter is on");
    }

    #[test]
    fn tile_is_opaque_with_light_mark() {
        let px = crate::glyph::render_tile(64);
        assert_eq!(px[32 * 64 + 32] >> 24, 255);
        assert_eq!(px[0] >> 24, 0, "rounded corner");
    }
}
