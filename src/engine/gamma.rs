//! Per-monitor gamma ramps (`SetDeviceGammaRamp`) with automatic fallback when Windows rejects a ramp
//! for deviating too far from identity (the `GdiICMGammaRange` limit).

use super::{Event, Events};
use crate::color::{self, Ramp};
use crate::win;
use crate::worker::Mailbox;
use std::sync::Arc;
use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{CreateDCW, DeleteDC, HDC};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

/// Binary-search iterations when a ramp is rejected (precision ≈ 1/2^N).
const SEARCH_STEPS: u32 = 7;

/// What was actually applied to a monitor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Applied {
    /// Encoded brightness scale that the ramp carries (1 = none).
    pub scale: f32,
    /// How much of the requested warmth was applied (1 = all).
    pub warmth: f32,
    /// Windows limited the ramp (warmth or dimming was reduced).
    pub limited: bool,
    /// The ramp could not be set at all (driver/HDR/remote session).
    pub failed: bool,
}

struct Dc(HDC);

impl Dc {
    fn open(device: &str) -> Option<Dc> {
        let w = win::wide(device);
        let hdc = unsafe { CreateDCW(PCWSTR::null(), PCWSTR(w.as_ptr()), PCWSTR::null(), None) };
        if hdc.is_invalid() {
            None
        } else {
            Some(Dc(hdc))
        }
    }

    fn set(&self, ramp: &Ramp) -> bool {
        unsafe { SetDeviceGammaRamp(self.0, ramp.as_ptr() as *const _).as_bool() }
    }

    fn get(&self) -> Option<Ramp> {
        let mut r: Ramp = [[0; 256]; 3];
        unsafe { GetDeviceGammaRamp(self.0, r.as_mut_ptr() as *mut _).as_bool().then_some(r) }
    }
}

impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

/// Largest `t` in [0, 1] for which `try_set(t)` succeeds, assuming success is monotonic in `t`.
/// Returns `None` if even `t = 0` fails.
pub fn search(mut try_set: impl FnMut(f32) -> bool) -> Option<f32> {
    if try_set(1.0) {
        return Some(1.0);
    }
    if !try_set(0.0) {
        return None;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..SEARCH_STEPS {
        let mid = (lo + hi) / 2.0;
        if try_set(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // Leave the device on the best accepted value.
    if lo > 0.0 {
        try_set(lo);
    }
    Some(lo)
}

/// Windows' default limit: a ramp entry may deviate from identity by about half the full scale.
pub const DEFAULT_BOUND: f32 = 0.5;

/// Largest deviation from identity (fraction of full scale) of `build_ramp(white, scale, t)`.
fn deviation(white: [f32; 3], scale: f32, t: f32) -> f32 {
    white.iter().map(|w| (1.0 - w * scale) * t).fold(0.0, f32::max)
}

/// Predicts the strongest (scale, warmth) that fits within `bound`, keeping warmth first.
/// Returns `(scale, warmth)`.
pub fn plan(white: [f32; 3], scale: f32, bound: f32) -> (f32, f32) {
    let scale = scale.clamp(0.0, 1.0);
    if deviation(white, scale, 1.0) <= bound {
        return (scale, 1.0);
    }
    let wmin = white.iter().cloned().fold(1.0, f32::min);
    if 1.0 - wmin <= bound {
        // Full warmth fits; dim only as far as the weakest channel allows.
        let min_scale = if wmin > 0.0 { (1.0 - bound) / wmin } else { 1.0 };
        return (min_scale.clamp(scale, 1.0), 1.0);
    }
    // Warmth alone exceeds the bound: scale it back, no gamma dimming.
    (1.0, (bound / (1.0 - wmin)).clamp(0.0, 1.0))
}

/// Per-monitor learned state.
#[derive(Clone, Copy, Debug)]
pub struct State {
    /// Learned deviation bound (1.0 once a full-range ramp was accepted).
    pub bound: f32,
}

impl Default for State {
    fn default() -> Self {
        State { bound: DEFAULT_BOUND }
    }
}

/// Applies `kelvin` and an encoded brightness `scale` to one monitor. Warmth takes priority:
/// if the full ramp is rejected, dimming is reduced first (the overlay makes up for it), then warmth.
///
/// Costs one `SetDeviceGammaRamp` call when the ramp is accepted, two when the learned bound
/// predicts correctly, and a short binary search (then re-learns the bound) otherwise.
pub fn apply(device: &str, kelvin: u32, scale: f32, st: &mut State) -> Applied {
    let Some(dc) = Dc::open(device) else {
        return Applied { scale: 1.0, warmth: 0.0, limited: false, failed: true };
    };
    let white = color::white_point(kelvin);
    let scale = scale.clamp(0.0, 1.0);

    if dc.set(&color::build_ramp(white, scale, 1.0)) {
        st.bound = st.bound.max(deviation(white, scale, 1.0));
        return Applied { scale, warmth: 1.0, limited: false, failed: false };
    }
    // Predict from the learned bound (with a small safety margin).
    let (ps, pw) = plan(white, scale, st.bound - 0.002);
    if dc.set(&color::build_ramp(white, ps, pw)) {
        return Applied { scale: ps, warmth: pw, limited: true, failed: false };
    }
    // Prediction failed: search, then learn the real bound from the result.
    let dim = |t: f32| 1.0 - t * (1.0 - scale);
    let result = if scale < 1.0 && dc.set(&color::build_ramp(white, 1.0, 1.0)) {
        let t = search(|t| dc.set(&color::build_ramp(white, dim(t), 1.0))).unwrap_or(0.0);
        Applied { scale: dim(t), warmth: 1.0, limited: true, failed: false }
    } else {
        match search(|t| dc.set(&color::build_ramp(white, 1.0, t))) {
            Some(t) => Applied { scale: 1.0, warmth: t, limited: true, failed: false },
            None => Applied { scale: 1.0, warmth: 0.0, limited: true, failed: true },
        }
    };
    if !result.failed {
        st.bound = deviation(white, result.scale, result.warmth);
    }
    result
}

/// A gamma write for one monitor.
pub struct Job {
    pub gen: u64,
    pub device: String,
    pub kelvin: u32,
    pub scale: f32,
}

/// Applies ramps off the UI thread (`SetDeviceGammaRamp` can block for a vsync per call).
pub struct Worker {
    mailbox: Arc<Mailbox<Job>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn start(events: Events) -> Worker {
        let mailbox: Arc<Mailbox<Job>> = Arc::new(Mailbox::default());
        let mb = mailbox.clone();
        let thread = std::thread::Builder::new()
            .name("slc-gamma".into())
            .spawn(move || {
                let mut states: Vec<State> = Vec::new();
                let mut gen = u64::MAX;
                while let Some(jobs) = mb.take(std::time::Duration::ZERO) {
                    for (index, job) in jobs {
                        if job.gen != gen {
                            gen = job.gen;
                            states.clear();
                        }
                        if states.len() <= index {
                            states.resize(index + 1, State::default());
                        }
                        let applied = apply(&job.device, job.kelvin, job.scale, &mut states[index]);
                        events.send(Event::Gamma {
                            gen: job.gen,
                            index,
                            kelvin: job.kelvin,
                            scale: job.scale,
                            applied,
                            bound: states[index].bound,
                        });
                    }
                }
            })
            .ok();
        Worker { mailbox, thread }
    }

    pub fn submit(&self, index: usize, job: Job) {
        self.mailbox.put(index, job);
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.mailbox.quit();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Restores the neutral (identity) ramp. Returns false if the device rejected it.
pub fn reset(device: &str) -> bool {
    Dc::open(device).map(|dc| dc.set(&color::identity_ramp())).unwrap_or(false)
}

/// Reads the current ramp (used to detect other apps overwriting ours).
pub fn read(device: &str) -> Option<Ramp> {
    Dc::open(device)?.get()
}

/// Cheap fingerprint of a ramp: a few sample points per channel.
pub fn fingerprint(r: &Ramp) -> [u16; 6] {
    [r[0][128], r[0][255], r[1][128], r[1][255], r[2][128], r[2][255]]
}

#[cfg(test)]
mod tests {
    use super::search;

    #[test]
    fn search_finds_threshold() {
        let limit = 0.6;
        let t = search(|t| t <= limit).unwrap();
        assert!(t <= limit && limit - t < 0.01, "{t}");
    }

    #[test]
    fn plan_respects_bound_and_priorities() {
        use crate::color::white_point;
        // No warmth: gamma may dim down to 1 - bound.
        assert_eq!(super::plan([1.0; 3], 0.3, 0.5), (0.5, 1.0));
        assert_eq!(super::plan([1.0; 3], 0.8, 0.5), (0.8, 1.0));
        // Ember (blue = 0) can only get half its warmth, no dimming.
        let (s, w) = super::plan(white_point(1200), 0.5, 0.5);
        assert_eq!(s, 1.0);
        assert!((w - 0.5).abs() < 1e-4);
        // Mild warmth fits fully; dimming limited by the blue channel.
        let wp = white_point(4200);
        let (s, w) = super::plan(wp, 0.2, 0.5);
        assert_eq!(w, 1.0);
        assert!(s > 0.2 && (1.0 - wp[2] * s) <= 0.5 + 1e-4);
        // Expanded range: everything fits.
        assert_eq!(super::plan(white_point(1200), 0.1, 1.0), (0.1, 1.0));
    }

    #[test]
    fn search_full_and_none() {
        assert_eq!(search(|_| true), Some(1.0));
        assert_eq!(search(|_| false), None);
    }
}
