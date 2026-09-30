//! Shared drawing for custom controls (sliders, buttons, chips, toggles).

use super::d2d::{self, Align, Color, Painter, Rect, Weight};
use super::theme::Palette;
use crate::color;

/// Segoe MDL2 Assets glyphs.
pub mod glyph {
    pub const SETTINGS: &str = "\u{E713}";
    pub const PAUSE: &str = "\u{E769}";
    pub const PLAY: &str = "\u{E768}";
    pub const MONITOR: &str = "\u{E7F4}";
    pub const CLOSE: &str = "\u{E711}";
    pub const WARNING: &str = "\u{E7BA}";
    pub const CLOCK: &str = "\u{E823}";
    pub const SEARCH: &str = "\u{E721}";
    pub const KEYBOARD: &str = "\u{E765}";
    pub const INFO: &str = "\u{E946}";
    pub const PALETTE: &str = "\u{E790}";
    pub const APPS: &str = "\u{E71D}";
}

pub const SLIDER_H: f32 = 24.0;
const TRACK_H: f32 = 4.0;
const THUMB_R: f32 = 9.0;

/// Maps an x position on a slider track to 0–1.
pub fn slider_frac(track: Rect, x: f32) -> f32 {
    ((x - track.x) / track.w.max(1.0)).clamp(0.0, 1.0)
}

/// A slider in `r` (full row); `frac` 0–1. `gradient` draws a warmth track instead of the accent fill.
pub fn slider(p: &Painter, r: Rect, frac: f32, pal: &Palette, hot: bool, warmth: bool) {
    let cy = r.y + r.h / 2.0;
    let track = Rect::new(r.x, cy - TRACK_H / 2.0, r.w, TRACK_H);
    let x = r.x + r.w * frac.clamp(0.0, 1.0);
    if warmth {
        // Kelvin gradient from warm (left) to neutral (right).
        const STEPS: usize = 24;
        let seg = track.w / STEPS as f32;
        for i in 0..STEPS {
            let t = (i as f32 + 0.5) / STEPS as f32;
            let k = color::MIN_KELVIN as f32 + t * (color::MAX_KELVIN - color::MIN_KELVIN) as f32;
            // A muted hint of the real color (the only non-gray element of the monotone UI).
            let c = d2d::mix(super::osd::kelvin_color(k as u32), pal.track, 0.65);
            p.fill(Rect::new(track.x + seg * i as f32, track.y - 1.0, seg + 0.5, TRACK_H + 2.0), c);
        }
    } else {
        p.fill_round(track, TRACK_H / 2.0, pal.track);
        p.fill_round(
            Rect::new(track.x, track.y, (x - track.x).max(TRACK_H), TRACK_H),
            TRACK_H / 2.0,
            pal.accent,
        );
    }
    let r_out = if hot { THUMB_R + 1.0 } else { THUMB_R };
    p.circle(x, cy, r_out, pal.surface);
    p.ring(x, cy, r_out, pal.border, 1.0);
    p.circle(x, cy, if hot { 5.5 } else { 4.5 }, pal.accent);
}

/// A flat button with an optional icon glyph.
pub fn button(
    p: &Painter,
    r: Rect,
    icon: Option<&str>,
    label: &str,
    pal: &Palette,
    hot: bool,
    primary: bool,
) {
    let (bg, fg) = if primary {
        (if hot { d2d::mix(pal.accent, pal.text, 0.12) } else { pal.accent }, pal.on_accent)
    } else {
        (if hot { pal.surface_hover } else { pal.surface }, pal.text)
    };
    p.fill_round(r, 6.0, bg);
    if !primary {
        p.stroke_round(r, 6.0, pal.border, 1.0);
    }
    match icon {
        Some(g) if label.is_empty() => {
            p.text(g, r, 14.0, Weight::Icon, Align::Center, fg);
        }
        Some(g) => {
            let tw = d2d::measure(label, 13.0, Weight::Regular);
            let total = 16.0 + 8.0 + tw;
            let x0 = r.x + (r.w - total) / 2.0;
            p.text(g, Rect::new(x0, r.y, 16.0, r.h), 13.0, Weight::Icon, Align::Center, fg);
            p.text(label, Rect::new(x0 + 24.0, r.y, tw + 2.0, r.h), 13.0, Weight::Regular, Align::Left, fg);
        }
        None => p.text(label, r, 13.0, Weight::Regular, Align::Center, fg),
    }
}

/// Width a chip needs for `label`.
pub fn chip_width(label: &str) -> f32 {
    d2d::measure(label, 12.5, Weight::Regular) + 24.0
}

pub fn chip(p: &Painter, r: Rect, label: &str, pal: &Palette, hot: bool, selected: bool) {
    let bg = if selected {
        d2d::mix(pal.surface, pal.accent, 0.25)
    } else if hot {
        pal.surface_hover
    } else {
        pal.surface
    };
    p.fill_round(r, r.h / 2.0, bg);
    p.stroke_round(r, r.h / 2.0, if selected { pal.accent } else { pal.border }, 1.0);
    p.text(label, r, 12.5, Weight::Regular, Align::Center, pal.text);
}

/// An on/off switch (40×20).
pub fn toggle(p: &Painter, r: Rect, on: bool, pal: &Palette, hot: bool) {
    let t = Rect::new(r.x, r.y + (r.h - 20.0) / 2.0, 40.0, 20.0);
    if on {
        p.fill_round(t, 10.0, if hot { d2d::mix(pal.accent, pal.text, 0.1) } else { pal.accent });
        p.circle(t.right() - 10.0, t.y + 10.0, 6.0, pal.on_accent);
    } else {
        p.fill_round(t, 10.0, if hot { pal.surface_hover } else { pal.surface });
        p.stroke_round(t, 10.0, pal.subtext, 1.0);
        p.circle(t.x + 10.0, t.y + 10.0, 5.0, pal.subtext);
    }
}

/// Label on the left and value on the right of one row.
pub fn label_value(p: &Painter, r: Rect, label: &str, value: &str, pal: &Palette) {
    p.text(label, r, 13.0, Weight::Semibold, Align::Left, pal.text);
    p.text(value, r, 13.0, Weight::Regular, Align::Right, pal.subtext);
}

pub fn separator(p: &Painter, x0: f32, x1: f32, y: f32, c: Color) {
    p.fill(Rect::new(x0, y, x1 - x0, 1.0), c);
}
