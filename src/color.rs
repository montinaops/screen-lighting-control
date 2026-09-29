//! Color math: Kelvin → white point, gamma ramps, and named temperature presets.
//!
//! All factors here work on gamma-*encoded* values (what the GPU lookup table maps and what DWM blends
//! in). Encoded values are roughly perceptually uniform, so a linear slider in this space feels even.

pub const MIN_KELVIN: u32 = 1200;
pub const MAX_KELVIN: u32 = 6500;
pub const NEUTRAL_KELVIN: u32 = 6500;

/// f.lux-compatible preset names.
pub const PRESETS: &[(u32, &str)] = &[
    (1200, "Ember"),
    (1900, "Candle"),
    (2300, "Warm Incandescent"),
    (2700, "Incandescent"),
    (3400, "Halogen"),
    (4200, "Fluorescent"),
    (6500, "Daylight"),
];

/// Name of the preset closest to `k` (e.g. for the flyout label).
pub fn preset_name(k: u32) -> &'static str {
    PRESETS.iter().min_by_key(|(pk, _)| pk.abs_diff(k)).map(|p| p.1).unwrap_or("Custom")
}

pub fn clamp_kelvin(k: i64) -> u32 {
    k.clamp(MIN_KELVIN as i64, MAX_KELVIN as i64) as u32
}

/// Blackbody color (Tanner Helland's fit), unnormalized 0–255.
fn helland(k: f32) -> [f32; 3] {
    let t = k / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g =
        if t <= 66.0 { 99.470_8 * t.ln() - 161.119_57 } else { 288.122_17 * (t - 60.0).powf(-0.075_514_85) };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)]
}

/// Per-channel multipliers (0–1) for a white point, normalized so 6500K = (1, 1, 1).
pub fn white_point(kelvin: u32) -> [f32; 3] {
    let k = kelvin.clamp(MIN_KELVIN, MAX_KELVIN) as f32;
    let w = helland(k);
    let n = helland(NEUTRAL_KELVIN as f32);
    [(w[0] / n[0]).min(1.0), (w[1] / n[1]).min(1.0), (w[2] / n[2]).min(1.0)]
}

/// A GDI gamma ramp: 3 channels (R, G, B) × 256 entries of 16-bit values.
pub type Ramp = [[u16; 256]; 3];

pub fn identity_ramp() -> Ramp {
    let mut r = [[0u16; 256]; 3];
    for ch in r.iter_mut() {
        for (i, v) in ch.iter_mut().enumerate() {
            *v = (i as u16) * 257;
        }
    }
    r
}

/// Ramp for a white point `white` and an encoded brightness `scale` (0–1), blended toward
/// identity by `strength` (0 = identity, 1 = full effect).
pub fn build_ramp(white: [f32; 3], scale: f32, strength: f32) -> Ramp {
    let s = strength.clamp(0.0, 1.0);
    let mut r = [[0u16; 256]; 3];
    for (c, ch) in r.iter_mut().enumerate() {
        let full = white[c] * scale.clamp(0.0, 1.0);
        let m = 1.0 + (full - 1.0) * s;
        for (i, v) in ch.iter_mut().enumerate() {
            let x = i as f32 / 255.0 * m;
            *v = (x * 65535.0).round().clamp(0.0, 65535.0) as u16;
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_white() {
        let w = white_point(6500);
        for c in w {
            assert!((c - 1.0).abs() < 1e-3, "{w:?}");
        }
    }

    #[test]
    fn warmer_means_less_blue_and_monotonic() {
        let mut prev = white_point(6500);
        for k in (1200..6500).rev().step_by(100) {
            let w = white_point(k);
            assert!(w[2] <= prev[2] + 1e-4, "blue must not increase when warming: {k}");
            assert!(w[1] <= prev[1] + 1e-4, "green must not increase when warming: {k}");
            assert!((w[0] - 1.0).abs() < 1e-3, "red stays at max below 6600K");
            prev = w;
        }
        assert_eq!(white_point(1200)[2], 0.0);
        let ember = white_point(1200);
        assert!(ember[1] > 0.2 && ember[1] < 0.5, "{ember:?}");
    }

    #[test]
    fn identity_ends() {
        let r = identity_ramp();
        assert_eq!(r[0][0], 0);
        assert_eq!(r[2][255], 65535);
        assert_eq!(build_ramp([1.0; 3], 1.0, 1.0), r);
        assert_eq!(build_ramp([0.5, 0.2, 0.0], 0.3, 0.0), r);
    }

    #[test]
    fn ramp_scales_and_blends() {
        let r = build_ramp([1.0, 0.5, 0.0], 0.5, 1.0);
        assert_eq!(r[0][255], 32768);
        assert_eq!(r[1][255], 16384);
        assert_eq!(r[2][255], 0);
        let half = build_ramp([1.0, 1.0, 0.0], 1.0, 0.5);
        assert_eq!(half[2][255], 32768);
    }

    #[test]
    fn preset_names() {
        assert_eq!(preset_name(2650), "Incandescent");
        assert_eq!(preset_name(6400), "Daylight");
        assert_eq!(clamp_kelvin(100), MIN_KELVIN);
        assert_eq!(clamp_kelvin(99_999), MAX_KELVIN);
    }
}
