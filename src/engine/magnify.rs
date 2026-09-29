//! Full-screen color matrix via the Magnification API (Darkroom and other special modes).
//!
//! Unlike gamma ramps there is no Windows range limit, and the effect disappears automatically
//! when the process that set it exits, which makes it safe for extreme modes.

use crate::info;
use windows::Win32::UI::Magnification::{
    MagInitialize, MagSetFullscreenColorEffect, MagUninitialize, MAGCOLOREFFECT,
};

/// A 5×5 color matrix (row vector [r g b a 1] × M).
pub type Matrix = [f32; 25];

pub const IDENTITY: Matrix = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 0.0, 1.0,
];

/// Darkroom (like f.lux): luminance inverted and shown in red only — no blue or green light at all.
pub const DARKROOM: Matrix = [
    -0.299, 0.0, 0.0, 0.0, 0.0, //
    -0.587, 0.0, 0.0, 0.0, 0.0, //
    -0.114, 0.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0, //
    1.0, 0.0, 0.0, 0.0, 1.0,
];

/// Applies `m` to a color (for tests and previews).
pub fn transform(m: &Matrix, rgb: [f32; 3]) -> [f32; 3] {
    let v = [rgb[0], rgb[1], rgb[2], 1.0, 1.0];
    let mut out = [0.0f32; 3];
    for (j, o) in out.iter_mut().enumerate() {
        *o = (0..5).map(|i| v[i] * m[i * 5 + j]).sum::<f32>().clamp(0.0, 1.0);
    }
    out
}

#[derive(Default)]
pub struct Magnifier {
    initialized: bool,
    current: Option<Matrix>,
}

impl Magnifier {
    /// Sets the full-screen matrix (`None` removes the effect). Returns false if Windows refused.
    pub fn set(&mut self, m: Option<Matrix>) -> bool {
        if self.current == m {
            return true;
        }
        unsafe {
            if m.is_some() && !self.initialized {
                self.initialized = MagInitialize().as_bool();
                if !self.initialized {
                    info!("magnification: MagInitialize failed");
                    return false;
                }
            }
            if !self.initialized {
                self.current = None;
                return true;
            }
            let effect = MAGCOLOREFFECT { transform: m.unwrap_or(IDENTITY) };
            let ok = MagSetFullscreenColorEffect(&effect).as_bool();
            if !ok {
                info!("magnification: MagSetFullscreenColorEffect failed");
            }
            if m.is_none() {
                let _ = MagUninitialize();
                self.initialized = false;
            }
            self.current = if ok { m } else { None };
            ok
        }
    }

    pub fn active(&self) -> bool {
        self.current.is_some()
    }
}

impl Drop for Magnifier {
    fn drop(&mut self) {
        self.set(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darkroom_is_red_only_and_inverted() {
        let white = transform(&DARKROOM, [1.0, 1.0, 1.0]);
        assert!(white[0] < 0.01 && white[1] == 0.0 && white[2] == 0.0, "white text becomes dark");
        let black = transform(&DARKROOM, [0.0, 0.0, 0.0]);
        assert_eq!(black, [1.0, 0.0, 0.0], "black background becomes red");
        let blue = transform(&DARKROOM, [0.0, 0.0, 1.0]);
        assert_eq!((blue[1], blue[2]), (0.0, 0.0));
    }

    #[test]
    fn identity_keeps_colors() {
        assert_eq!(transform(&IDENTITY, [0.2, 0.4, 0.6]), [0.2, 0.4, 0.6]);
    }
}
