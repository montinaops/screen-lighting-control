//! Sunrise / sunset from latitude and longitude (NOAA general solar position equations,
//! accurate to about a minute at non-polar latitudes). Everything is computed offline.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sun {
    /// Sunrise and sunset as minutes after UTC midnight (may be < 0 or ≥ 1440 for far longitudes).
    RiseSet { rise: f64, set: f64 },
    /// The sun never rises on this day.
    NeverRises,
    /// The sun never sets on this day.
    NeverSets,
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Day of year (1-based).
pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    const CUM: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = (month > 2 && is_leap(year)) as u32;
    CUM[(month.clamp(1, 12) - 1) as usize] + day + leap
}

/// Sunrise/sunset (UTC) for the given date and location. Uses the standard 90.833° zenith
/// (refraction + solar disc).
pub fn sun_times(year: i32, doy: u32, lat: f64, lon: f64) -> Sun {
    let days = if is_leap(year) { 366.0 } else { 365.0 };
    // Fractional year at local solar noon.
    let g = 2.0 * PI / days * (doy as f64 - 1.0);
    let eqtime = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let decl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let lat_r = lat.to_radians();
    let cos_ha = (90.833f64.to_radians().cos()) / (lat_r.cos() * decl.cos()) - lat_r.tan() * decl.tan();
    if cos_ha > 1.0 {
        return Sun::NeverRises;
    }
    if cos_ha < -1.0 {
        return Sun::NeverSets;
    }
    let ha = cos_ha.acos().to_degrees();
    Sun::RiseSet { rise: 720.0 - 4.0 * (lon + ha) - eqtime, set: 720.0 - 4.0 * (lon - ha) - eqtime }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hm(m: f64) -> (i32, i32) {
        let m = m.rem_euclid(1440.0).round() as i32;
        (m / 60, m % 60)
    }

    fn close(actual: f64, expected_utc: (i32, i32), tol_min: f64) {
        let e = (expected_utc.0 * 60 + expected_utc.1) as f64;
        let d = ((actual.rem_euclid(1440.0) - e + 720.0).rem_euclid(1440.0) - 720.0).abs();
        assert!(d <= tol_min, "got {:?}, expected {:?}", hm(actual), expected_utc);
    }

    #[test]
    fn day_of_year_handles_leap_years() {
        assert_eq!(day_of_year(2024, 3, 1), 61);
        assert_eq!(day_of_year(2023, 3, 1), 60);
        assert_eq!(day_of_year(2026, 12, 31), 365);
    }

    #[test]
    fn london_summer_solstice() {
        // NOAA: London 2026-06-21 sunrise 03:43 UTC, sunset 20:21 UTC.
        let Sun::RiseSet { rise, set } = sun_times(2026, day_of_year(2026, 6, 21), 51.5074, -0.1278) else {
            panic!()
        };
        close(rise, (3, 43), 3.0);
        close(set, (20, 21), 3.0);
    }

    #[test]
    fn sao_paulo_winter() {
        // NOAA: São Paulo 2026-07-01 sunrise 09:47 UTC (06:47 local), sunset 20:30 UTC (17:30 local).
        let Sun::RiseSet { rise, set } = sun_times(2026, day_of_year(2026, 7, 1), -23.5505, -46.6333) else {
            panic!()
        };
        close(rise, (9, 47), 3.0);
        close(set, (20, 30), 3.0);
    }

    #[test]
    fn polar_cases() {
        assert_eq!(sun_times(2026, day_of_year(2026, 6, 21), 78.22, 15.65), Sun::NeverSets);
        assert_eq!(sun_times(2026, day_of_year(2026, 12, 21), 78.22, 15.65), Sun::NeverRises);
    }
}
