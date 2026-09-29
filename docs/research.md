# SLC — Research: Dimmer + f.lux

Screen Lighting Control (SLC) combines the ideas of two existing tools — **Dimmer** (Nelson Pires) and **f.lux** — and improves on both.

_Research date: 2026-09-29_

---

## 1. Dimmer (Nelson Pires)

- **What it does:** dims the screen *below* the monitor's hardware minimum. Windows-only, portable (~56–90 KB, no install), freeware.
- **How it works:** a click-through, semi-transparent **black overlay window** on each screen. It does not touch the backlight or the gamma ramps.
- **Features (v2.0.1, 2022-12-01):**
  - Per-monitor sliders (unlimited monitors) plus a "Master" slider for all screens
  - Per-monitor enable/disable checkbox; per-monitor DPI awareness
  - Experimental per-monitor RGB tint
  - Option to exclude its own window from dimming (so you can still find the controls)
  - Tray icon and menu, autorun, start minimized, remembers the last level
  - Mouse-wheel on the slider, single instance, `.ini` settings
  - Debug tab with system/display info; optional 5px yellow boundary to identify screens
- **Weaknesses:**
  - No hotkeys, no schedule
  - Inverted slider (up = darker), range capped at about 0–90
  - Overlay shows up in screenshots and screen recordings
  - Backlight stays at full power: no energy savings, and contrast gets worse
  - Hardware mouse cursor is not dimmed
  - Can be flagged by game anti-cheat; sometimes triggers AV false positives
  - Glitches with auto-hide taskbars and duplicated-display mode

## 2. f.lux

- **What it does:** changes display color temperature automatically by time of day and location (sunrise/sunset): daylight during the day, warm at night. Windows/macOS/Linux (the Linux xflux version is stale). Free; paid Corporate edition.
- **How it works:** writes **gamma ramps** (the GPU's color lookup tables) and reads the VCGT tag in ICC profiles so an existing calibration is kept. Location is rounded to 0.1° for privacy.
- **Kelvin presets:** Ember 1200K · Candle 1900K · Warm Incandescent 2300K · Incandescent 2700K · Halogen 3400K · Fluorescent 4200K · Sunlight 5500K
- **Features:**
  - Wake-time-based schedule; transition speed (fast, or a slow 1-hour fade)
  - **Darkroom** (inverted colors, red only), **Movie mode** (2.5 h, keeps skin tones and shadow detail)
  - Backwards alarm clock (a reminder of how close bedtime is)
  - Hotkeys: Alt+End (toggle), Alt+PgUp/PgDn (dim), Alt+Shift+End (Darkroom)
  - Disable for 1 hour, until sunrise, for fullscreen apps, or for specific apps
  - Philips Hue / smart-lighting control
  - Optional software mouse cursor, so the cursor is tinted too
- **Weaknesses:**
  - Windows limits the gamma-ramp range; below ~2700K needs "Expand Color Range" (admin registry change + reboot)
  - Conflicts with Windows Night Light and GPU driver color overrides; flicker reports
  - Hardware cursor is not tinted
  - Dated UI, slow development
  - No real per-monitor control

## 3. The three ways to control a screen

| Method | Used by | Pros | Cons |
|---|---|---|---|
| **Overlay** | Dimmer | Works everywhere, very deep dimming, per-monitor, can tint | Appears in screenshots, no power savings, cursor not dimmed, anti-cheat flags, lower contrast |
| **Gamma ramp / color transform** | f.lux, Night Light, Redshift | Invisible to screenshots, precise color temperature | OS range limits, conflicts with other apps and drivers, some drivers ignore it |
| **DDC/CI (hardware)** | Twinkle Tray, Monitorian, ClickMonitorDDC | Real backlight: saves power, keeps contrast | Slow (~50–200 ms per change), not every monitor supports it, can't go below the hardware minimum, frequent writes may wear out the monitor's settings memory |

**SLC's main idea:** no existing tool combines all three. One brightness slider per monitor that first lowers the **hardware backlight** (DDC/CI, or WMI for laptop panels), then uses **gamma dimming**, and uses the **overlay** only after the hardware minimum. That gives power savings and contrast where possible, and deep dimming where it's needed.

## 4. Brainstorm: improvements

### Core engine
1. The combined brightness pipeline above (hardware, then gamma, then overlay) on one smooth slider.
2. Independent brightness and warmth per monitor, with optional "link all".
3. Use the modern Windows color pipeline (MHC / display color transform, as Night Light uses) where available: no 2700K limit and no reboot. Fall back to gamma, then overlay.
4. Detect conflicts: warn about Night Light, f.lux, GPU driver color overrides, or HDR, and offer to resolve them.
5. HDR awareness: use the SDR-content brightness setting or the overlay when HDR is on.

### Scheduling and automation
6. Schedule modes: sun-based (city-only privacy option, no GPS), fixed time, or wake-time based. Each point sets brightness **and** Kelvin.
7. Schedule editor with a graph: drag points on a 24-hour curve, and preview any time of day instantly.
8. Ambient light: the laptop's light sensor (or a webcam estimate) adjusts to the room.
9. Per-app rules: color-critical apps turn warmth off; games switch to hardware-only dimming to avoid anti-cheat; video players get Movie mode; fullscreen and screen-share detection.
10. Idle fade: dim after N minutes of inactivity, restore on input.

### Usability (fixing Dimmer's rough spots)
11. Global hotkeys for everything; mouse wheel over the tray icon.
12. Named presets / "scenes" (Reading, Movie, Darkroom, Presentation) from the tray, a hotkey, or the CLI.
13. Intuitive 0–100% (100 = bright) and a panic hotkey that restores full brightness.
14. Optional software cursor so the cursor gets dimmed too.
15. Hide the overlay from screenshots and recordings with `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` (Win10 2004+).

### Health and comfort
16. Wind-down reminders; optional 20-20-20 eye-break reminder.
17. Reduce blue light without an orange cast; darkroom tints other than red (amber, grayscale).
18. Smooth, flicker-free transitions with configurable easing.

### Integrations and extras
19. Smart lights (Philips Hue, Home Assistant, LIFX) following the same schedule.
20. CLI and local API (scripts, Stream Deck, AutoHotkey).
21. Cross-platform core later (Linux/Wayland, macOS).
22. Portable build plus installer, signed binaries, open source.

---

## Sources
- [Dimmer – Nelson Pires](https://www.nelsonpires.com/software/dimmer)
- [Dimmer changelog v2.0.1](https://www.nelsonpires.com/assets/downloads/dimmer/2.0.1/whatsnew.txt)
- [gHacks review of Dimmer](https://www.ghacks.net/2020/08/20/dimmer-is-a-freeware-tool-that-puts-an-overlay-on-the-screen-to-reduce-the-brightness-level/)
- [BetterDisplay discussion on overlays and the cursor](https://github.com/waydabber/BetterDisplay/discussions/3421)
- [f.lux](https://justgetflux.com/)
- [f.lux FAQ](https://justgetflux.com/faq.html)
- [IrisTech f.lux 4 review](https://iristech.co/f-lux-4-beta-review-windows/)
- [f.lux forum – Darkroom hotkey](https://forum.justgetflux.com/topic/4009/hotkey-for-darkroom-mode)
- [f.lux – Wikipedia](https://en.wikipedia.org/wiki/F.lux)
