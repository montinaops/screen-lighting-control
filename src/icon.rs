//! Procedural tray icon (no resources needed): a ring with the left half filled,
//! the usual "brightness" glyph. Drawn with analytic anti-aliasing.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Normal,
    Paused,
    Darkroom,
}

/// Straight (non-premultiplied) BGRA pixels, row-major, top-down.
pub fn render(size: u32, glyph: Glyph) -> Vec<u32> {
    let (r, g, b) = match glyph {
        Glyph::Normal => (0xFF, 0xB3, 0x2E),
        Glyph::Paused => (0xA0, 0xA0, 0xA0),
        Glyph::Darkroom => (0xE0, 0x30, 0x30),
    };
    let n = size as f32;
    let c = n / 2.0;
    let outer = n * 0.44;
    let ring = (n * 0.10).max(1.5);
    let inner = outer - ring - (n * 0.07).max(1.0);
    let mut px = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5 - c;
            let fy = y as f32 + 0.5 - c;
            let d = (fx * fx + fy * fy).sqrt();
            // Ring coverage: inside `outer`, outside `outer - ring`.
            let ring_cov = (outer - d + 0.5).clamp(0.0, 1.0) * (d - (outer - ring) + 0.5).clamp(0.0, 1.0);
            // Left half disk (paused glyph draws it hollow).
            let disk = if glyph == Glyph::Paused {
                0.0
            } else {
                (inner - d + 0.5).clamp(0.0, 1.0) * (-fx + 0.5).clamp(0.0, 1.0)
            };
            let a = ring_cov.max(disk);
            if a > 0.0 {
                let a8 = (a * 255.0).round() as u32;
                px[(y * size + x) as usize] = (a8 << 24) | (r << 16) | (g << 8) | b;
            }
        }
    }
    px
}

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
