//! The automatic day/evening/night curve (PRODUCT §6).
//!
//! Three overlapping weights are computed for a local time of day:
//! - `day`: 1 between day start and sunset, with smooth ramps of `sunrise_minutes` / `sunset_minutes`
//!   centred on each event;
//! - `night`: 1 from bedtime − 1 h until day start (30-minute ramp in);
//! - evening is what remains after sunset and before night.
//!
//! Colors are mixed in mired space (1e6 / K), where equal steps look equally different.

use crate::model::{Schedule, ScheduleMode};
use crate::solar::{self, Sun};

const DAY: f64 = 1440.0;
const NIGHT_RAMP: f64 = 30.0;
/// Night begins this long before bedtime (bedtime = wake − 8 h).
const NIGHT_BEFORE_BED: f64 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Day,
    Evening,
    Night,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub kelvin: u32,
    pub phase: Phase,
    /// 0 (day) … 1 (full night): scales the optional night brightness ceiling.
    pub night: f32,
}

/// Local event times for one day, in minutes after local midnight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Events {
    /// When full daylight colors begin (the earlier of sunrise and wake time).
    pub day_start: f64,
    pub sunset: f64,
    pub night_start: f64,
    /// Sun never rises / never sets.
    pub always_night: bool,
    pub always_day: bool,
}

fn smooth(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn wrap(m: f64) -> f64 {
    m.rem_euclid(DAY)
}

/// Computes the local events for a date. `utc_offset` is local − UTC in minutes.
pub fn events(s: &Schedule, year: i32, doy: u32, utc_offset: f64) -> Events {
    let wake = s.wake as f64;
    let night_start = wrap(wake - 8.0 * 60.0 - NIGHT_BEFORE_BED);
    let fixed = Events {
        day_start: s.fixed_day as f64,
        sunset: s.fixed_evening as f64,
        night_start,
        always_night: false,
        always_day: false,
    };
    if s.mode == ScheduleMode::Fixed || !s.has_location() {
        return fixed;
    }
    match solar::sun_times(year, doy, s.lat, s.lon) {
        Sun::RiseSet { rise, set } => {
            let rise = wrap(rise + utc_offset);
            let set = wrap(set + utc_offset);
            // Waking before sunrise pulls daylight colors earlier (wake is always within the morning).
            let day_start = if wake < rise && rise - wake < 6.0 * 60.0 { wake } else { rise };
            Events { day_start, sunset: set, night_start, always_night: false, always_day: false }
        }
        Sun::NeverRises => Events { always_night: true, ..fixed },
        Sun::NeverSets => Events { always_day: true, ..fixed },
    }
}

fn day_weight(t: f64, e: &Events, s: &Schedule) -> f64 {
    if e.always_day {
        return 1.0;
    }
    if e.always_night {
        return 0.0;
    }
    let (sr, ss) = ((s.sunrise_minutes as f64).max(1.0), (s.sunset_minutes as f64).max(1.0));
    let u = wrap(t - e.day_start);
    let len = wrap(e.sunset - e.day_start);
    if u <= len {
        smooth(u / sr + 0.5).min(smooth((len - u) / ss + 0.5))
    } else {
        smooth(0.5 - (u - len) / ss).max(smooth(0.5 - (DAY - u) / sr))
    }
}

fn night_weight(t: f64, e: &Events, s: &Schedule) -> f64 {
    if e.always_night {
        return 1.0;
    }
    let u = wrap(t - e.night_start);
    // Night lasts until daylight has fully arrived.
    let len = wrap(e.day_start - e.night_start) + s.sunrise_minutes as f64 / 2.0;
    if u <= len {
        1.0
    } else {
        smooth(1.0 - (DAY - u) / NIGHT_RAMP)
    }
}

fn mired(k: u32) -> f64 {
    1e6 / k as f64
}

/// The schedule's target at local minute `t` (0–1440) for the given events.
pub fn target_at(s: &Schedule, e: &Events, t: f64) -> Target {
    let d = day_weight(t, e, s);
    let n = night_weight(t, e, s) * (1.0 - d);
    let m = mired(s.evening_k) + (mired(s.day_k) - mired(s.evening_k)) * d;
    let m = m + (mired(s.night_k) - m) * n;
    let kelvin = (1e6 / m).round() as u32;
    let evening = (1.0 - d) * (1.0 - n);
    let phase = if d >= evening && d >= n {
        Phase::Day
    } else if n >= evening {
        Phase::Night
    } else {
        Phase::Evening
    };
    Target { kelvin, phase, night: n as f32 }
}

/// Current local date/time: (year, day of year, minutes after midnight, local − UTC offset in minutes).
pub fn now_local() -> (i32, u32, f64, f64) {
    let (l, u) = unsafe {
        (
            windows::Win32::System::SystemInformation::GetLocalTime(),
            windows::Win32::System::SystemInformation::GetSystemTime(),
        )
    };
    let lm = l.wHour as f64 * 60.0 + l.wMinute as f64 + l.wSecond as f64 / 60.0;
    let um = u.wHour as f64 * 60.0 + u.wMinute as f64 + u.wSecond as f64 / 60.0;
    // Offsets are at most ±14 h, so fold the difference into (−12 h, +12 h] … (−14 h, +14 h).
    let mut off = lm - um;
    if l.wDay != u.wDay {
        off += if (l.wYear, l.wMonth, l.wDay) > (u.wYear, u.wMonth, u.wDay) { DAY } else { -DAY };
    }
    let doy = solar::day_of_year(l.wYear as i32, l.wMonth as u32, l.wDay as u32);
    (l.wYear as i32, doy, lm, (off / 15.0).round() * 15.0)
}

/// The schedule's target right now.
pub fn target_now(s: &Schedule) -> Target {
    let (y, doy, t, off) = now_local();
    target_at(s, &events(s, y, doy, off), t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sched() -> Schedule {
        Schedule {
            mode: ScheduleMode::Fixed,
            fixed_day: 7 * 60,
            fixed_evening: 19 * 60,
            wake: 7 * 60,
            ..Schedule::default()
        }
    }

    fn at(s: &Schedule, h: f64) -> Target {
        let e = events(s, 2026, 180, 0.0);
        target_at(s, &e, h * 60.0)
    }

    #[test]
    fn fixed_phases() {
        let s = sched();
        // Bedtime 23:00, night from 22:00.
        assert_eq!(at(&s, 12.0).kelvin, 6500);
        assert_eq!(at(&s, 12.0).phase, Phase::Day);
        assert_eq!(at(&s, 20.5).kelvin, 3400);
        assert_eq!(at(&s, 20.5).phase, Phase::Evening);
        assert_eq!(at(&s, 23.0).kelvin, 2700);
        assert_eq!(at(&s, 3.0).phase, Phase::Night);
        assert!((at(&s, 3.0).night - 1.0).abs() < 1e-6);
        assert_eq!(at(&s, 12.0).night, 0.0);
    }

    #[test]
    fn transitions_are_smooth_and_monotonic() {
        let s = sched();
        // Evening: 18:40 → 19:20 goes from 6500 down to 3400 without jumps.
        let mut prev = at(&s, 18.5).kelvin;
        let mut step = 0u32;
        for i in 0..=60 {
            let k = at(&s, 18.5 + i as f64 / 60.0).kelvin;
            assert!(k <= prev, "warming must be monotonic");
            step = step.max(prev - k);
            prev = k;
        }
        assert!(step < 250, "no big jumps (max step {step}K per minute)");
        // Morning: continuous across day start.
        let a = at(&s, 7.0 - 1.0 / 60.0).kelvin as i64;
        let b = at(&s, 7.0 + 1.0 / 60.0).kelvin as i64;
        assert!((a - b).abs() < 200, "{a} vs {b}");
        // Whole-day continuity: no minute-to-minute jump bigger than 250K.
        let mut prev = at(&s, 0.0).kelvin as i64;
        for i in 1..1440 {
            let k = at(&s, i as f64 / 60.0).kelvin as i64;
            assert!((k - prev).abs() < 250, "jump at minute {i}: {prev} -> {k}");
            prev = k;
        }
    }

    #[test]
    fn sun_mode_uses_location_and_wake() {
        let mut s = Schedule { lat: 51.5074, lon: -0.1278, wake: 6 * 60, ..Schedule::default() };
        // London midsummer, BST (+60): sunrise ~04:43, sunset ~21:21.
        let e = events(&s, 2026, 172, 60.0);
        assert!((e.day_start - (4.0 * 60.0 + 43.0)).abs() < 4.0, "{e:?}");
        assert!((e.sunset - (21.0 * 60.0 + 21.0)).abs() < 4.0, "{e:?}");
        // Winter: waking at 06:00 before an 08:00 sunrise starts the day at 06:00.
        let e = events(&s, 2026, 355, 0.0);
        assert_eq!(e.day_start, 360.0);
        // No location → fixed times.
        s.lat = f64::NAN;
        assert_eq!(events(&s, 2026, 172, 0.0).day_start, s.fixed_day as f64);
    }

    #[test]
    fn polar_day_and_night() {
        let s = Schedule { lat: 78.22, lon: 15.65, ..Schedule::default() };
        let summer = events(&s, 2026, 172, 120.0);
        assert!(summer.always_day);
        assert_eq!(target_at(&s, &summer, 12.0 * 60.0).kelvin, 6500);
        let winter = events(&s, 2026, 355, 60.0);
        assert_eq!(target_at(&s, &winter, 12.0 * 60.0).kelvin, 2700);
    }
}
