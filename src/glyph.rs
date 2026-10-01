//! The SLC mark: an eclipse (a disc partly covered by another), rendered with analytic
//! anti-aliasing. Pure code: shared by the app (tray/window icons) and `build.rs` (the exe's .ico).

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// Crescent.
    Normal,
    /// Crescent at reduced opacity.
    Paused,
    /// Full disc ("total eclipse"): a color filter such as Darkroom is on.
    Darkroom,
}

/// Monochrome colors (0xRRGGBB) for icons on dark and light surfaces.
pub const WHITE: u32 = 0xFFFFFF;
pub const INK: u32 = 0x1A1A1A;

fn coverage(d: f32, r: f32) -> f32 {
    (r - d + 0.5).clamp(0.0, 1.0)
}

/// Mark coverage (0–1) at pixel center (px, py) for a mark of radius `r` centered at (cx, cy).
fn mark(px: f32, py: f32, cx: f32, cy: f32, r: f32, glyph: Glyph) -> f32 {
    let (dx, dy) = (px - cx, py - cy);
    let disc = coverage((dx * dx + dy * dy).sqrt(), r);
    if glyph == Glyph::Darkroom {
        return disc;
    }
    // The covering disc sits up and to the right, leaving a crescent on the lower left.
    let (ox, oy) = (cx + r * 0.52, cy - r * 0.30);
    let (ex, ey) = (px - ox, py - oy);
    let cut = coverage((ex * ex + ey * ey).sqrt(), r * 0.86);
    disc * (1.0 - cut)
}

/// Straight (non-premultiplied) BGRA pixels, row-major, top-down: the mark in `color` on transparent.
pub fn render(size: u32, glyph: Glyph, color: u32) -> Vec<u32> {
    let n = size as f32;
    let c = n / 2.0;
    let opacity = if glyph == Glyph::Paused { 0.45 } else { 1.0 };
    let mut px = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let a = mark(x as f32 + 0.5, y as f32 + 0.5, c, c, n * 0.42, glyph) * opacity;
            if a > 0.0 {
                px[(y * size + x) as usize] = (((a * 255.0).round() as u32) << 24) | (color & 0x00FF_FFFF);
            }
        }
    }
    px
}

/// The app icon: just the white moon on a transparent background (no tile).
pub fn render_app_icon(size: u32) -> Vec<u32> {
    render(size, Glyph::Normal, WHITE)
}
