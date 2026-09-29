//! The settings window: a left navigation and six pages, custom-drawn with Direct2D.
//!
//! Each page is described by a list of [`Widget`]s (built from the current [`View`]); the same list
//! drives painting and hit-testing. Text entry uses native EDIT controls (IME, clipboard, selection)
//! placed over the drawn layout. Changes are sent to the controller as [`Action`]s.

use super::d2d::{self, Align, Rect, Surface, Weight};
use super::theme::Palette;
use super::widgets::{self, glyph};
use crate::model::{self, ScheduleMode, Settings, Theme};
use crate::{cities, color, hotkeys, schedule, win};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const CLASS: PCWSTR = w!("MONTINA.SLC.Settings");
/// Posted to the controller with a boxed `Action` in lParam.
pub const WM_APP_SETTINGS_ACTION: u32 = 0x8000 + 12;

const NAV_W: f32 = 196.0;
const PAD: f32 = 28.0;
const ROW: f32 = 44.0;
const DEFAULT_W: f32 = 860.0;
const DEFAULT_H: f32 = 640.0;
const MIN_W: f32 = 720.0;
const MIN_H: f32 = 480.0;
const WM_MOUSELEAVE: u32 = 0x02A3;
const EN_CHANGE: u32 = 0x0300;
const EN_KILLFOCUS: u32 = 0x0200;

pub enum Action {
    /// Change settings; the controller applies, re-registers hotkeys as needed, and saves.
    Edit(Box<dyn FnOnce(&mut Settings)>),
    /// Show a warmth for a few seconds (timeline scrub); `None` ends the preview.
    Preview(Option<u32>),
    /// Hotkey capture started/ended (global hotkeys are suspended meanwhile).
    Capturing(bool),
    /// Save the current brightness/warmth as a new scene.
    SaveCurrentAsScene(String),
    Identify,
    CopyDiagnostics,
    OpenSettingsFolder,
    Install,
    Uninstall,
    ExpandRange,
    Closed,
}

#[derive(Clone, Debug, Default)]
pub struct MonitorInfo {
    pub key: String,
    pub name: String,
    /// "DDC/CI backlight", "Laptop panel" or "Software only".
    pub method: String,
    pub hdr: bool,
    pub gamma_limited: bool,
}

#[derive(Clone, Debug, Default)]
pub struct View {
    pub settings: Settings,
    pub monitors: Vec<MonitorInfo>,
    pub settings_path: String,
    pub portable: bool,
    pub installed: bool,
    pub hotkey_conflicts: Vec<String>,
    pub range_expanded: bool,
    pub night_light_on: bool,
    pub current_brightness: f32,
    pub current_kelvin: u32,
    pub recent_apps: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    General,
    Displays,
    Schedule,
    Scenes,
    Rules,
    Hotkeys,
    About,
}

impl Page {
    pub fn from_name(name: &str) -> Option<Page> {
        PAGES.iter().find(|(_, _, l)| l.eq_ignore_ascii_case(name)).map(|p| p.0)
    }
}

const PAGES: [(Page, &str, &str); 7] = [
    (Page::General, glyph::SETTINGS, "General"),
    (Page::Displays, glyph::MONITOR, "Displays"),
    (Page::Schedule, glyph::CLOCK, "Schedule"),
    (Page::Scenes, glyph::PALETTE, "Scenes"),
    (Page::Rules, glyph::APPS, "Rules"),
    (Page::Hotkeys, glyph::KEYBOARD, "Hotkeys"),
    (Page::About, glyph::INFO, "About"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Id {
    Nav(usize),
    AutoStart,
    IdleDim,
    IdleMinutes,
    IdleLevel,
    Osd,
    Theme(u8),
    MonEnabled(usize),
    MonShare(usize),
    Identify,
    ExpandRange,
    SchedEnabled,
    SchedMode(u8),
    CityEdit,
    CityResult(usize),
    Wake,
    DayK,
    EveningK,
    NightK,
    SunsetMin,
    SunriseMin,
    NightBrightOn,
    NightBright,
    FixedDay,
    FixedEvening,
    Timeline,
    Scene(usize),
    SceneName,
    SceneBright,
    SceneKelvin,
    SceneHotkey,
    SceneEffect,
    SceneAdd,
    SceneDelete,
    Hotkey(usize),
    PauseFullscreen,
    RuleAction(usize),
    RuleScene(usize),
    RuleDelete(usize),
    RecentApp(usize),
    RuleEdit,
    HotkeyClear(usize),
    HotkeysReset,
    Install,
    Uninstall,
    CopyDiag,
    OpenFolder,
}

enum Kind {
    Title(String),
    Heading(String),
    /// Label (left) with optional description below.
    Label(String, String),
    Text(String, d2d::Color),
    Toggle(bool),
    Slider {
        frac: f32,
        value: String,
        warmth: bool,
    },
    Segmented(Vec<&'static str>, usize),
    Button(String, bool),
    Edit,
    Chip(String, bool),
    Timeline,
    Card,
    KeyBox(String, bool, bool),
}

struct Widget {
    id: Option<Id>,
    rect: Rect,
    kind: Kind,
}

/// Page-local UI state that is not part of the settings.
#[derive(Default)]
struct Local {
    city_results: Vec<cities::City>,
    selected_scene: Option<usize>,
    capture: Option<Id>,
}

pub struct SettingsWindow {
    hwnd: HWND,
    controller: HWND,
    surface: Surface,
    view: View,
    palette: Palette,
    page: Page,
    scroll: f32,
    content_h: f32,
    widgets: Vec<Widget>,
    hot: Option<Id>,
    drag: Option<Id>,
    local: Local,
    edits: Vec<(Id, HWND)>,
    font: HFONT,
    edit_brush: HBRUSH,
}

fn fmt_k(k: u32) -> String {
    format!("{k}K · {}", color::preset_name(k))
}

fn kelvin_frac(k: u32) -> f32 {
    (k.saturating_sub(color::MIN_KELVIN)) as f32 / (color::MAX_KELVIN - color::MIN_KELVIN) as f32
}

fn frac_kelvin(f: f32) -> u32 {
    super::flyout::kelvin_from_frac(f)
}

impl SettingsWindow {
    pub fn create(controller: HWND, view: View, palette: Palette, page: Page) -> Option<Box<SettingsWindow>> {
        unsafe {
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(proc),
                hInstance: win::hinstance(),
                lpszClassName: CLASS,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hIcon: crate::icon::create(32, crate::icon::Glyph::Normal).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let mut sw = Box::new(SettingsWindow {
                hwnd: HWND::default(),
                controller,
                surface: Surface::new(HWND::default()),
                view,
                palette,
                page,
                scroll: 0.0,
                content_h: 0.0,
                widgets: Vec::new(),
                hot: None,
                drag: None,
                local: Local::default(),
                edits: Vec::new(),
                font: HFONT::default(),
                edit_brush: HBRUSH::default(),
            });
            let scale = windows::Win32::UI::HiDpi::GetDpiForSystem().max(96) as f32 / 96.0;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                CLASS,
                w!("Screen Lighting Control — Settings"),
                WS_OVERLAPPEDWINDOW & !WS_MAXIMIZEBOX,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                (DEFAULT_W * scale) as i32,
                (DEFAULT_H * scale) as i32,
                None,
                None,
                Some(win::hinstance()),
                Some(&mut *sw as *mut SettingsWindow as *const core::ffi::c_void),
            )
            .map_err(|e| crate::info!("settings window: CreateWindowEx failed: {e}"))
            .ok()?;
            sw.hwnd = hwnd;
            sw.surface = Surface::new(hwnd);
            sw.apply_theme();
            sw.rebuild();
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            Some(sw)
        }
    }

    pub fn show_page(&mut self, page: Page) {
        self.set_page(page);
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }

    /// New data from the controller (after any change).
    pub fn update(&mut self, view: View, palette: Palette) {
        let theme_changed = palette.dark != self.palette.dark;
        self.view = view;
        self.palette = palette;
        if theme_changed {
            self.apply_theme();
        }
        self.rebuild();
    }

    fn apply_theme(&mut self) {
        unsafe {
            // DWMWA_USE_IMMERSIVE_DARK_MODE (20): dark title bar on Windows 10 20H1+ and 11.
            let dark: i32 = self.palette.dark as i32;
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWINDOWATTRIBUTE(20),
                &dark as *const _ as *const _,
                std::mem::size_of::<i32>() as u32,
            );
            if !self.edit_brush.is_invalid() {
                let _ = DeleteObject(self.edit_brush.into());
            }
            let c = self.palette.surface;
            self.edit_brush = CreateSolidBrush(colorref(c));
        }
    }

    fn scale(&self) -> f32 {
        self.surface.scale()
    }

    fn client_dip(&self) -> (f32, f32) {
        let mut rc = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rc);
        }
        let s = self.scale();
        ((rc.right - rc.left) as f32 / s, (rc.bottom - rc.top) as f32 / s)
    }

    fn send(&self, a: Action) {
        let ptr = Box::into_raw(Box::new(a));
        unsafe {
            if PostMessageW(Some(self.controller), WM_APP_SETTINGS_ACTION, WPARAM(0), LPARAM(ptr as isize))
                .is_err()
            {
                drop(Box::from_raw(ptr));
            }
        }
    }

    fn edit(&self, f: impl FnOnce(&mut Settings) + 'static) {
        self.send(Action::Edit(Box::new(f)));
    }

    fn set_page(&mut self, page: Page) {
        if self.page != page {
            self.end_capture();
            self.page = page;
            self.scroll = 0.0;
            self.destroy_edits();
            self.local.city_results.clear();
        }
        self.rebuild();
    }

    // ----- layout -------------------------------------------------------------------------------

    fn rebuild(&mut self) {
        let (w, h) = self.client_dip();
        let mut out = Vec::new();
        // Navigation.
        for (i, (_, _, _)) in PAGES.iter().enumerate() {
            out.push(Widget {
                id: Some(Id::Nav(i)),
                rect: Rect::new(10.0, 64.0 + i as f32 * 40.0, NAV_W - 20.0, 36.0),
                kind: Kind::Card,
            });
        }
        let x0 = NAV_W + PAD;
        let cw = (w - x0 - PAD).max(300.0);
        let mut b = Builder { out: Vec::new(), x: x0, y: 24.0 - self.scroll, w: cw };
        match self.page {
            Page::General => self.page_general(&mut b),
            Page::Displays => self.page_displays(&mut b),
            Page::Schedule => self.page_schedule(&mut b),
            Page::Scenes => self.page_scenes(&mut b),
            Page::Rules => self.page_rules(&mut b),
            Page::Hotkeys => self.page_hotkeys(&mut b),
            Page::About => self.page_about(&mut b),
        }
        self.content_h = b.y + self.scroll + 24.0;
        let max_scroll = (self.content_h - h).max(0.0);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
            return self.rebuild();
        }
        out.extend(b.out);
        self.widgets = out;
        self.sync_edits();
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn page_general(&self, b: &mut Builder) {
        let s = &self.view.settings;
        b.title("General");
        b.toggle_row(
            Id::AutoStart,
            "Start with Windows",
            "Launch SLC in the tray when you sign in.",
            s.autostart,
        );
        b.toggle_row(
            Id::Osd,
            "On-screen display",
            "Show a small popup when a hotkey changes something.",
            s.osd,
        );
        b.toggle_row(
            Id::IdleDim,
            "Dim when idle",
            "Fade the screens down after a while without keyboard or mouse input. Not while a video or fullscreen app is playing.",
            s.idle_dim,
        );
        if s.idle_dim {
            b.slider_row(
                Id::IdleMinutes,
                "After",
                "",
                (s.idle_minutes as f32 - 1.0) / 29.0,
                format!("{} min", s.idle_minutes),
                false,
            );
            b.slider_row(
                Id::IdleLevel,
                "Dim to",
                "",
                (s.idle_level - 1.0) / 79.0,
                format!("{:.0}%", s.idle_level),
                false,
            );
        }
        let sel = match s.theme {
            Theme::System => 0,
            Theme::Light => 1,
            Theme::Dark => 2,
        };
        b.row(
            "Theme",
            "Colors of the flyout, OSD and this window.",
            vec![(Some(Id::Theme(0)), 260.0, 32.0, Kind::Segmented(vec!["System", "Light", "Dark"], sel))],
        );
        b.heading("Installation");
        let (label, desc) = if self.view.installed {
            ("Installed for this user", "Start menu shortcut, autostart and an entry in Apps & features.")
        } else if self.view.portable {
            ("Portable mode", "Settings are stored next to slc.exe. Install to add a Start menu shortcut and an uninstaller.")
        } else {
            ("Not installed", "Settings are stored in your profile.")
        };
        let button = if self.view.installed {
            (Some(Id::Uninstall), 140.0, 32.0, Kind::Button("Uninstall…".into(), false))
        } else {
            (Some(Id::Install), 140.0, 32.0, Kind::Button("Install".into(), true))
        };
        b.row(label, desc, vec![button]);
        b.text(&format!("Settings file: {}", self.view.settings_path), self.palette.subtext);
    }

    fn page_displays(&self, b: &mut Builder) {
        b.title("Displays");
        if self.view.night_light_on {
            b.card_text(
                "Windows Night Light is on. It fights with SLC over the screen colors (flicker). Turn it off in Settings › System › Display.",
                self.palette.danger,
            );
        }
        let any_limited = self.view.monitors.iter().any(|m| m.gamma_limited);
        for (i, m) in self.view.monitors.iter().enumerate() {
            let ms = self.view.settings.monitor(&m.key).cloned().unwrap_or_default();
            b.heading(&format!("{}  ·  #{}", m.name, i + 1));
            let mut desc = format!("Brightness control: {}.", m.method);
            if m.hdr {
                desc.push_str(" HDR is on, so color changes are limited.");
            }
            if m.gamma_limited {
                desc.push_str(" Windows limits how warm this display can get.");
            }
            b.toggle_row(Id::MonEnabled(i), "Apply SLC to this display", &desc, ms.enabled);
            if m.method != "Software only" {
                b.slider_row(
                    Id::MonShare(i),
                    "Backlight share",
                    "Part of the brightness slider that moves the real backlight (saves power, keeps contrast). Below it, SLC dims in software.",
                    ms.hw_share / crate::engine::MAX_HW_SHARE,
                    format!("{:.0}%", ms.hw_share),
                    false,
                );
            }
        }
        b.heading("Tools");
        b.row(
            "Identify displays",
            "Shows each display's number on it for a few seconds.",
            vec![(Some(Id::Identify), 140.0, 32.0, Kind::Button("Identify".into(), false))],
        );
        let desc = if self.view.range_expanded {
            "Windows allows the full color range (warmest colors and deep gamma dimming)."
        } else if any_limited {
            "Windows is limiting SLC's warmth. Allow the full range (needs administrator rights, then sign out and back in)."
        } else {
            "Allows warmer colors below 2700K and deeper dimming without the overlay (needs administrator rights)."
        };
        let controls = if self.view.range_expanded {
            vec![]
        } else {
            vec![(Some(Id::ExpandRange), 140.0, 32.0, Kind::Button("Expand…".into(), any_limited))]
        };
        b.row("Expand color range", desc, controls);
    }

    fn page_schedule(&self, b: &mut Builder) {
        let sc = &self.view.settings.schedule;
        b.title("Schedule");
        b.toggle_row(
            Id::SchedEnabled,
            "Automatic warmth",
            "Follow the sun: daylight colors by day, warm light at night.",
            sc.enabled,
        );
        b.row(
            "Timing",
            "Use sunrise and sunset for your city, or fixed times.",
            vec![(
                Some(Id::SchedMode(0)),
                220.0,
                32.0,
                Kind::Segmented(vec!["Sun", "Fixed times"], (sc.mode == ScheduleMode::Fixed) as usize),
            )],
        );
        if sc.mode == ScheduleMode::Sun {
            let loc = if sc.has_location() {
                format!(
                    "{} ({:.2}, {:.2}). Type to search for another city.",
                    if sc.city.is_empty() { "Custom" } else { &sc.city },
                    sc.lat,
                    sc.lon
                )
            } else {
                "No location yet: fixed times are used until you pick a city. Type a city name to search."
                    .to_string()
            };
            b.row("Location", &loc, vec![(Some(Id::CityEdit), 260.0, 32.0, Kind::Edit)]);
            if !self.local.city_results.is_empty() {
                b.chips(
                    self.local
                        .city_results
                        .iter()
                        .enumerate()
                        .map(|(i, c)| (Id::CityResult(i), c.label(), false))
                        .collect(),
                );
            }
        } else {
            b.row("Day starts", "HH:MM", vec![(Some(Id::FixedDay), 100.0, 32.0, Kind::Edit)]);
            b.row("Evening starts", "HH:MM", vec![(Some(Id::FixedEvening), 100.0, 32.0, Kind::Edit)]);
        }
        b.row(
            "Wake time",
            "Night colors start an hour before bedtime (8 hours before waking). Waking before sunrise brings daylight colors earlier.",
            vec![(Some(Id::Wake), 100.0, 32.0, Kind::Edit)],
        );
        b.heading("Colors");
        b.slider_row(Id::DayK, "Day", "", kelvin_frac(sc.day_k), fmt_k(sc.day_k), true);
        b.slider_row(Id::EveningK, "Evening", "", kelvin_frac(sc.evening_k), fmt_k(sc.evening_k), true);
        b.slider_row(Id::NightK, "Night", "", kelvin_frac(sc.night_k), fmt_k(sc.night_k), true);
        b.slider_row(
            Id::SunsetMin,
            "Sunset transition",
            "",
            sc.sunset_minutes as f32 / 180.0,
            format!("{} min", sc.sunset_minutes),
            false,
        );
        b.slider_row(
            Id::SunriseMin,
            "Sunrise transition",
            "",
            sc.sunrise_minutes as f32 / 180.0,
            format!("{} min", sc.sunrise_minutes),
            false,
        );
        b.toggle_row(
            Id::NightBrightOn,
            "Dim at night",
            "Lower every display's brightness at night (a ceiling that follows the schedule).",
            sc.night_brightness.is_some(),
        );
        if let Some(nb) = sc.night_brightness {
            b.slider_row(
                Id::NightBright,
                "Night brightness",
                "",
                (nb - 1.0) / 99.0,
                format!("{nb:.0}%"),
                false,
            );
        }
        b.heading("Today");
        b.push_line(Some(Id::Timeline), 120.0, Kind::Timeline);
        b.text("Drag across the timeline to preview a time of day on your screens.", self.palette.subtext);
    }

    fn page_scenes(&self, b: &mut Builder) {
        let s = &self.view.settings;
        b.title("Scenes");
        b.text("One click in the flyout (or a hotkey) applies a scene's brightness and warmth. Select a scene to edit it.", self.palette.subtext);
        b.y += 4.0;
        b.chips(
            s.scenes
                .iter()
                .enumerate()
                .map(|(i, sc)| (Id::Scene(i), sc.name.clone(), self.local.selected_scene == Some(i)))
                .collect(),
        );
        b.push_line(
            Some(Id::SceneAdd),
            34.0,
            Kind::Button(
                format!(
                    "Save current ({:.0}% · {}K) as a new scene",
                    self.view.current_brightness, self.view.current_kelvin
                ),
                false,
            ),
        );
        if let Some(sc) = self.local.selected_scene.and_then(|i| s.scenes.get(i)) {
            b.heading(&format!("Edit “{}”", sc.name));
            b.row("Name", "", vec![(Some(Id::SceneName), 220.0, 32.0, Kind::Edit)]);
            let bright = sc.brightness.unwrap_or(100.0);
            b.slider_row(
                Id::SceneBright,
                "Brightness",
                if sc.brightness.is_none() { "Not changed by this scene. Drag to set one." } else { "" },
                (bright - 1.0) / 99.0,
                sc.brightness.map(|v| format!("{v:.0}%")).unwrap_or("—".into()),
                false,
            );
            let k = sc.kelvin.unwrap_or(6500);
            b.slider_row(
                Id::SceneKelvin,
                "Warmth",
                if sc.kelvin.is_none() { "Not changed by this scene. Drag to set one." } else { "" },
                kelvin_frac(k),
                sc.kelvin.map(fmt_k).unwrap_or("—".into()),
                true,
            );
            let effect = match sc.effect {
                model::SceneEffect::None => "None",
                model::SceneEffect::Movie => "Movie mode",
                model::SceneEffect::Darkroom => "Darkroom",
                model::SceneEffect::Grayscale => "Grayscale",
                model::SceneEffect::Amber => "Amber night",
                model::SceneEffect::Red => "Red night",
            };
            b.row(
                "Effect",
                "Click to change: Movie mode, or a color filter (Darkroom, Grayscale, Amber night, Red night).",
                vec![(Some(Id::SceneEffect), 160.0, 32.0, Kind::Button(effect.into(), false))],
            );
            let capturing = self.local.capture == Some(Id::SceneHotkey);
            b.row(
                "Hotkey",
                "Click, then press the key combination (Esc cancels, Backspace clears).",
                vec![(
                    Some(Id::SceneHotkey),
                    220.0,
                    32.0,
                    Kind::KeyBox(
                        if sc.hotkey.is_empty() { "None".into() } else { sc.hotkey.clone() },
                        capturing,
                        false,
                    ),
                )],
            );
            b.push_line(Some(Id::SceneDelete), 34.0, Kind::Button("Delete scene".into(), false));
        }
    }

    fn page_rules(&self, b: &mut Builder) {
        let s = &self.view.settings;
        b.title("Rules");
        b.toggle_row(
            Id::PauseFullscreen,
            "Pause in fullscreen apps",
            "Turn effects off while a game, video or presentation fills the screen.",
            s.pause_fullscreen,
        );
        b.heading("App rules");
        b.text("While one of these apps is in front, SLC changes its behavior.", self.palette.subtext);
        for (i, r) in s.rules.iter().enumerate() {
            let (sel, desc) = match &r.action {
                model::RuleAction::Disable => (0, "No SLC effects — for color-critical work (photo and video editing)."),
                model::RuleAction::NoOverlay => {
                    (1, "Backlight and gamma only, never the overlay — for games with anti-cheat and capture tools.")
                }
                model::RuleAction::Scene(_) => (2, "Applies a scene while the app is in front (click the scene to change it)."),
            };
            let mut controls =
                vec![(Some(Id::RuleDelete(i)), 32.0, 32.0, Kind::Button(glyph::CLOSE.into(), false))];
            if let model::RuleAction::Scene(name) = &r.action {
                controls.push((Some(Id::RuleScene(i)), 120.0, 32.0, Kind::Button(name.clone(), false)));
            }
            controls.push((
                Some(Id::RuleAction(i)),
                270.0,
                32.0,
                Kind::Segmented(vec!["Off", "No overlay", "Scene"], sel),
            ));
            b.row(&r.exe, desc, controls);
        }
        if s.rules.is_empty() {
            b.text("No rules yet.", self.palette.subtext);
        }
        b.heading("Add a rule");
        b.row(
            "Program",
            "Type a program name (e.g. photoshop.exe) and press Tab, or pick a recent app below.",
            vec![(Some(Id::RuleEdit), 220.0, 32.0, Kind::Edit)],
        );
        let recent: Vec<(Id, String, bool)> = self
            .view
            .recent_apps
            .iter()
            .enumerate()
            .filter(|(_, a)| !s.rules.iter().any(|r| &r.exe == *a))
            .map(|(i, a)| (Id::RecentApp(i), format!("+ {a}"), false))
            .collect();
        if !recent.is_empty() {
            b.chips(recent);
        }
    }

    fn page_hotkeys(&self, b: &mut Builder) {
        let s = &self.view.settings;
        b.title("Hotkeys");
        b.text(
            "Click a shortcut, then press the new key combination. Esc cancels, Backspace removes it.",
            self.palette.subtext,
        );
        b.y += 4.0;
        for (i, (action, _, label)) in model::HOTKEY_ACTIONS.iter().enumerate() {
            let binding = s.hotkey(action).to_string();
            let conflict = !binding.is_empty() && self.view.hotkey_conflicts.contains(&binding);
            let desc = if conflict { "In use by another program — choose another shortcut" } else { "" };
            let capturing = self.local.capture == Some(Id::Hotkey(i));
            b.row(
                label,
                desc,
                vec![
                    (Some(Id::HotkeyClear(i)), 32.0, 32.0, Kind::Button(glyph::CLOSE.into(), false)),
                    (
                        Some(Id::Hotkey(i)),
                        200.0,
                        32.0,
                        Kind::KeyBox(
                            if binding.is_empty() { "None".into() } else { binding },
                            capturing,
                            conflict,
                        ),
                    ),
                ],
            );
        }
        b.y += 4.0;
        b.push_line(Some(Id::HotkeysReset), 34.0, Kind::Button("Restore default hotkeys".into(), false));
    }

    fn page_about(&self, b: &mut Builder) {
        b.title("About");
        b.text(&format!("Screen Lighting Control {}", crate::VERSION), self.palette.text);
        b.text("Tiny screen dimmer and color temperature controller for Windows.", self.palette.subtext);
        b.text(
            "Hybrid pipeline: backlight (DDC/CI) → gamma → overlay. No network, no telemetry.",
            self.palette.subtext,
        );
        b.y += 12.0;
        b.push_line(Some(Id::CopyDiag), 34.0, Kind::Button("Copy diagnostics".into(), false));
        b.push_line(Some(Id::OpenFolder), 34.0, Kind::Button("Open settings folder".into(), false));
        b.y += 12.0;
        b.text("City data © GeoNames (CC BY 4.0). Solar equations: NOAA.", self.palette.subtext);
        b.text("© MONTINA-Ops. All rights reserved.", self.palette.subtext);
    }

    // ----- native edit controls ------------------------------------------------------------------

    fn edit_text(&self, id: Id) -> String {
        let s = &self.view.settings;
        match id {
            Id::Wake => model::format_hm(s.schedule.wake),
            Id::FixedDay => model::format_hm(s.schedule.fixed_day),
            Id::FixedEvening => model::format_hm(s.schedule.fixed_evening),
            Id::SceneName => self
                .local
                .selected_scene
                .and_then(|i| s.scenes.get(i))
                .map(|sc| sc.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn sync_edits(&mut self) {
        let wanted: Vec<(Id, Rect)> = self
            .widgets
            .iter()
            .filter(|w| matches!(w.kind, Kind::Edit))
            .filter_map(|w| w.id.map(|id| (id, w.rect)))
            .collect();
        // Remove edits that are no longer on the page.
        self.edits.retain(|(id, h)| {
            let keep = wanted.iter().any(|(w, _)| w == id);
            if !keep {
                unsafe {
                    let _ = DestroyWindow(*h);
                }
            }
            keep
        });
        let s = self.scale();
        unsafe {
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
            self.font = CreateFontW(
                -(13.0 * s) as i32,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                w!("Segoe UI"),
            );
        }
        let (_, h) = self.client_dip();
        for (id, r) in wanted {
            let inner = r.inset(10.0, 7.0);
            let visible = r.y >= 0.0 && r.bottom() <= h;
            let is_new = !self.edits.iter().any(|(e, _)| *e == id);
            let hwnd = match self.edits.iter().find(|(e, _)| *e == id) {
                Some((_, h)) => *h,
                None => unsafe {
                    let Ok(h) = CreateWindowExW(
                        WINDOW_EX_STYLE(0),
                        w!("EDIT"),
                        PCWSTR(win::wide(&self.edit_text(id)).as_ptr()),
                        WS_CHILD | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                        0,
                        0,
                        10,
                        10,
                        Some(self.hwnd),
                        None,
                        Some(win::hinstance()),
                        None,
                    ) else {
                        continue;
                    };
                    self.edits.push((id, h));
                    h
                },
            };
            unsafe {
                if is_new && id == Id::CityEdit {
                    // EM_SETCUEBANNER (Common Controls v6, enabled by the manifest).
                    let cue = win::wide("Search for a city…");
                    SendMessageW(hwnd, 0x1501, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
                }
                SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    (inner.x * s) as i32,
                    (inner.y * s) as i32,
                    (inner.w * s) as i32,
                    (inner.h * s) as i32,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                let _ = ShowWindow(hwnd, if visible { SW_SHOWNA } else { SW_HIDE });
            }
        }
    }

    fn destroy_edits(&mut self) {
        for (_, h) in self.edits.drain(..) {
            unsafe {
                let _ = DestroyWindow(h);
            }
        }
    }

    fn edit_value(&self, hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..n])
    }

    fn set_edit_text(&self, id: Id, text: &str) {
        if let Some((_, h)) = self.edits.iter().find(|(e, _)| *e == id) {
            unsafe {
                let _ = SetWindowTextW(*h, PCWSTR(win::wide(text).as_ptr()));
            }
        }
    }

    fn on_edit_command(&mut self, code: u32, hwnd: HWND) {
        let Some(id) = self.edits.iter().find(|(_, h)| *h == hwnd).map(|(id, _)| *id) else { return };
        let text = self.edit_value(hwnd);
        match (id, code) {
            (Id::CityEdit, EN_CHANGE) => {
                self.local.city_results = cities::search(&text, 6);
                self.rebuild();
            }
            (Id::Wake | Id::FixedDay | Id::FixedEvening, EN_KILLFOCUS) => {
                if let Some(m) = model::parse_hm(&text) {
                    self.edit(move |s| match id {
                        Id::Wake => s.schedule.wake = m,
                        Id::FixedDay => s.schedule.fixed_day = m,
                        _ => s.schedule.fixed_evening = m,
                    });
                } else {
                    self.set_edit_text(id, &self.edit_text(id));
                }
            }
            (Id::RuleEdit, EN_KILLFOCUS) => {
                if model::normalize_exe(&text).is_some() {
                    self.edit(move |s| add_rule(s, &text));
                    self.set_edit_text(Id::RuleEdit, "");
                }
            }
            (Id::SceneName, EN_KILLFOCUS) => {
                let name = text.trim().replace(['[', ']', '='], "");
                if let (Some(i), false) = (self.local.selected_scene, name.is_empty()) {
                    let taken = self
                        .view
                        .settings
                        .scenes
                        .iter()
                        .enumerate()
                        .any(|(j, s)| j != i && s.name.eq_ignore_ascii_case(&name));
                    if !taken {
                        self.edit(move |s| {
                            if let Some(sc) = s.scenes.get_mut(i) {
                                sc.name = name;
                            }
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // ----- input ---------------------------------------------------------------------------------

    fn hit(&self, x: f32, y: f32) -> Option<Id> {
        self.widgets.iter().rev().find(|w| w.id.is_some() && w.rect.contains(x, y)).and_then(|w| w.id)
    }

    fn widget(&self, id: Id) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id == Some(id))
    }

    fn slider_frac(&self, id: Id, x: f32) -> f32 {
        self.widget(id).map(|w| widgets::slider_frac(slider_track(w.rect), x)).unwrap_or(0.0)
    }

    fn on_drag(&mut self, id: Id, x: f32) {
        let f = self.slider_frac(id, x);
        match id {
            Id::MonShare(i) => {
                let key = self.view.monitors[i].key.clone();
                let v = (f * crate::engine::MAX_HW_SHARE / 5.0).round() * 5.0;
                self.edit(move |s| s.monitor_mut(&key).hw_share = v);
            }
            Id::IdleMinutes => {
                let m = (1.0 + f * 29.0).round() as u32;
                self.edit(move |s| s.idle_minutes = m);
            }
            Id::IdleLevel => {
                let v = (1.0 + f * 79.0).round();
                self.edit(move |s| s.idle_level = v);
            }
            Id::DayK | Id::EveningK | Id::NightK => {
                let k = frac_kelvin(f);
                self.edit(move |s| match id {
                    Id::DayK => s.schedule.day_k = k,
                    Id::EveningK => s.schedule.evening_k = k,
                    _ => s.schedule.night_k = k,
                });
                self.send(Action::Preview(Some(k)));
            }
            Id::SunsetMin | Id::SunriseMin => {
                let m = ((f * 180.0) / 5.0).round() as u32 * 5;
                self.edit(move |s| {
                    if id == Id::SunsetMin {
                        s.schedule.sunset_minutes = m
                    } else {
                        s.schedule.sunrise_minutes = m
                    }
                });
            }
            Id::NightBright => {
                let v = (1.0 + f * 99.0).round();
                self.edit(move |s| s.schedule.night_brightness = Some(v));
            }
            Id::SceneBright | Id::SceneKelvin => {
                let Some(i) = self.local.selected_scene else { return };
                if id == Id::SceneBright {
                    let v = (1.0 + f * 99.0).round();
                    self.edit(move |s| s.scenes[i].brightness = Some(v));
                } else {
                    let k = frac_kelvin(f);
                    self.edit(move |s| s.scenes[i].kelvin = Some(k));
                    self.send(Action::Preview(Some(k)));
                }
            }
            Id::Timeline => {
                let Some(w) = self.widget(Id::Timeline) else { return };
                let minute = widgets::slider_frac(w.rect, x) * 1440.0;
                let sc = &self.view.settings.schedule;
                let (y, doy, _, off) = schedule::now_local();
                let t = schedule::target_at(sc, &schedule::events(sc, y, doy, off), minute as f64);
                self.send(Action::Preview(Some(t.kelvin)));
            }
            _ => {}
        }
    }

    fn on_click(&mut self, id: Id, x: f32) {
        match id {
            Id::Nav(i) => self.set_page(PAGES[i].0),
            Id::AutoStart => self.edit(|s| s.autostart = !s.autostart),
            Id::IdleDim => self.edit(|s| s.idle_dim = !s.idle_dim),
            Id::Osd => self.edit(|s| s.osd = !s.osd),
            Id::Theme(_) => {
                let seg = self.segment_at(id, x, 3);
                self.edit(move |s| s.theme = [Theme::System, Theme::Light, Theme::Dark][seg]);
            }
            Id::MonEnabled(i) => {
                let key = self.view.monitors[i].key.clone();
                self.edit(move |s| {
                    let m = s.monitor_mut(&key);
                    m.enabled = !m.enabled;
                });
            }
            Id::Identify => self.send(Action::Identify),
            Id::ExpandRange => self.send(Action::ExpandRange),
            Id::SchedEnabled => self.edit(|s| s.schedule.enabled = !s.schedule.enabled),
            Id::SchedMode(_) => {
                let seg = self.segment_at(id, x, 2);
                self.edit(move |s| {
                    s.schedule.mode = if seg == 1 { ScheduleMode::Fixed } else { ScheduleMode::Sun }
                });
                self.destroy_edits();
            }
            Id::CityResult(i) => {
                if let Some(c) = self.local.city_results.get(i).copied() {
                    self.edit(move |s| {
                        s.schedule.city = c.label();
                        s.schedule.lat = c.lat;
                        s.schedule.lon = c.lon;
                        s.schedule.mode = ScheduleMode::Sun;
                    });
                    self.local.city_results.clear();
                    self.set_edit_text(Id::CityEdit, "");
                }
            }
            Id::NightBrightOn => self.edit(|s| {
                s.schedule.night_brightness =
                    if s.schedule.night_brightness.is_some() { None } else { Some(60.0) }
            }),
            Id::Scene(i) => {
                self.local.selected_scene = if self.local.selected_scene == Some(i) { None } else { Some(i) };
                self.destroy_edits();
                self.rebuild();
            }
            Id::SceneAdd => {
                let n = (1..)
                    .map(|i| format!("Scene {i}"))
                    .find(|n| !self.view.settings.scenes.iter().any(|s| &s.name == n))
                    .unwrap_or_default();
                self.local.selected_scene = Some(self.view.settings.scenes.len());
                self.send(Action::SaveCurrentAsScene(n));
            }
            Id::SceneDelete => {
                if let Some(i) = self.local.selected_scene.take() {
                    self.edit(move |s| {
                        if i < s.scenes.len() {
                            s.scenes.remove(i);
                        }
                    });
                    self.destroy_edits();
                }
            }
            Id::SceneEffect => {
                if let Some(i) = self.local.selected_scene {
                    self.edit(move |s| {
                        if let Some(sc) = s.scenes.get_mut(i) {
                            use model::SceneEffect as E;
                            sc.effect = match sc.effect {
                                E::None => E::Movie,
                                E::Movie => E::Darkroom,
                                E::Darkroom => E::Grayscale,
                                E::Grayscale => E::Amber,
                                E::Amber => E::Red,
                                E::Red => E::None,
                            };
                        }
                    });
                }
            }
            Id::SceneHotkey | Id::Hotkey(_) => self.begin_capture(id),
            Id::HotkeyClear(i) => {
                let action = model::HOTKEY_ACTIONS[i].0;
                self.edit(move |s| set_hotkey(s, action, String::new()));
            }
            Id::HotkeysReset => self.edit(|s| {
                s.hotkeys =
                    model::HOTKEY_ACTIONS.iter().map(|(a, b, _)| (a.to_string(), b.to_string())).collect()
            }),
            Id::PauseFullscreen => self.edit(|s| s.pause_fullscreen = !s.pause_fullscreen),
            Id::RuleAction(i) => {
                let seg = self.segment_at(id, x, 3);
                let first_scene =
                    self.view.settings.scenes.first().map(|s| s.name.clone()).unwrap_or_default();
                self.edit(move |s| {
                    if let Some(r) = s.rules.get_mut(i) {
                        r.action = match seg {
                            0 => model::RuleAction::Disable,
                            1 => model::RuleAction::NoOverlay,
                            _ => match &r.action {
                                model::RuleAction::Scene(n) => model::RuleAction::Scene(n.clone()),
                                _ => model::RuleAction::Scene(first_scene),
                            },
                        };
                    }
                });
            }
            Id::RuleScene(i) => self.edit(move |s| {
                // Cycle through the scenes.
                let names: Vec<String> = s.scenes.iter().map(|sc| sc.name.clone()).collect();
                if let Some(r) = s.rules.get_mut(i) {
                    if let model::RuleAction::Scene(n) = &r.action {
                        let next = names
                            .iter()
                            .position(|x| x == n)
                            .map(|p| (p + 1) % names.len().max(1))
                            .unwrap_or(0);
                        if let Some(nn) = names.get(next) {
                            r.action = model::RuleAction::Scene(nn.clone());
                        }
                    }
                }
            }),
            Id::RuleDelete(i) => self.edit(move |s| {
                if i < s.rules.len() {
                    s.rules.remove(i);
                }
            }),
            Id::RecentApp(i) => {
                if let Some(exe) = self.view.recent_apps.get(i).cloned() {
                    self.edit(move |s| add_rule(s, &exe));
                }
            }
            Id::Install => self.send(Action::Install),
            Id::Uninstall => self.send(Action::Uninstall),
            Id::CopyDiag => self.send(Action::CopyDiagnostics),
            Id::OpenFolder => self.send(Action::OpenSettingsFolder),
            Id::IdleMinutes
            | Id::IdleLevel
            | Id::MonShare(_)
            | Id::DayK
            | Id::EveningK
            | Id::NightK
            | Id::SunsetMin
            | Id::SunriseMin
            | Id::NightBright
            | Id::SceneBright
            | Id::SceneKelvin
            | Id::Timeline => {
                self.drag = Some(id);
                unsafe { SetCapture(self.hwnd) };
                self.on_drag(id, x);
            }
            Id::CityEdit | Id::Wake | Id::FixedDay | Id::FixedEvening | Id::SceneName | Id::RuleEdit => {}
        }
    }

    fn segment_at(&self, id: Id, x: f32, n: usize) -> usize {
        self.widget(id)
            .map(|w| ((widgets::slider_frac(w.rect, x) * n as f32) as usize).min(n - 1))
            .unwrap_or(0)
    }

    fn begin_capture(&mut self, id: Id) {
        self.local.capture = Some(id);
        self.send(Action::Capturing(true));
        unsafe {
            let _ = SetFocus(Some(self.hwnd));
        }
        self.rebuild();
    }

    fn end_capture(&mut self) {
        if self.local.capture.take().is_some() {
            self.send(Action::Capturing(false));
            self.rebuild();
        }
    }

    /// A key pressed while capturing a hotkey. Returns true if consumed.
    fn on_capture_key(&mut self, vk: u32) -> bool {
        let Some(id) = self.local.capture else { return false };
        if vk == VK_ESCAPE.0 as u32 {
            self.end_capture();
            return true;
        }
        let clear = vk == VK_BACK.0 as u32 || vk == VK_DELETE.0 as u32;
        let modifier_only = [
            VK_CONTROL,
            VK_MENU,
            VK_SHIFT,
            VK_LWIN,
            VK_RWIN,
            VK_LCONTROL,
            VK_RCONTROL,
            VK_LMENU,
            VK_RMENU,
            VK_LSHIFT,
            VK_RSHIFT,
        ]
        .iter()
        .any(|k| k.0 as u32 == vk);
        if modifier_only {
            return true;
        }
        let text = if clear {
            String::new()
        } else {
            let down = |k: VIRTUAL_KEY| unsafe { GetKeyState(k.0 as i32) } < 0;
            let mut mods = 0u32;
            if down(VK_CONTROL) {
                mods |= MOD_CONTROL.0;
            }
            if down(VK_MENU) {
                mods |= MOD_ALT.0;
            }
            if down(VK_SHIFT) {
                mods |= MOD_SHIFT.0;
            }
            if down(VK_LWIN) || down(VK_RWIN) {
                mods |= MOD_WIN.0;
            }
            if mods == 0 {
                return true; // a modifier is required; keep waiting
            }
            hotkeys::format(hotkeys::Binding { mods, vk })
        };
        match id {
            Id::Hotkey(i) => {
                let action = model::HOTKEY_ACTIONS[i].0;
                self.edit(move |s| set_hotkey(s, action, text));
            }
            Id::SceneHotkey => {
                if let Some(i) = self.local.selected_scene {
                    self.edit(move |s| {
                        if let Some(sc) = s.scenes.get_mut(i) {
                            sc.hotkey = text;
                        }
                    });
                }
            }
            _ => {}
        }
        self.end_capture();
        true
    }

    // ----- painting ------------------------------------------------------------------------------

    fn paint(&mut self) {
        let pal = self.palette;
        let (w, h) = self.client_dip();
        let hot = self.drag.or(self.hot);
        let page = self.page;
        let widgets_list = std::mem::take(&mut self.widgets);
        let view = self.view.clone();
        self.surface.paint(|p| {
            p.clear(pal.bg);
            // Navigation pane.
            p.fill(Rect::new(0.0, 0.0, NAV_W, h), pal.surface);
            p.fill(Rect::new(NAV_W - 1.0, 0.0, 1.0, h), pal.border);
            p.text(
                glyph::BRIGHTNESS,
                Rect::new(20.0, 16.0, 24.0, 32.0),
                18.0,
                Weight::Icon,
                Align::Left,
                pal.accent,
            );
            p.text(
                "Screen Lighting",
                Rect::new(48.0, 16.0, NAV_W - 56.0, 32.0),
                15.0,
                Weight::Semibold,
                Align::Left,
                pal.text,
            );
            for (i, (pg, icon, label)) in PAGES.iter().enumerate() {
                let r = Rect::new(10.0, 64.0 + i as f32 * 40.0, NAV_W - 20.0, 36.0);
                let selected = *pg == page;
                if selected || hot == Some(Id::Nav(i)) {
                    p.fill_round(
                        r,
                        6.0,
                        if selected {
                            pal.surface_hover
                        } else {
                            d2d::mix(pal.surface, pal.surface_hover, 0.6)
                        },
                    );
                }
                if selected {
                    p.fill_round(Rect::new(r.x, r.y + 9.0, 3.0, 18.0), 1.5, pal.accent);
                }
                p.text(
                    icon,
                    Rect::new(r.x + 14.0, r.y, 20.0, r.h),
                    14.0,
                    Weight::Icon,
                    Align::Left,
                    pal.text,
                );
                p.text(
                    label,
                    Rect::new(r.x + 44.0, r.y, r.w - 44.0, r.h),
                    13.5,
                    Weight::Regular,
                    Align::Left,
                    pal.text,
                );
            }
            let _clip = p.clip(Rect::new(NAV_W, 0.0, w - NAV_W, h));
            for wd in widgets_list.iter().filter(|wd| !matches!(wd.id, Some(Id::Nav(_)))) {
                let r = wd.rect;
                if r.bottom() < 0.0 || r.y > h {
                    continue;
                }
                let is_hot = wd.id.is_some() && hot == wd.id;
                match &wd.kind {
                    Kind::Title(t) => p.text(t, r, 24.0, Weight::Semibold, Align::Left, pal.text),
                    Kind::Heading(t) => p.text(t, r, 14.0, Weight::Semibold, Align::Left, pal.text),
                    Kind::Label(t, d) => {
                        if d.is_empty() {
                            p.text(t, r, 13.5, Weight::Regular, Align::Left, pal.text);
                        } else {
                            p.text(
                                t,
                                Rect::new(r.x, r.y + 2.0, r.w, 20.0),
                                13.5,
                                Weight::Regular,
                                Align::Left,
                                pal.text,
                            );
                            let dc = if d.starts_with("In use") { pal.danger } else { pal.subtext };
                            p.text_wrapped(d, Rect::new(r.x, r.y + 23.0, r.w, r.h - 23.0), 12.0, dc);
                        }
                    }
                    Kind::Text(t, c) => p.text_wrapped(t, r, 12.5, *c),
                    Kind::Toggle(on) => {
                        widgets::toggle(p, Rect::new(r.right() - 40.0, r.y, 40.0, r.h), *on, &pal, is_hot)
                    }
                    Kind::Slider { frac, value, warmth } => {
                        widgets::slider(p, slider_track(r), *frac, &pal, is_hot, *warmth);
                        p.text(
                            value,
                            Rect::new(r.x, r.y, VALUE_W, r.h),
                            12.5,
                            Weight::Regular,
                            Align::Right,
                            pal.subtext,
                        );
                    }
                    Kind::Segmented(opts, sel) => {
                        p.fill_round(r, 6.0, pal.surface);
                        p.stroke_round(r, 6.0, pal.border, 1.0);
                        let sw = r.w / opts.len() as f32;
                        for (i, o) in opts.iter().enumerate() {
                            let sr = Rect::new(r.x + sw * i as f32, r.y, sw, r.h).inset(3.0, 3.0);
                            if i == *sel {
                                p.fill_round(sr, 4.0, pal.accent);
                            }
                            let c = if i == *sel { pal.on_accent } else { pal.text };
                            p.text(o, sr, 12.5, Weight::Regular, Align::Center, c);
                        }
                    }
                    Kind::Button(label, primary) => {
                        if label.chars().count() == 1 {
                            widgets::button(p, r, Some(label), "", &pal, is_hot, *primary);
                        } else {
                            widgets::button(p, r, None, label, &pal, is_hot, *primary);
                        }
                    }
                    Kind::Edit => {
                        p.fill_round(r, 6.0, pal.surface);
                        p.stroke_round(r, 6.0, if is_hot { pal.subtext } else { pal.border }, 1.0);
                        if wd.id == Some(Id::CityEdit) {
                            p.text(
                                glyph::SEARCH,
                                Rect::new(r.right() - 28.0, r.y, 20.0, r.h),
                                12.0,
                                Weight::Icon,
                                Align::Center,
                                pal.subtext,
                            );
                        }
                    }
                    Kind::Chip(label, selected) => widgets::chip(p, r, label, &pal, is_hot, *selected),
                    Kind::KeyBox(text, capturing, conflict) => {
                        let border = if *capturing {
                            pal.accent
                        } else if *conflict {
                            pal.danger
                        } else if is_hot {
                            pal.subtext
                        } else {
                            pal.border
                        };
                        p.fill_round(r, 6.0, pal.surface);
                        p.stroke_round(r, 6.0, border, if *capturing { 2.0 } else { 1.0 });
                        let t = if *capturing { "Press keys…" } else { text.as_str() };
                        p.text(
                            t,
                            r,
                            12.5,
                            Weight::Semibold,
                            Align::Center,
                            if *capturing { pal.accent } else { pal.text },
                        );
                    }
                    Kind::Card => {
                        p.fill_round(r, 8.0, d2d::mix(pal.surface, pal.danger, 0.12));
                    }
                    Kind::Timeline => paint_timeline(p, r, &view.settings.schedule, &pal),
                }
            }
        });
        self.widgets = widgets_list;
    }

    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        let dip = |lp: LPARAM, s: f32| {
            let (x, y) = win::point_from(lp.0 as usize);
            (x as f32 / s, y as f32 / s)
        };
        match msg {
            WM_PAINT => unsafe {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(self.hwnd, &mut ps);
                self.paint();
                let _ = EndPaint(self.hwnd, &ps);
                Some(LRESULT(0))
            },
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_SIZE => {
                self.rebuild();
                Some(LRESULT(0))
            }
            WM_GETMINMAXINFO => unsafe {
                let mmi = &mut *(lp.0 as *mut MINMAXINFO);
                let s = self.scale();
                mmi.ptMinTrackSize = POINT { x: (MIN_W * s) as i32, y: (MIN_H * s) as i32 };
                Some(LRESULT(0))
            },
            WM_DPICHANGED => unsafe {
                let r = &*(lp.0 as *const RECT);
                let _ = SetWindowPos(
                    self.hwnd,
                    None,
                    r.left,
                    r.top,
                    r.right - r.left,
                    r.bottom - r.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                self.rebuild();
                Some(LRESULT(0))
            },
            WM_LBUTTONDOWN => {
                let (x, y) = dip(lp, self.scale());
                unsafe {
                    let _ = SetFocus(Some(self.hwnd));
                }
                if let Some(id) = self.hit(x, y) {
                    if self.local.capture.is_some() && !matches!(id, Id::Hotkey(_) | Id::SceneHotkey) {
                        self.end_capture();
                    }
                    self.on_click(id, x);
                } else {
                    self.end_capture();
                }
                Some(LRESULT(0))
            }
            WM_MOUSEMOVE => {
                let (x, y) = dip(lp, self.scale());
                if let Some(id) = self.drag {
                    self.on_drag(id, x);
                } else {
                    let hot = self.hit(x, y);
                    if hot != self.hot {
                        self.hot = hot;
                        unsafe {
                            let _ = InvalidateRect(Some(self.hwnd), None, false);
                            let mut tme = TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: self.hwnd,
                                dwHoverTime: 0,
                            };
                            let _ = TrackMouseEvent(&mut tme);
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.hot = None;
                unsafe {
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if let Some(id) = self.drag.take() {
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    if matches!(id, Id::Timeline | Id::DayK | Id::EveningK | Id::NightK | Id::SceneKelvin) {
                        self.send(Action::Preview(None));
                    }
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = win::hiword(wp.0) as u16 as i16 as f32;
                let (_, h) = self.client_dip();
                self.scroll = (self.scroll - delta / 120.0 * 48.0).clamp(0.0, (self.content_h - h).max(0.0));
                self.rebuild();
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                if self.on_capture_key(wp.0 as u32) {
                    return Some(LRESULT(0));
                }
                None
            }
            WM_COMMAND => {
                let code = win::hiword(wp.0);
                self.on_edit_command(code, HWND(lp.0 as *mut _));
                Some(LRESULT(0))
            }
            WM_CTLCOLOREDIT => unsafe {
                let hdc = HDC(wp.0 as *mut _);
                SetTextColor(hdc, colorref(self.palette.text));
                SetBkColor(hdc, colorref(self.palette.surface));
                Some(LRESULT(self.edit_brush.0 as isize))
            },
            WM_ACTIVATE => {
                // While active, stay above SLC's own dimming overlays so the controls are readable.
                let active = win::loword(wp.0) != WA_INACTIVE;
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(if active { HWND_TOPMOST } else { HWND_NOTOPMOST }),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                }
                if !active {
                    self.end_capture();
                }
                None
            }
            WM_CLOSE => {
                self.end_capture();
                self.send(Action::Closed);
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// Adds a "disable" rule for `exe` unless one exists.
fn add_rule(s: &mut Settings, exe: &str) {
    if let Some(exe) = model::normalize_exe(exe) {
        if !s.rules.iter().any(|r| r.exe == exe) {
            s.rules.push(model::Rule { exe, action: model::RuleAction::Disable });
        }
    }
}

fn set_hotkey(s: &mut Settings, action: &str, binding: String) {
    // One binding per shortcut: taking it from another action clears it there.
    if !binding.is_empty() {
        for (a, b) in s.hotkeys.iter_mut() {
            if a != action && b.eq_ignore_ascii_case(&binding) {
                b.clear();
            }
        }
        for sc in s.scenes.iter_mut() {
            if sc.hotkey.eq_ignore_ascii_case(&binding) {
                sc.hotkey.clear();
            }
        }
    }
    if let Some((_, b)) = s.hotkeys.iter_mut().find(|(a, _)| a == action) {
        *b = binding;
    }
}

fn colorref(c: d2d::Color) -> COLORREF {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    COLORREF(b(c.r) | (b(c.g) << 8) | (b(c.b) << 16))
}

/// 24-hour warmth curve with today's events and a "now" marker.
fn paint_timeline(p: &d2d::Painter, r: Rect, sc: &model::Schedule, pal: &Palette) {
    let (y, doy, now, off) = schedule::now_local();
    let ev = schedule::events(sc, y, doy, off);
    p.fill_round(r, 8.0, pal.surface);
    let plot = r.inset(10.0, 10.0);
    let plot = Rect::new(plot.x, plot.y, plot.w, plot.h - 18.0);
    const N: usize = 96;
    let bw = plot.w / N as f32;
    for i in 0..N {
        let minute = (i as f64 + 0.5) * 1440.0 / N as f64;
        let k = schedule::target_at(sc, &ev, minute).kelvin;
        let f = kelvin_frac(k);
        let bh = 8.0 + f * (plot.h - 8.0);
        p.fill(
            Rect::new(plot.x + bw * i as f32, plot.bottom() - bh, bw + 0.5, bh),
            super::osd::kelvin_color(k),
        );
    }
    let mx = plot.x + plot.w * (now / 1440.0) as f32;
    p.fill(Rect::new(mx - 1.0, plot.y - 4.0, 2.0, plot.h + 4.0), pal.text);
    for (h, label) in [(0.0, "0:00"), (6.0, "6:00"), (12.0, "12:00"), (18.0, "18:00"), (24.0, "24:00")] {
        let x = plot.x + plot.w * (h / 24.0);
        let align = if h == 0.0 {
            Align::Left
        } else if h == 24.0 {
            Align::Right
        } else {
            Align::Center
        };
        let lr = match align {
            Align::Left => Rect::new(x, plot.bottom() + 2.0, 60.0, 16.0),
            Align::Right => Rect::new(x - 60.0, plot.bottom() + 2.0, 60.0, 16.0),
            Align::Center => Rect::new(x - 30.0, plot.bottom() + 2.0, 60.0, 16.0),
        };
        p.text(label, lr, 11.0, Weight::Regular, align, pal.subtext);
    }
}

/// Width of the value label drawn at the left of a settings slider.
const VALUE_W: f32 = 132.0;

/// The draggable track of a settings slider (the value label sits to its left).
fn slider_track(r: Rect) -> Rect {
    Rect::new(r.x + VALUE_W + 12.0, r.y, (r.w - VALUE_W - 12.0).max(40.0), r.h)
}

type Control = (Option<Id>, f32, f32, Kind);

/// Accumulates widgets top to bottom.
struct Builder {
    out: Vec<Widget>,
    x: f32,
    y: f32,
    w: f32,
}

impl Builder {
    fn push(&mut self, id: Option<Id>, rect: Rect, kind: Kind) {
        self.out.push(Widget { id, rect, kind });
    }
    fn title(&mut self, t: &str) {
        self.push(None, Rect::new(self.x, self.y, self.w, 40.0), Kind::Title(t.into()));
        self.y += 52.0;
    }
    fn heading(&mut self, t: &str) {
        self.y += 10.0;
        self.push(None, Rect::new(self.x, self.y, self.w, 24.0), Kind::Heading(t.into()));
        self.y += 32.0;
    }
    fn text(&mut self, t: &str, c: d2d::Color) {
        let h = d2d::measure_wrapped(t, 12.5, self.w).max(18.0);
        self.push(None, Rect::new(self.x, self.y, self.w, h + 2.0), Kind::Text(t.into(), c));
        self.y += h + 6.0;
    }
    fn card_text(&mut self, t: &str, c: d2d::Color) {
        let h = d2d::measure_wrapped(t, 12.5, self.w - 28.0).max(18.0);
        self.push(None, Rect::new(self.x, self.y, self.w, h + 20.0), Kind::Card);
        self.push(
            None,
            Rect::new(self.x + 14.0, self.y + 10.0, self.w - 28.0, h + 2.0),
            Kind::Text(t.into(), c),
        );
        self.y += h + 32.0;
    }
    /// A settings row: label + wrapped description on the left, controls right-aligned
    /// (first control is rightmost). The row grows to fit the description.
    fn row(&mut self, label: &str, desc: &str, controls: Vec<Control>) {
        let controls_w: f32 = controls.iter().map(|c| c.1 + 8.0).sum();
        let label_w = (self.w - controls_w - 16.0).max(160.0);
        let desc_h = if desc.is_empty() { 0.0 } else { d2d::measure_wrapped(desc, 12.0, label_w) };
        let h = (22.0 + desc_h + 4.0).max(ROW);
        self.push(None, Rect::new(self.x, self.y, label_w, h), Kind::Label(label.into(), desc.into()));
        let mut right = self.x + self.w;
        for (id, w, ch, kind) in controls {
            let x = right - w;
            self.push(id, Rect::new(x, self.y + (h - ch) / 2.0, w, ch), kind);
            right = x - 8.0;
        }
        self.y += h + 8.0;
    }
    fn toggle_row(&mut self, id: Id, label: &str, desc: &str, on: bool) {
        self.row(label, desc, vec![(Some(id), 40.0, 24.0, Kind::Toggle(on))]);
    }
    fn slider_row(&mut self, id: Id, label: &str, desc: &str, frac: f32, value: String, warmth: bool) {
        let w = (self.w * 0.55).clamp(300.0, 440.0);
        self.row(label, desc, vec![(Some(id), w, widgets::SLIDER_H, Kind::Slider { frac, value, warmth })]);
    }
    fn push_line(&mut self, id: Option<Id>, h: f32, kind: Kind) {
        let w = match &kind {
            Kind::Button(l, _) => d2d::measure(l, 13.0, Weight::Regular) + 40.0,
            Kind::Chip(l, _) => widgets::chip_width(l),
            _ => self.w,
        };
        self.push(id, Rect::new(self.x, self.y, w.min(self.w), h), kind);
        self.y += h + 8.0;
    }
    fn chips(&mut self, items: Vec<(Id, String, bool)>) {
        let (mut x, h) = (self.x, 30.0);
        for (id, label, sel) in items {
            let w = widgets::chip_width(&label);
            if x + w > self.x + self.w {
                x = self.x;
                self.y += h + 8.0;
            }
            self.push(Some(id), Rect::new(x, self.y, w, h), Kind::Chip(label, sel));
            x += w + 8.0;
        }
        self.y += h + 12.0;
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        unsafe {
            SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
            self.destroy_edits();
            let _ = DestroyWindow(self.hwnd);
            if !self.font.is_invalid() {
                let _ = DeleteObject(self.font.into());
            }
            if !self.edit_brush.is_invalid() {
                let _ = DeleteObject(self.edit_brush.into());
            }
        }
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let cs = &*(lp.0 as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
    }
    let sw = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsWindow;
    if !sw.is_null() {
        if let Some(r) = (*sw).handle(msg, wp, lp) {
            return r;
        }
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_hotkey_moves_binding() {
        let mut s = Settings::default();
        let taken = s.hotkey("panic").to_string();
        set_hotkey(&mut s, "pause", taken.clone());
        assert_eq!(s.hotkey("pause"), taken);
        assert_eq!(s.hotkey("panic"), "", "previous owner loses the binding");
        set_hotkey(&mut s, "pause", String::new());
        assert_eq!(s.hotkey("pause"), "");
    }

    #[test]
    fn colorref_packs_bgr() {
        assert_eq!(colorref(d2d::rgb(0x112233)).0, 0x332211);
    }
}
