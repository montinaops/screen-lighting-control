//! The SLC glyph (a ring with the left half filled), rendered with analytic anti-aliasing.
//! Pure code: shared by the app (tray/window icons) and `build.rs` (the exe's .ico).

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
