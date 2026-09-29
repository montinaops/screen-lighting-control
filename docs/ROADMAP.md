# SLC — Roadmap

Each line is roughly one pull request. Status: ⬜ planned · 🟨 in progress · ✅ merged.

## Milestone v1.0 — core

| # | PR | Scope | Status |
|---|---|---|---|
| 1 | docs: product specification | `PRODUCT.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `DECISIONS.md`, research | ✅ |
| 2 | feat: project skeleton | Cargo setup, size-optimized profile, CI, message loop, single instance, tray icon, logging, `--reset` | ⬜ |
| 3 | feat: monitors + gamma engine | Monitor enumeration, Kelvin math, gamma ramps with range fallback, identity reset | ⬜ |
| 4 | feat: overlay dimming | Capture-excluded click-through overlays, z-order keeping | ⬜ |
| 5 | feat: hardware brightness | DDC/CI + laptop panel IOCTL on a worker thread; hybrid pipeline split | ⬜ |
| 6 | feat: settings + safety net | INI persistence, state file, dirty-flag restore, panic hook, display/power events | ⬜ |
| 7 | feat: hotkeys + OSD | Global hotkeys, OSD popup | ⬜ |
| 8 | feat: schedule | NOAA solar calculations, embedded cities, schedule curve, transitions, overrides | ⬜ |
| 9 | feat: flyout UI | Direct2D flyout with sliders, per-monitor list, scenes, pause | ⬜ |
| 10 | feat: settings window | Pages: General, Displays, Schedule (timeline), Scenes, Hotkeys, About | ⬜ |
| 11 | feat: scenes, darkroom, movie | Scenes, Magnification-based Darkroom, Movie mode | ⬜ |
| 12 | feat: install / uninstall | Self-install, Start menu, Run key, uninstall entry | ⬜ |
| 13 | feat: system integration | Night Light / HDR detection, "Expand color range" elevation | ⬜ |
| 14 | release: v1.0.0 | QA checklist, README, release workflow, tag | ⬜ |

## Milestone v1.1 — automation
- Per-app rules (disable or apply a scene when a given exe is in the foreground; fullscreen detection)
- CLI control of the running instance (`--set`, `--scene`, `--pause`) via `WM_COPYDATA`
- Idle fade (dim after N minutes without input, restore on input)

## Milestone v1.2 — comfort
- Wind-down reminders, 20-20-20 break reminders (toast/OSD)
- Grayscale and amber night modes (color matrix)
- Software cursor option (so the cursor is dimmed too)

## Milestone v1.3 — integrations
- Philips Hue (local bridge API) and Home Assistant following the schedule
- Ambient light sensor (Windows.Devices.Sensors) auto-brightness
