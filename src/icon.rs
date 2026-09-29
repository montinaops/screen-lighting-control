//! Win32 icons (HICON) from the procedural glyph.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

pub use crate::glyph::{render, Glyph};

/// Creates an HICON from `render`. The caller owns it (DestroyIcon).
pub fn create(size: u32, glyph: Glyph) -> Option<HICON> {
    let pixels = render(size, glyph);
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
    fn glyph_has_left_fill_and_empty_corners() {
        let s = 32;
        let px = render(s, Glyph::Normal);
        assert_eq!(px[0] >> 24, 0, "corner must be transparent");
        let left = px[(16 * s + 10) as usize] >> 24;
        let right = px[(16 * s + 22) as usize] >> 24;
        assert_eq!(left, 255, "left half filled");
        assert_eq!(right, 0, "right half empty inside the ring");
    }

    #[test]
    fn paused_glyph_is_hollow() {
        let px = render(32, Glyph::Paused);
        assert_eq!(px[(16 * 32 + 10) as usize] >> 24, 0);
    }
}
