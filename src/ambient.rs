//! Ambient light sensor auto-brightness (Windows.Devices.Sensors.LightSensor).
//!
//! Brightness follows a logarithmic curve of the measured illuminance (our eyes perceive light roughly
//! logarithmically), plus a user offset learned from manual changes. Readings are smoothed and changes
//! smaller than a few percent are ignored, so the backlight does not hunt.

use windows::Devices::Sensors::LightSensor;

/// Changes smaller than this (percentage points) are not applied.
pub const HYSTERESIS: f32 = 3.0;
/// Weight of a new reading in the exponential moving average.
const SMOOTHING: f32 = 0.3;

/// Brightness (5–100%) for an illuminance: ~20% in the dark, ~60% in a lit room (100 lx),
/// ~80% at 1,000 lx, 100% in daylight.
pub fn curve(lux: f32) -> f32 {
    (20.0 + 20.0 * (lux.max(0.0) + 1.0).log10()).clamp(5.0, 100.0)
}

pub fn smooth(prev: Option<f32>, reading: f32) -> f32 {
    match prev {
        Some(p) => p + (reading - p) * SMOOTHING,
        None => reading,
    }
}

pub struct Sensor {
    sensor: LightSensor,
}

impl Sensor {
    /// The default light sensor, if this PC has one.
    pub fn open() -> Option<Sensor> {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            );
        }
        LightSensor::GetDefault().ok().map(|sensor| Sensor { sensor })
    }

    pub fn lux(&self) -> Option<f32> {
        self.sensor.GetCurrentReading().ok()?.IlluminanceInLux().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_is_monotonic_and_bounded() {
        assert_eq!(curve(0.0), 20.0);
        assert!((curve(99.0) - 60.0).abs() < 0.01);
        assert_eq!(curve(100_000.0), 100.0);
        let mut prev = 0.0;
        for lux in [0.0, 1.0, 10.0, 50.0, 200.0, 1_000.0, 5_000.0] {
            let b = curve(lux);
            assert!(b >= prev);
            prev = b;
        }
    }

    #[test]
    fn smoothing_moves_part_way() {
        assert_eq!(smooth(None, 50.0), 50.0);
        assert!((smooth(Some(0.0), 100.0) - 30.0).abs() < 1e-4);
    }
}
