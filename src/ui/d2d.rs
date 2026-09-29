//! Direct2D / DirectWrite drawing layer shared by every SLC window.
//! All coordinates are in DIPs; the render target's DPI maps them to pixels.

use std::cell::RefCell;
use std::collections::HashMap;
use windows::core::{w, Interface};
use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows_numerics::Vector2;

pub type Color = D2D1_COLOR_F;

/// `0xRRGGBB` → opaque color.
pub const fn rgb(v: u32) -> Color {
    rgba(v, 1.0)
}

pub const fn rgba(v: u32, a: f32) -> Color {
    Color {
        r: ((v >> 16) & 0xFF) as f32 / 255.0,
        g: ((v >> 8) & 0xFF) as f32 / 255.0,
        b: (v & 0xFF) as f32 / 255.0,
        a,
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
    pub fn inset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, (self.w - 2.0 * dx).max(0.0), (self.h - 2.0 * dy).max(0.0))
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.right(), bottom: self.bottom() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Semibold,
    Bold,
    /// "Segoe MDL2 Assets" icon glyphs (Windows 10 and 11).
    Icon,
}

struct Factories {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    formats: RefCell<HashMap<(u32, Weight, Align), IDWriteTextFormat>>,
}

thread_local! {
    // Leaked on purpose: releasing DirectWrite/Direct2D factories during process teardown can deadlock
    // (the shared DWrite factory waits for worker threads that are already gone).
    static FACTORIES: Option<&'static Factories> = unsafe {
        let d2d = D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok();
        let dwrite = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok();
        match (d2d, dwrite) {
            (Some(d2d), Some(dwrite)) => {
                Some(&*Box::leak(Box::new(Factories { d2d, dwrite, formats: RefCell::new(HashMap::new()) })))
            }
            _ => None,
        }
    };
}

fn text_format(f: &Factories, size: f32, weight: Weight, align: Align) -> Option<IDWriteTextFormat> {
    let key = ((size * 10.0) as u32, weight, align);
    if let Some(tf) = f.formats.borrow().get(&key) {
        return Some(tf.clone());
    }
    let dw = match weight {
        Weight::Regular | Weight::Icon => DWRITE_FONT_WEIGHT_NORMAL,
        Weight::Semibold => DWRITE_FONT_WEIGHT_SEMI_BOLD,
        Weight::Bold => DWRITE_FONT_WEIGHT_BOLD,
    };
    let family = if weight == Weight::Icon { w!("Segoe MDL2 Assets") } else { w!("Segoe UI") };
    let tf = unsafe {
        let tf = f
            .dwrite
            .CreateTextFormat(
                family,
                None,
                dw,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
            .ok()?;
        let _ = tf.SetTextAlignment(match align {
            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
        });
        let _ = tf.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
        let _ = tf.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
        let trim =
            DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
        let _ = tf.SetTrimming(&trim, None);
        tf
    };
    f.formats.borrow_mut().insert(key, tf.clone());
    Some(tf)
}

/// Width of `text` in DIPs.
pub fn measure(text: &str, size: f32, weight: Weight) -> f32 {
    FACTORIES.with(|f| {
        let Some(f) = *f else { return 0.0 };
        let Some(tf) = text_format(f, size, weight, Align::Left) else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = f.dwrite.CreateTextLayout(&wide, &tf, 10_000.0, 1_000.0) else { return 0.0 };
            let mut m = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut m).is_ok() {
                m.widthIncludingTrailingWhitespace
            } else {
                0.0
            }
        }
    })
}

/// A window's render target (created lazily, recreated after device loss).
pub struct Surface {
    hwnd: HWND,
    rt: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    size: (u32, u32),
    dpi: f32,
}

impl Surface {
    pub fn new(hwnd: HWND) -> Surface {
        Surface { hwnd, rt: None, brush: None, size: (0, 0), dpi: 96.0 }
    }

    /// DIPs per pixel scale (dpi / 96).
    pub fn scale(&self) -> f32 {
        unsafe { GetDpiForWindow(self.hwnd).max(96) as f32 / 96.0 }
    }

    /// Draws a frame with `f`. Handles creation, resizing, DPI and device loss.
    pub fn paint(&mut self, f: impl FnOnce(&Painter)) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let size = ((rc.right - rc.left).max(1) as u32, (rc.bottom - rc.top).max(1) as u32);
        let dpi = unsafe { GetDpiForWindow(self.hwnd).max(96) } as f32;
        FACTORIES.with(|fac| {
            let Some(fac) = *fac else { return };
            unsafe {
                if self.rt.is_none() {
                    let props = D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: dpi,
                        dpiY: dpi,
                        ..Default::default()
                    };
                    let hprops = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: D2D_SIZE_U { width: size.0, height: size.1 },
                        presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                    };
                    let Ok(rt) = fac.d2d.CreateHwndRenderTarget(&props, &hprops) else { return };
                    self.brush = rt.CreateSolidColorBrush(&rgb(0), None).ok();
                    self.rt = Some(rt);
                    self.size = size;
                    self.dpi = dpi;
                }
                let Some(rt) = self.rt.clone() else { return };
                if self.size != size {
                    let _ = rt.Resize(&D2D_SIZE_U { width: size.0, height: size.1 });
                    self.size = size;
                }
                if self.dpi != dpi {
                    rt.SetDpi(dpi, dpi);
                    self.dpi = dpi;
                }
                let Some(brush) = self.brush.clone() else { return };
                rt.BeginDraw();
                let target: ID2D1RenderTarget = rt.cast().expect("hwnd target is a render target");
                let p = Painter { rt: target, brush, fac };
                f(&p);
                if let Err(e) = rt.EndDraw(None, None) {
                    if e.code() == D2DERR_RECREATE_TARGET {
                        self.rt = None;
                        self.brush = None;
                    }
                }
            }
        });
    }
}

pub struct Painter<'a> {
    rt: ID2D1RenderTarget,
    brush: ID2D1SolidColorBrush,
    fac: &'a Factories,
}

impl Painter<'_> {
    fn b(&self, c: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&c) };
        &self.brush
    }

    pub fn clear(&self, c: Color) {
        unsafe { self.rt.Clear(Some(&c)) };
    }

    pub fn fill(&self, r: Rect, c: Color) {
        unsafe { self.rt.FillRectangle(&r.d2d(), self.b(c)) };
    }

    pub fn fill_round(&self, r: Rect, radius: f32, c: Color) {
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillRoundedRectangle(&rr, self.b(c)) };
    }

    pub fn stroke_round(&self, r: Rect, radius: f32, c: Color, width: f32) {
        let r = r.inset(width / 2.0, width / 2.0);
        let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius };
        unsafe { self.rt.DrawRoundedRectangle(&rr, self.b(c), width, None) };
    }

    pub fn circle(&self, cx: f32, cy: f32, radius: f32, c: Color) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillEllipse(&e, self.b(c)) };
    }

    pub fn ring(&self, cx: f32, cy: f32, radius: f32, c: Color, width: f32) {
        let e = D2D1_ELLIPSE { point: Vector2 { X: cx, Y: cy }, radiusX: radius, radiusY: radius };
        unsafe { self.rt.DrawEllipse(&e, self.b(c), width, None) };
    }

    pub fn line(&self, x0: f32, y0: f32, x1: f32, y1: f32, c: Color, width: f32) {
        unsafe {
            self.rt.DrawLine(Vector2 { X: x0, Y: y0 }, Vector2 { X: x1, Y: y1 }, self.b(c), width, None)
        };
    }

    pub fn text(&self, s: &str, r: Rect, size: f32, weight: Weight, align: Align, c: Color) {
        let Some(tf) = text_format(self.fac, size, weight, align) else { return };
        let wide: Vec<u16> = s.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(
                &wide,
                &tf,
                &r.d2d(),
                self.b(c),
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            )
        };
    }

    /// Clips drawing to `r` until the returned guard is dropped.
    pub fn clip(&self, r: Rect) -> ClipGuard<'_> {
        unsafe { self.rt.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        ClipGuard { rt: &self.rt }
    }
}

pub struct ClipGuard<'a> {
    rt: &'a ID2D1RenderTarget,
}

impl Drop for ClipGuard<'_> {
    fn drop(&mut self) {
        unsafe { self.rt.PopAxisAlignedClip() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_and_rects() {
        let c = rgb(0xFF8000);
        assert_eq!((c.r, c.b, c.a), (1.0, 0.0, 1.0));
        assert!((c.g - 128.0 / 255.0).abs() < 1e-6);
        let m = mix(rgb(0x000000), rgb(0xFFFFFF), 0.5);
        assert!((m.r - 0.5).abs() < 1e-6);
        let r = Rect::new(10.0, 10.0, 100.0, 20.0);
        assert!(r.contains(10.0, 29.9) && !r.contains(110.0, 15.0));
        assert_eq!(r.inset(5.0, 5.0), Rect::new(15.0, 15.0, 90.0, 10.0));
    }
}
