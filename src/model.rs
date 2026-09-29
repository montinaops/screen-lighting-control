//! User settings (persisted in `slc.ini`) with defaults from PRODUCT.md.

use crate::color;
use crate::config::Ini;
use crate::engine;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleMode {
    /// Sunrise/sunset from the location.
    Sun,
    /// Fixed day/night start times.
    Fixed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorSettings {
    /// Last known name (for the settings UI when the monitor is disconnected).
    pub name: String,
    pub enabled: bool,
    pub brightness: f32,
    pub hw_share: f32,
    /// Backlight level found the first time SLC saw this monitor (restored by uninstall).
    pub original_hw: Option<f32>,
    /// The < 5% confirmation was accepted for this monitor.
    pub deep_dim_ok: bool,
}

impl Default for MonitorSettings {
    fn default() -> Self {
        MonitorSettings {
            name: String::new(),
            enabled: true,
            brightness: engine::MAX_BRIGHTNESS,
            hw_share: 50.0,
            original_hw: None,
            deep_dim_ok: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Schedule {
    pub enabled: bool,
    pub mode: ScheduleMode,
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    /// Minutes after midnight.
    pub wake: u32,
    pub day_k: u32,
    pub evening_k: u32,
    pub night_k: u32,
    pub sunset_minutes: u32,
    pub sunrise_minutes: u32,
    /// Fixed mode: day and evening start (minutes after midnight).
    pub fixed_day: u32,
    pub fixed_evening: u32,
    /// Optional night brightness (applies to every monitor as a ceiling).
    pub night_brightness: Option<f32>,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            enabled: true,
            mode: ScheduleMode::Sun,
            // No location yet: the settings window asks for a city. Until then, fixed times are used.
            lat: f64::NAN,
            lon: f64::NAN,
            city: String::new(),
            wake: 7 * 60,
            day_k: 6500,
            evening_k: 3400,
            night_k: 2700,
            sunset_minutes: 40,
            sunrise_minutes: 60,
            fixed_day: 7 * 60,
            fixed_evening: 19 * 60,
            night_brightness: None,
        }
    }
}

impl Schedule {
    pub fn has_location(&self) -> bool {
        self.lat.is_finite() && self.lon.is_finite()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneEffect {
    None,
    Darkroom,
    Movie,
    Grayscale,
    Amber,
    Red,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub name: String,
    pub brightness: Option<f32>,
    pub kelvin: Option<u32>,
    pub effect: SceneEffect,
    pub hotkey: String,
}

impl Scene {
    fn new(name: &str, brightness: Option<f32>, kelvin: Option<u32>, effect: SceneEffect) -> Scene {
        Scene { name: name.into(), brightness, kelvin, effect, hotkey: String::new() }
    }
}

pub fn default_scenes() -> Vec<Scene> {
    vec![
        Scene::new("Daylight", Some(100.0), Some(6500), SceneEffect::None),
        Scene::new("Reading", Some(80.0), Some(4200), SceneEffect::None),
        Scene::new("Evening", Some(60.0), Some(3400), SceneEffect::None),
        Scene::new("Night", Some(35.0), Some(2700), SceneEffect::None),
        Scene::new("Movie", None, Some(3400), SceneEffect::Movie),
        Scene::new("Darkroom", None, None, SceneEffect::Darkroom),
    ]
}

/// What an app rule does while that app is in the foreground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuleAction {
    /// No SLC effects (neutral colors, no software dimming), e.g. for color-critical apps.
    Disable,
    /// Keep backlight and gamma, but never show the overlay (games with anti-cheat, capture tools).
    NoOverlay,
    /// Apply a scene's brightness/warmth temporarily.
    Scene(String),
}

impl RuleAction {
    pub fn to_ini(&self) -> String {
        match self {
            RuleAction::Disable => "disable".into(),
            RuleAction::NoOverlay => "no_overlay".into(),
            RuleAction::Scene(s) => format!("scene:{s}"),
        }
    }
    pub fn parse(s: &str) -> Option<RuleAction> {
        let l = s.trim();
        match l.to_ascii_lowercase().as_str() {
            "disable" => Some(RuleAction::Disable),
            "no_overlay" => Some(RuleAction::NoOverlay),
            _ => l
                .get(..6)
                .filter(|p| p.eq_ignore_ascii_case("scene:"))
                .map(|_| RuleAction::Scene(l[6..].trim().to_string())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Lowercase executable file name, e.g. `photoshop.exe`.
    pub exe: String,
    pub action: RuleAction,
}

/// Normalizes a user-typed program name to a lowercase `name.exe`.
pub fn normalize_exe(s: &str) -> Option<String> {
    let name = s.trim().rsplit(['\\', '/']).next()?.trim().to_ascii_lowercase();
    if name.is_empty() || name.contains(['=', '[', ']']) {
        return None;
    }
    Some(if name.ends_with(".exe") { name } else { format!("{name}.exe") })
}

/// Hotkey actions and their default bindings (PRODUCT §9).
pub const HOTKEY_ACTIONS: &[(&str, &str, &str)] = &[
    ("brightness_up", "Win+Alt+Up", "Brightness up"),
    ("brightness_down", "Win+Alt+Down", "Brightness down"),
    ("warmer", "Win+Alt+Left", "Warmer"),
    ("cooler", "Win+Alt+Right", "Cooler"),
    ("pause", "Win+Alt+Home", "Pause / resume"),
    ("panic", "Win+Alt+End", "Panic restore"),
    ("darkroom", "Win+Alt+Insert", "Darkroom"),
    ("next_scene", "Win+Alt+PgUp", "Next scene"),
    ("prev_scene", "Win+Alt+PgDn", "Previous scene"),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub autostart: bool,
    pub osd: bool,
    pub theme: Theme,
    /// Manual warmth (used when the schedule is off, and as the override value).
    pub kelvin: u32,
    pub monitors: Vec<(String, MonitorSettings)>,
    pub schedule: Schedule,
    pub scenes: Vec<Scene>,
    /// (action, binding) — binding "" = disabled.
    pub hotkeys: Vec<(String, String)>,
    pub rules: Vec<Rule>,
    /// Pause effects while any app is fullscreen (games, videos, presentations).
    pub pause_fullscreen: bool,
    /// Dim to `idle_level` after `idle_minutes` without input.
    pub idle_dim: bool,
    pub idle_minutes: u32,
    pub idle_level: f32,
    /// 20-20-20 eye-break reminders.
    pub eye_breaks: bool,
    /// One-time reminder this many minutes before bedtime (wake − 8 h).
    pub bedtime_reminder: bool,
    pub bedtime_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            autostart: true,
            osd: true,
            theme: Theme::System,
            kelvin: color::NEUTRAL_KELVIN,
            monitors: Vec::new(),
            schedule: Schedule::default(),
            scenes: default_scenes(),
            hotkeys: HOTKEY_ACTIONS.iter().map(|(a, b, _)| (a.to_string(), b.to_string())).collect(),
            rules: Vec::new(),
            pause_fullscreen: false,
            idle_dim: false,
            idle_minutes: 5,
            idle_level: 30.0,
            eye_breaks: false,
            bedtime_reminder: false,
            bedtime_minutes: 60,
        }
    }
}

pub fn format_hm(minutes: u32) -> String {
    format!("{:02}:{:02}", (minutes / 60) % 24, minutes % 60)
}

pub fn parse_hm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl Settings {
    pub fn monitor(&self, key: &str) -> Option<&MonitorSettings> {
        self.monitors.iter().find(|(k, _)| k == key).map(|(_, m)| m)
    }

    pub fn monitor_mut(&mut self, key: &str) -> &mut MonitorSettings {
        if let Some(i) = self.monitors.iter().position(|(k, _)| k == key) {
            return &mut self.monitors[i].1;
        }
        self.monitors.push((key.to_string(), MonitorSettings::default()));
        &mut self.monitors.last_mut().expect("just pushed").1
    }

    pub fn hotkey(&self, action: &str) -> &str {
        self.hotkeys.iter().find(|(a, _)| a == action).map(|(_, b)| b.as_str()).unwrap_or("")
    }

    pub fn from_ini(ini: &Ini) -> Settings {
        let d = Settings::default();
        let mut s = Settings {
            autostart: ini.get_bool("general", "autostart").unwrap_or(d.autostart),
            osd: ini.get_bool("general", "osd").unwrap_or(d.osd),
            theme: match ini.get("general", "theme").unwrap_or("system").to_ascii_lowercase().as_str() {
                "light" => Theme::Light,
                "dark" => Theme::Dark,
                _ => Theme::System,
            },
            kelvin: ini.get_parse::<i64>("general", "kelvin").map(color::clamp_kelvin).unwrap_or(d.kelvin),
            ..d.clone()
        };
        for key in ini.sections_with_prefix("monitor.") {
            let sec = format!("monitor.{key}");
            let md = MonitorSettings::default();
            s.monitors.push((
                key,
                MonitorSettings {
                    name: ini.get(&sec, "name").unwrap_or("").to_string(),
                    enabled: ini.get_bool(&sec, "enabled").unwrap_or(md.enabled),
                    brightness: ini
                        .get_parse::<f32>(&sec, "brightness")
                        .unwrap_or(md.brightness)
                        .clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS),
                    hw_share: ini
                        .get_parse::<f32>(&sec, "hw_share")
                        .unwrap_or(md.hw_share)
                        .clamp(0.0, engine::MAX_HW_SHARE),
                    original_hw: ini.get_parse::<f32>(&sec, "original_hw"),
                    deep_dim_ok: ini.get_bool(&sec, "deep_dim_ok").unwrap_or(false),
                },
            ));
        }
        let sd = &d.schedule;
        let k = |key: &str, def: u32| {
            ini.get_parse::<i64>("schedule", key).map(color::clamp_kelvin).unwrap_or(def)
        };
        let mins = |key: &str, def: u32| ini.get("schedule", key).and_then(parse_hm).unwrap_or(def);
        s.schedule = Schedule {
            enabled: ini.get_bool("schedule", "enabled").unwrap_or(sd.enabled),
            mode: match ini.get("schedule", "mode") {
                Some(m) if m.eq_ignore_ascii_case("fixed") => ScheduleMode::Fixed,
                _ => ScheduleMode::Sun,
            },
            lat: ini
                .get_parse::<f64>("schedule", "lat")
                .filter(|v| (-90.0..=90.0).contains(v))
                .unwrap_or(sd.lat),
            lon: ini
                .get_parse::<f64>("schedule", "lon")
                .filter(|v| (-180.0..=180.0).contains(v))
                .unwrap_or(sd.lon),
            city: ini.get("schedule", "city").unwrap_or("").to_string(),
            wake: mins("wake", sd.wake),
            day_k: k("day_k", sd.day_k),
            evening_k: k("evening_k", sd.evening_k),
            night_k: k("night_k", sd.night_k),
            sunset_minutes: ini
                .get_parse::<u32>("schedule", "sunset_minutes")
                .unwrap_or(sd.sunset_minutes)
                .min(240),
            sunrise_minutes: ini
                .get_parse::<u32>("schedule", "sunrise_minutes")
                .unwrap_or(sd.sunrise_minutes)
                .min(240),
            fixed_day: mins("fixed_day", sd.fixed_day),
            fixed_evening: mins("fixed_evening", sd.fixed_evening),
            night_brightness: ini
                .get_parse::<f32>("schedule", "night_brightness")
                .map(|b| b.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS)),
        };
        let names = ini.sections_with_prefix("scene.");
        if !names.is_empty() {
            s.scenes = names
                .into_iter()
                .map(|name| {
                    let sec = format!("scene.{name}");
                    Scene {
                        brightness: ini
                            .get_parse::<f32>(&sec, "brightness")
                            .map(|b| b.clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS)),
                        kelvin: ini.get_parse::<i64>(&sec, "kelvin").map(color::clamp_kelvin),
                        effect: match ini.get(&sec, "effect").unwrap_or("").to_ascii_lowercase().as_str() {
                            "darkroom" => SceneEffect::Darkroom,
                            "grayscale" => SceneEffect::Grayscale,
                            "amber" => SceneEffect::Amber,
                            "red" => SceneEffect::Red,
                            "movie" => SceneEffect::Movie,
                            _ => SceneEffect::None,
                        },
                        hotkey: ini.get(&sec, "hotkey").unwrap_or("").to_string(),
                        name,
                    }
                })
                .collect();
        }
        s.pause_fullscreen = ini.get_bool("general", "pause_fullscreen").unwrap_or(false);
        s.idle_dim = ini.get_bool("general", "idle_dim").unwrap_or(false);
        s.eye_breaks = ini.get_bool("general", "eye_breaks").unwrap_or(false);
        s.bedtime_reminder = ini.get_bool("general", "bedtime_reminder").unwrap_or(false);
        s.bedtime_minutes = ini.get_parse::<u32>("general", "bedtime_minutes").unwrap_or(60).clamp(10, 180);
        s.idle_minutes = ini.get_parse::<u32>("general", "idle_minutes").unwrap_or(5).clamp(1, 60);
        s.idle_level = ini
            .get_parse::<f32>("general", "idle_level")
            .unwrap_or(30.0)
            .clamp(engine::MIN_BRIGHTNESS, engine::MAX_BRIGHTNESS);
        s.rules = ini
            .keys("rules")
            .into_iter()
            .filter_map(|(k, v)| Some(Rule { exe: normalize_exe(&k)?, action: RuleAction::parse(&v)? }))
            .collect();
        for (action, binding) in s.hotkeys.iter_mut() {
            if let Some(v) = ini.get("hotkeys", action) {
                *binding = v.to_string();
            }
        }
        s
    }

    pub fn to_ini(&self) -> Ini {
        let mut ini = Ini::default();
        ini.set("general", "autostart", self.autostart as u8);
        ini.set("general", "osd", self.osd as u8);
        ini.set(
            "general",
            "theme",
            match self.theme {
                Theme::System => "system",
                Theme::Light => "light",
                Theme::Dark => "dark",
            },
        );
        ini.set("general", "kelvin", self.kelvin);
        ini.set("general", "pause_fullscreen", self.pause_fullscreen as u8);
        ini.set("general", "idle_dim", self.idle_dim as u8);
        ini.set("general", "eye_breaks", self.eye_breaks as u8);
        ini.set("general", "bedtime_reminder", self.bedtime_reminder as u8);
        ini.set("general", "bedtime_minutes", self.bedtime_minutes);
        ini.set("general", "idle_minutes", self.idle_minutes);
        ini.set("general", "idle_level", format!("{:.0}", self.idle_level));
        for r in &self.rules {
            ini.set("rules", &r.exe, r.action.to_ini());
        }
        let sc = &self.schedule;
        ini.set("schedule", "enabled", sc.enabled as u8);
        ini.set("schedule", "mode", if sc.mode == ScheduleMode::Fixed { "fixed" } else { "sun" });
        if sc.has_location() {
            ini.set("schedule", "lat", format!("{:.4}", sc.lat));
            ini.set("schedule", "lon", format!("{:.4}", sc.lon));
        }
        ini.set("schedule", "city", &sc.city);
        ini.set("schedule", "wake", format_hm(sc.wake));
        ini.set("schedule", "day_k", sc.day_k);
        ini.set("schedule", "evening_k", sc.evening_k);
        ini.set("schedule", "night_k", sc.night_k);
        ini.set("schedule", "sunset_minutes", sc.sunset_minutes);
        ini.set("schedule", "sunrise_minutes", sc.sunrise_minutes);
        ini.set("schedule", "fixed_day", format_hm(sc.fixed_day));
        ini.set("schedule", "fixed_evening", format_hm(sc.fixed_evening));
        if let Some(b) = sc.night_brightness {
            ini.set("schedule", "night_brightness", format!("{b:.0}"));
        }
        for (action, binding) in &self.hotkeys {
            ini.set("hotkeys", action, binding);
        }
        for (key, m) in &self.monitors {
            let sec = format!("monitor.{key}");
            ini.set(&sec, "name", &m.name);
            ini.set(&sec, "enabled", m.enabled as u8);
            ini.set(&sec, "brightness", format!("{:.1}", m.brightness));
            ini.set(&sec, "hw_share", format!("{:.0}", m.hw_share));
            if let Some(o) = m.original_hw {
                ini.set(&sec, "original_hw", format!("{o:.0}"));
            }
            if m.deep_dim_ok {
                ini.set(&sec, "deep_dim_ok", 1);
            }
        }
        for sc in &self.scenes {
            let sec = format!("scene.{}", sc.name);
            if let Some(b) = sc.brightness {
                ini.set(&sec, "brightness", format!("{b:.0}"));
            }
            if let Some(k) = sc.kelvin {
                ini.set(&sec, "kelvin", k);
            }
            match sc.effect {
                SceneEffect::Darkroom => ini.set(&sec, "effect", "darkroom"),
                SceneEffect::Grayscale => ini.set(&sec, "effect", "grayscale"),
                SceneEffect::Amber => ini.set(&sec, "effect", "amber"),
                SceneEffect::Red => ini.set(&sec, "effect", "red"),
                SceneEffect::Movie => ini.set(&sec, "effect", "movie"),
                SceneEffect::None => {}
            }
            if !sc.hotkey.is_empty() {
                ini.set(&sec, "hotkey", &sc.hotkey);
            }
        }
        ini
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_roundtrip() {
        let s = Settings::default();
        let back = Settings::from_ini(&Ini::parse(&s.to_ini().to_text()));
        assert_eq!(back.scenes, s.scenes);
        assert_eq!(back.hotkeys, s.hotkeys);
        assert_eq!(back.kelvin, s.kelvin);
        assert!(!back.schedule.has_location());
        assert_eq!(back.schedule.wake, 7 * 60);
    }

    #[test]
    fn full_roundtrip() {
        let mut s = Settings { theme: Theme::Dark, ..Default::default() };
        s.schedule.lat = -23.55;
        s.schedule.lon = -46.63;
        s.schedule.city = "São Paulo".into();
        s.schedule.mode = ScheduleMode::Fixed;
        s.schedule.night_brightness = Some(40.0);
        let m = s.monitor_mut("abc");
        m.name = "DELL".into();
        m.brightness = 42.5;
        m.original_hw = Some(80.0);
        s.scenes.truncate(2);
        s.hotkeys[0].1 = "Ctrl+Alt+F1".into();
        s.pause_fullscreen = true;
        s.idle_dim = true;
        s.idle_minutes = 12;
        s.idle_level = 20.0;
        s.eye_breaks = true;
        s.bedtime_reminder = true;
        s.bedtime_minutes = 45;
        s.rules = vec![
            Rule { exe: "photoshop.exe".into(), action: RuleAction::Disable },
            Rule { exe: "game.exe".into(), action: RuleAction::NoOverlay },
            Rule { exe: "vlc.exe".into(), action: RuleAction::Scene("Movie".into()) },
        ];
        let back = Settings::from_ini(&Ini::parse(&s.to_ini().to_text()));
        assert_eq!(back, s);
    }

    #[test]
    fn invalid_values_fall_back() {
        let ini = Ini::parse(
            "[general]\nkelvin=99999\n[schedule]\nlat=123\nwake=25:00\n[monitor.x]\nbrightness=-5\nhw_share=500\n",
        );
        let s = Settings::from_ini(&ini);
        assert_eq!(s.kelvin, color::MAX_KELVIN);
        assert!(s.schedule.lat.is_nan());
        assert_eq!(s.schedule.wake, 7 * 60);
        let m = s.monitor("x").unwrap();
        assert_eq!(m.brightness, engine::MIN_BRIGHTNESS);
        assert_eq!(m.hw_share, engine::MAX_HW_SHARE);
    }

    #[test]
    fn exe_names_and_actions() {
        assert_eq!(normalize_exe(r"C:\Program Files\Adobe\Photoshop.EXE").as_deref(), Some("photoshop.exe"));
        assert_eq!(normalize_exe("vlc").as_deref(), Some("vlc.exe"));
        assert_eq!(normalize_exe("  "), None);
        assert_eq!(RuleAction::parse("Scene: Night"), Some(RuleAction::Scene("Night".into())));
        assert_eq!(RuleAction::parse("bogus"), None);
    }

    #[test]
    fn hm_parsing() {
        assert_eq!(parse_hm("07:30"), Some(450));
        assert_eq!(parse_hm("24:00"), None);
        assert_eq!(format_hm(450), "07:30");
    }
}
