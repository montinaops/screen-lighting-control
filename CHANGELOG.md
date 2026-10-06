# Changelog

All notable changes to SLC. Versions follow [SemVer](https://semver.org/).

## [Unreleased]

## [1.5.2] - 2026-10-06

### Fixed
- The tray flyout no longer jumps up a little when you click "Return to schedule" (or when the deep-dim banner or a
  display appears or disappears). It now keeps its bottom edge in place and only grows or shrinks upward.

## [1.5.1] - 2026-10-03

### Fixed
- OSD messages no longer cut off long text: the popup widens to fit (up to 440 px) and stays centered. Found in the
  1.5.0 live test — "Back to work — next break in 30 min" lost its last letters, and the eye-break countdown its "s".

## [1.5.0] - 2026-10-03

### Added
- Computer breaks (Settings › General › Reminders): after a stretch of activity (every 30–120 min, default 60), an OSD
  countdown reminds you to step away from the screen for 5, 10 or 15 minutes. A gentle reminder only — nothing is
  dimmed or blocked. Being away for a whole break counts as one; skipped in fullscreen apps and while paused. A
  computer break also resets the 20-20-20 clock, replaces a running eye break, and an eye break right before one is
  skipped.

## [1.4.3] - 2026-10-01

### Changed
- The app icon (Start menu, title bar, taskbar, Apps & features) is now just the white moon on a transparent
  background — the dark tile behind it was removed at the owner's request.

## [1.4.2] - 2026-09-30

### Changed
- Back to the flat monochrome mark (plain white moon on dark, ink on light; white moon on a near-black tile for the
  app icon). The 1.4.1 gradient was removed at the owner's request.

## [1.4.1] - 2026-09-30

### Changed
- The eclipse mark had a monochrome gradient (reverted in 1.4.2).

## [1.4.0] - 2026-09-30

### Changed
- **New monotone, minimalist look.** New mark: an eclipse (crescent). The tray icon is white on a dark taskbar and
  near-black on a light one (it follows the Windows taskbar theme); the app icon is a white crescent on a near-black
  rounded tile. The flyout, OSD and settings window are grayscale — white on dark / ink on light — with the warmth
  slider keeping only a muted hint of the real color. Warnings use neutral cards instead of red.
- **Privacy:** the tray flyout no longer shows your city or country ("Automatic · Night"), so nothing identifying is
  visible if the panel is open while streaming or screen sharing. (The location is only shown in Settings › Schedule.)

## [1.3.4] - 2026-09-30

### Fixed
- Left-clicking the tray icon opened the panel and closed it again right away (Windows sends two notifications per
  click; SLC toggled on both). The panel now opens on click and closes on the next click, Esc or clicking elsewhere.
- Right-click could open the menu twice for one click.

### Added
- Tray menu: **Open Screen Lighting Control** and **Settings…** at the top.
- The tray menu follows the light/dark theme instead of always being white.

## [1.3.3] - 2026-09-30

### Changed
- **Uninstall removes everything** — no settings question anymore: program folder, settings (profile and portable),
  Start menu shortcut, Apps & features entry, autostart and SLC's registry key. If SLC itself enabled the Windows
  color-range setting, uninstall restores it too (Windows asks for administrator permission once); if another program
  had enabled it, it is left alone.

### Fixed
- The program folder could survive uninstall while the final "removed" message was open; cleanup now retries until SLC
  has exited.
- After installing, SLC no longer keeps the folder it was installed from in use (it now starts in its own folder).

## [1.3.2] - 2026-09-30

### Added
- First start: if Windows still limits the color range, SLC explains why and offers to lift it. Windows asks for
  administrator permission once; the change takes effect after signing in again. Asked only once (remembered in
  `state.ini`); still available in Settings › Displays.

## [1.3.1] - 2026-09-30

### Fixed
- `slc.exe --reset` now also sets every monitor's backlight to 100% (it only reset colors and the pointer, so a dimmed
  backlight survived a crash), closes a running instance first, and saves 100% / neutral warmth so the next start
  doesn't dim again.

## [1.3.0] - 2026-09-30

Milestone v1.3. Smart-light integrations were dropped so SLC stays network-free (decision D18).

### Added
- Automatic brightness from the ambient light sensor (Settings › Displays, shown only when the PC has a sensor):
  a logarithmic lux→brightness curve with smoothing and 3% hysteresis; manual brightness changes teach an offset.

## [1.2.0] - 2026-09-30

Milestone v1.2: comfort.

### Added
- Dim the mouse pointer too (Settings › Displays): while the overlay dims a screen, the system pointers are replaced by
  copies darkened by the same amount (in 10% steps); restored on exit, `--reset` and crash recovery.
- Reminders (Settings › General): 20-20-20 eye breaks (a 20-second OSD countdown after 20 minutes of continuous
  activity; a 5-minute pause counts as a break; skipped in fullscreen apps) and a bedtime reminder N minutes before
  bedtime. The tray tooltip counts down to bedtime in the last 3 hours (f.lux's "backwards alarm clock").
- Color filters: Grayscale, Amber night (low blue light without an orange cast) and Red night, alongside Darkroom —
  tray "Color filter" menu, scene effects (editable in Settings › Scenes), `--filter <name>`. All use the crash-safe
  Magnification matrix.

## [1.1.0] - 2026-09-30

Milestone v1.1: automation.

### Added
- Per-app rules (Settings › Rules): while a given program is in front, turn SLC off (color-critical work), drop only
  the overlay (games with anti-cheat, capture tools), or apply a scene. Add rules by name or from recently used apps.
- Pause in fullscreen apps (games, videos, presentations), re-checked every 5 s for apps that go fullscreen in place.
- Dim when idle (Settings › General): after N minutes without input, fade to a set brightness over 2 s and restore on
  the next input. Skipped while sound is playing or a fullscreen app is in front.

## [1.0.0] - 2026-09-30

First release: the v1.0 core (see `docs/ROADMAP.md`).

### Dimming
- Hybrid pipeline per display: **hardware backlight** (DDC/CI VCP 0x10 with retry; laptop panel IOCTL) → **gamma**
  (as far as Windows allows, with a learned per-display bound) → **overlay** for the rest, down to 1%.
- Overlays are click-through, topmost, destroyed at 0%, and **hidden from screenshots / screen sharing**.
- Starting SLC never changes the screen: it adopts each display's current backlight; uninstall restores it.
- Hardware and gamma writes run on worker threads (debounced, latest value wins); the UI never blocks.
- Per-display brightness with a master control that keeps relative offsets; per-display backlight share and enable.

### Color
- Warmth 1200K–6500K (f.lux preset names), mixed in mired space.
- **Automatic schedule**: offline NOAA sunrise/sunset for a city picked from 1,305 embedded cities (or fixed times),
  wake-time aware, smooth transitions, optional night brightness ceiling, f.lux-style overrides until the next phase.
- **Darkroom** (red-only, inverted; Magnification color matrix, removed by Windows if SLC dies) and **Movie mode**.
- Expand color range (elevated) for Windows' gamma limit; HDR displays use backlight + overlay only.

### Interface
- Tray icon (state glyphs, tooltip, menu, **mouse wheel** for brightness) and a Direct2D **flyout** with brightness,
  warmth, per-display sliders, scenes and pause.
- **Settings** window: General, Displays (Identify, Night Light warning), Schedule (city search, 24 h timeline with
  preview), Scenes, Hotkeys (capture, conflicts), About (diagnostics).
- Global **hotkeys** (`Win+Alt+…`) and an on-screen display; light/dark theme follows Windows.

### Safety
- Panic hotkey, 10-second confirmation the first time a display goes below 5%, crash recovery (session flag, panic
  hook, exception filter), re-apply after unlock / resume / display changes, gamma watchdog.

### Packaging
- One ~0.7 MB exe with icon, version info and manifest; portable by default; built-in **install / uninstall**
  (Start menu, Apps & features, autostart; no admin); command line for scripting (`--set`, `--scene`, `--pause`, …).
- Measured: ~65 ms to tray, ~2.7 MB working set and ~0% CPU when idle.
