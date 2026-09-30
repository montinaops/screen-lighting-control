# Screen Lighting Control (SLC)

A tiny, extremely fast Windows utility that controls **how bright** your screens are and **what color** their light is.
SLC combines the ideas of [Dimmer](https://www.nelsonpires.com/software/dimmer) (dimming below the monitor's minimum)
and [f.lux](https://justgetflux.com/) (automatic warm light at night), and improves on both.

- **Hybrid dimming**: one slider per display moves the real backlight first (DDC/CI or laptop panel), then gamma, and
  only then a black overlay. That saves power and keeps contrast, and still goes down to ~1% when you need it.
- **Automatic warmth** that follows the sun for your city (offline, no location services) or fixed times, with smooth
  transitions and f.lux-style temporary overrides.
- **Per-display control**, scenes, Darkroom (red-only), Movie mode, global hotkeys, an on-screen display.
- **Never trapped in the dark**: panic hotkey, confirmation below 5%, crash recovery. The overlay is hidden from
  screenshots and screen sharing.
- **One ~0.7 MB exe**, no runtime, no admin rights, no network, no telemetry. Starts in ~65 ms and uses ~3 MB of RAM
  and ~0% CPU when idle.

## Install

Download `slc.exe` from the [latest release](../../releases/latest) and run it. SLC starts in the tray (portable mode:
settings are saved next to the exe). To install it for your user (Start menu, autostart, Apps & features entry), open
**Settings › General › Install**, or run:

```
slc.exe --install
```

Uninstall from Windows **Apps & features**, from Settings, or with `slc.exe --uninstall`. Uninstalling removes
everything SLC created — program, settings, shortcut, autostart and registry entries — restores each display's original
backlight, and undoes the Windows color-range setting if SLC turned it on (Windows asks for permission once).

Requirements: Windows 10 version 2004 or newer, or Windows 11 (x64).

## Using SLC

- **Left-click the tray icon** for the quick panel: brightness, warmth, per-display sliders, scenes, pause.
- **Scroll over the tray icon** to change brightness.
- **Right-click** for the menu (scenes, Darkroom, Movie mode, automatic schedule, pause, reset).
- **Settings** has pages for displays, schedule (pick your city), scenes, hotkeys and more.

### Default hotkeys

| Hotkey | Action |
|---|---|
| `Win+Alt+Up` / `Win+Alt+Down` | Brightness +5% / −5% |
| `Win+Alt+Left` / `Win+Alt+Right` | Warmer / cooler |
| `Win+Alt+Home` | Pause / resume |
| `Win+Alt+End` | **Panic restore**: full brightness, neutral color, pause 1 hour |
| `Win+Alt+Insert` | Darkroom on/off |
| `Win+Alt+PgUp` / `Win+Alt+PgDn` | Next / previous scene |

All hotkeys can be changed in Settings › Hotkeys.

### Command line

```
slc.exe --set brightness=40 [monitor=2] [kelvin=3400]
slc.exe --scene Night | --pause 60 | --resume | --settings [page] | --exit
slc.exe --reset            restore neutral colors and full brightness
slc.exe --self-test        report displays, capabilities and schedule
slc.exe --expand-range     (admin) allow warmer colors / deeper gamma dimming
```

### Good to know

- Windows limits how far gamma can change colors (about 2700K). On first start SLC offers to lift this limit: Windows
  asks for administrator permission **once**, and the change takes effect after you sign out and back in. You can also
  do it later in Settings › Displays › **Expand…**. Darkroom is not affected by this limit.
- Turn off **Windows Night Light** while using SLC; both change the screen colors and would fight.
- HDR displays use backlight + overlay only.

## Documentation

- [`docs/PRODUCT.md`](docs/PRODUCT.md) — product specification
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — technical design
- [`docs/DECISIONS.md`](docs/DECISIONS.md) — decision log
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — milestones
- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — building and contributing
- [`docs/QA.md`](docs/QA.md) — release checklist
- [`docs/research.md`](docs/research.md) — research on Dimmer and f.lux
- [`CHANGELOG.md`](CHANGELOG.md), [`NOTICE.md`](NOTICE.md)

© MONTINA-Ops. All rights reserved.
