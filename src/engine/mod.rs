//! The hybrid brightness pipeline (PRODUCT.md §4): one brightness value per monitor is split
//! into a hardware level, a gamma scale and an overlay opacity.

pub mod gamma;
pub mod overlay;

use crate::info;
use crate::monitors::Monitor;

pub const MIN_BRIGHTNESS: f32 = 1.0;
pub const MAX_BRIGHTNESS: f32 = 100.0;
/// Hardware share limit (the slider always keeps some software range).
pub const MAX_HW_SHARE: f32 = 90.0;

/// How one brightness value is distributed across the stages.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Split {
    /// Hardware level 0–100 of the device's range (`None` = no hardware control).
    pub hardware: Option<f32>,
    /// Software factor in encoded space (1 = no software dimming).
    pub software: f32,
}

/// Splits brightness `b` (1–100) with hardware share `hw_share` (0–90, percent of the slider).
pub fn split(b: f32, hw_share: f32, has_hardware: bool) -> Split {
    let b = b.clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS);
    let h = if has_hardware { hw_share.clamp(0.0, MAX_HW_SHARE) } else { 0.0 };
    if h <= 0.0 {
        return Split { hardware: has_hardware.then_some(100.0), software: b / 100.0 };
    }
    let knee = 100.0 - h;
    if b >= knee {
        Split { hardware: Some((b - knee) / h * 100.0), software: 1.0 }
    } else {
        Split { hardware: Some(0.0), software: b / knee }
    }
}

/// Divides a software factor between gamma (`gamma`, what the ramp achieved) and the overlay.
/// Returns the overlay opacity (0–1) so that `gamma · (1 − α) = software`.
pub fn overlay_alpha(software: f32, gamma: f32) -> f32 {
    if gamma <= 0.0 {
        return 0.0;
    }
    (1.0 - software / gamma).clamp(0.0, 1.0)
}

/// Removes every effect from every monitor. Returns how many monitors were reset.
pub fn reset_all(monitors: &[Monitor]) -> usize {
    let mut n = 0;
    for m in monitors {
        if gamma::reset(&m.device) {
            n += 1;
        } else {
            info!("gamma reset failed on {}", m.device);
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn software_only() {
        let s = split(40.0, 50.0, false);
        assert_eq!(s.hardware, None);
        assert!(close(s.software, 0.4));
        assert!(close(split(0.0, 0.0, false).software, 0.01), "clamped to 1%");
    }

    #[test]
    fn hardware_takes_the_top_of_the_slider() {
        let s = split(100.0, 50.0, true);
        assert_eq!(s.hardware, Some(100.0));
        assert!(close(s.software, 1.0));
        let s = split(75.0, 50.0, true);
        assert!(close(s.hardware.unwrap(), 50.0));
        assert!(close(s.software, 1.0));
        let s = split(50.0, 50.0, true);
        assert!(close(s.hardware.unwrap(), 0.0));
        assert!(close(s.software, 1.0));
    }

    #[test]
    fn software_below_the_knee() {
        let s = split(25.0, 50.0, true);
        assert_eq!(s.hardware, Some(0.0));
        assert!(close(s.software, 0.5));
        let s = split(1.0, 50.0, true);
        assert!(close(s.software, 0.02));
    }

    #[test]
    fn hardware_with_zero_share_stays_at_max() {
        let s = split(30.0, 0.0, true);
        assert_eq!(s.hardware, Some(100.0));
        assert!(close(s.software, 0.3));
    }

    #[test]
    fn overlay_makes_up_the_rest() {
        assert!(close(overlay_alpha(0.3, 0.3), 0.0));
        assert!(close(overlay_alpha(0.3, 0.6), 0.5));
        assert!(close(overlay_alpha(0.01, 1.0), 0.99));
        assert!(close(overlay_alpha(1.0, 1.0), 0.0));
    }

    #[test]
    fn split_is_continuous_at_the_knee() {
        let a = split(50.001, 50.0, true);
        let b = split(49.999, 50.0, true);
        assert!(a.hardware.unwrap() < 0.01 && close(b.hardware.unwrap(), 0.0));
        assert!((a.software - b.software).abs() < 1e-3);
    }
}
