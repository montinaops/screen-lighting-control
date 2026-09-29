//! Dimmed mouse cursor: the cursor is drawn above every window (including SLC's overlay), so at deep
//! dimming it would stay bright. When enabled, the system cursors are replaced by darkened copies and
//! restored with `SPI_SETCURSORS` on exit, reset and crash recovery.

use crate::info;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Standard cursors that are replaced (OCR_* ids).
const CURSORS: &[u32] =
    &[32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650, 32651];

/// Brightness factors are quantized to this step to avoid rebuilding cursors on every small change.
const STEP: f32 = 0.1;

pub fn quantize(factor: f32) -> f32 {
    ((factor.clamp(0.0, 1.0) / STEP).round() * STEP).clamp(0.1, 1.0)
}

/// Multiplies the RGB of premultiplied-or-straight BGRA pixels by `f` (alpha kept).
pub fn dim_pixels(px: &mut [u32], f: f32) {
    for p in px.iter_mut() {
        let a = *p & 0xFF00_0000;
        let ch = |shift: u32| ((((*p >> shift) & 0xFF) as f32 * f).round() as u32) << shift;
        *p = a | ch(16) | ch(8) | ch(0);
    }
}

#[derive(Default)]
pub struct CursorDimmer {
    /// Factor currently applied (None = system cursors untouched).
    applied: Option<f32>,
}

impl CursorDimmer {
    /// Sets the cursor brightness factor (1 = normal). Returns quickly when nothing changes.
    pub fn set(&mut self, factor: Option<f32>) {
        let want = factor.map(quantize).filter(|f| *f < 0.999);
        if want == self.applied {
            return;
        }
        match want {
            None => restore(),
            Some(f) => {
                // Always start from the user's scheme so repeated dimming does not compound.
                restore();
                let n = CURSORS.iter().filter(|&&id| replace(id, f)).count();
                info!("cursor dimmed to {:.0}% ({n} cursors)", f * 100.0);
            }
        }
        self.applied = want;
    }
}

impl Drop for CursorDimmer {
    fn drop(&mut self) {
        if self.applied.is_some() {
            restore();
        }
    }
}

/// Reloads the user's cursor scheme (undoes any replacement). Safe to call any time.
pub fn restore() {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
}

fn read_bitmap(hbm: HBITMAP) -> Option<(i32, i32, Vec<u32>)> {
    unsafe {
        let mut bm = BITMAP::default();
        if GetObjectW(hbm.into(), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _))
            == 0
        {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u32; (w * h) as usize];
        let dc = GetDC(None::<HWND>);
        let got = GetDIBits(dc, hbm, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(None::<HWND>, dc);
        (got != 0).then_some((w, h, px))
    }
}

fn make_color_bitmap(w: i32, h: i32, px: &[u32]) -> Option<HBITMAP> {
    unsafe {
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = GetDC(None::<HWND>);
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None::<HWND>, dc);
        let hbm = hbm.ok()?;
        std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u32, px.len());
        Some(hbm)
    }
}

/// Average brightness (0–255) of the visible pixels of the current arrow cursor (diagnostics).
pub fn arrow_brightness() -> Option<(f32, usize, bool)> {
    unsafe {
        let src = LoadCursorW(None, IDC_ARROW).ok()?;
        let mut ii = ICONINFO::default();
        GetIconInfo(src.into(), &mut ii).ok()?;
        let color = !ii.hbmColor.is_invalid();
        let (_, _, px) = read_bitmap(if color { ii.hbmColor } else { ii.hbmMask })?;
        if color {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        let _ = DeleteObject(ii.hbmMask.into());
        let lit: Vec<f32> = px
            .iter()
            .filter(|p| *p >> 24 != 0 || !color)
            .map(|p| (((p >> 16) & 0xFF) + ((p >> 8) & 0xFF) + (p & 0xFF)) as f32 / 3.0)
            .collect();
        let n = lit.len();
        Some((if n == 0 { 0.0 } else { lit.iter().sum::<f32>() / n as f32 }, n, color))
    }
}

/// Replaces system cursor `id` with a copy dimmed by `f`.
fn replace(id: u32, f: f32) -> bool {
    unsafe {
        let Ok(src) = LoadCursorW(None, windows::core::PCWSTR(id as usize as *const u16)) else {
            return false;
        };
        let mut ii = ICONINFO::default();
        if GetIconInfo(src.into(), &mut ii).is_err() {
            return false;
        }
        let result = (|| {
            let (w, h, mut px) = if !ii.hbmColor.is_invalid() {
                let (w, h, px) = read_bitmap(ii.hbmColor)?;
                // Color cursors without alpha rely on the mask: give opaque pixels full alpha.
                let has_alpha = px.iter().any(|p| p >> 24 != 0);
                let px = if has_alpha {
                    px
                } else {
                    let (_, _, mask) = read_bitmap(ii.hbmMask)?;
                    px.iter()
                        .zip(mask.iter())
                        .map(|(c, m)| if m & 0x00FF_FFFF == 0 { c | 0xFF00_0000 } else { 0 })
                        .collect()
                };
                (w, h, px)
            } else {
                // Monochrome: the mask holds AND (top half) and XOR (bottom half).
                let (w, h2, mask) = read_bitmap(ii.hbmMask)?;
                let h = h2 / 2;
                let px = (0..(w * h) as usize)
                    .map(|i| {
                        let and = mask[i] & 0x00FF_FFFF != 0;
                        let xor = mask[i + (w * h) as usize] & 0x00FF_FFFF != 0;
                        match (and, xor) {
                            (false, false) => 0xFF00_0000,
                            (false, true) | (true, true) => 0xFFFF_FFFF,
                            (true, false) => 0,
                        }
                    })
                    .collect();
                (w, h, px)
            };
            dim_pixels(&mut px, f);
            let color = make_color_bitmap(w, h, &px)?;
            let mask_bytes = vec![0u8; ((w as usize).div_ceil(16) * 2) * h as usize];
            let mask = CreateBitmap(w, h, 1, 1, Some(mask_bytes.as_ptr() as _));
            let info = ICONINFO {
                fIcon: false.into(),
                xHotspot: ii.xHotspot,
                yHotspot: ii.yHotspot,
                hbmMask: mask,
                hbmColor: color,
            };
            let cur = CreateIconIndirect(&info).ok();
            let _ = DeleteObject(color.into());
            let _ = DeleteObject(mask.into());
            // SetSystemCursor takes ownership of the new cursor.
            cur.map(|c| SetSystemCursor(HCURSOR(c.0), SYSTEM_CURSOR_ID(id)).is_ok())
        })();
        if !ii.hbmColor.is_invalid() {
            let _ = DeleteObject(ii.hbmColor.into());
        }
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(ii.hbmMask.into());
        }
        result.unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizes_to_steps() {
        assert!((quantize(0.44) - 0.4).abs() < 1e-6);
        assert!((quantize(0.0) - 0.1).abs() < 1e-6, "never fully invisible");
        assert_eq!(quantize(1.2), 1.0);
    }

    #[test]
    fn dims_rgb_keeps_alpha() {
        let mut px = [0xFF80_4020u32, 0x0000_0000];
        dim_pixels(&mut px, 0.5);
        assert_eq!(px[0], 0xFF40_2010);
        assert_eq!(px[1], 0);
    }
}
