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

/// A monochrome gradient (0xRRGGBB) running diagonally from the top-left (`from`) to the
/// bottom-right (`to`) of the mark, like light falling on the moon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ink {
    pub from: u32,
    pub to: u32,
}

/// For dark surfaces: white fading to soft silver.
pub const WHITE: Ink = Ink { from: 0xFFFFFF, to: 0x969696 };
/// For light surfaces: graphite deepening to near-black.
pub const INK: Ink = Ink { from: 0x4A4A4A, to: 0x0A0A0A };
/// The app tile background: graphite at the top to near-black at the bottom.
pub const TILE: Ink = Ink { from: 0x3A3A3A, to: 0x080808 };

fn lerp_rgb(a: u32, b: u32, t: f32) -> u32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// Position (0–1) along the top-left → bottom-right diagonal of a box of radius `r` around (cx, cy).
fn diagonal(px: f32, py: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((px - (cx - r)) + (py - (cy - r))) / (4.0 * r)
}

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

/// Straight (non-premultiplied) BGRA pixels, row-major, top-down: the mark in `ink` on transparent.
pub fn render(size: u32, glyph: Glyph, ink: Ink) -> Vec<u32> {
    let n = size as f32;
    let c = n / 2.0;
    let opacity = if glyph == Glyph::Paused { 0.45 } else { 1.0 };
    let mut px = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let a = mark(fx, fy, c, c, n * 0.42, glyph) * opacity;
            if a > 0.0 {
                let color = lerp_rgb(ink.from, ink.to, diagonal(fx, fy, c, c, n * 0.42));
                px[(y * size + x) as usize] = (((a * 255.0).round() as u32) << 24) | color;
            }
        }
    }
    px
}

/// The app icon: a white-to-silver crescent on a graphite-to-black rounded tile (reads on light and
/// dark backgrounds).
pub fn render_tile(size: u32) -> Vec<u32> {
    let n = size as f32;
    let c = n / 2.0;
    let radius = n * 0.22;
    let half = n / 2.0 - 0.5;
    let mut px = vec![0u32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5 - c, y as f32 + 0.5 - c);
            // Rounded-square coverage (signed distance to a rounded box).
            let (qx, qy) = (fx.abs() - (half - radius), fy.abs() - (half - radius));
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius;
            let tile = (0.5 - outside).clamp(0.0, 1.0);
            if tile <= 0.0 {
                continue;
            }
            let (px_, py_) = (x as f32 + 0.5, y as f32 + 0.5);
            let bg = lerp_rgb(TILE.from, TILE.to, py_ / n);
            let m = mark(px_, py_, c, c, n * 0.30, Glyph::Normal);
            let fg = lerp_rgb(WHITE.from, WHITE.to, diagonal(px_, py_, c, c, n * 0.30));
            let color = lerp_rgb(bg, fg, m);
            px[(y * size + x) as usize] = (((tile * 255.0).round() as u32) << 24) | color;
        }
    }
    px
}
