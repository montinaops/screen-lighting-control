# SLC — Roadmap

Each line is roughly one pull request. Status: ⬜ planned · 🟨 in progress · ✅ merged.

## Milestone v1.0 — core ✅ (released 1.0.0)

| # | PR | Scope | Status |
|---|---|---|---|
| 1 | docs: product specification | `PRODUCT.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `DECISIONS.md`, research | ✅ |
| 2 | feat: project skeleton | Cargo setup, size-optimized profile, CI, message loop, single instance, tray icon, logging, `--reset` | ✅ |
| 3 | feat: monitors + gamma engine | Monitor enumeration, Kelvin math, gamma ramps with range fallback, identity reset | ✅ |
| 4 | feat: overlay dimming | Capture-excluded click-through overlays, z-order keeping | ✅ |
| 5 | feat: hardware brightness | DDC/CI + laptop panel IOCTL on a worker thread; hybrid pipeline split | ✅ |
| 6 | feat: settings + safety net | INI persistence, state file, dirty-flag restore, panic hook, display/power events | ✅ |
| 7 | feat: hotkeys + OSD | Global hotkeys, OSD popup | ✅ |
| 8 | feat: schedule | NOAA solar calculations, embedded cities, schedule curve, transitions, overrides | ✅ |
| 9 | feat: flyout UI | Direct2D flyout with sliders, per-monitor list, scenes, pause | ✅ |
| 10 | feat: settings window | Pages: General, Displays, Schedule (timeline), Scenes, Hotkeys, About | ✅ |
| 11 | feat: scenes, darkroom, movie | Scenes, Magnification-based Darkroom, Movie mode | ✅ |
| 12 | feat: install / uninstall | Self-install, Start menu, Run key, uninstall entry | ✅ |
| 13 | feat: system integration | Night Light / HDR detection, "Expand color range" elevation | ✅ |
| 14 | release: v1.0.0 | QA checklist, README, release workflow, tag | ✅ |

## Milestone v1.1 — automation ✅ (released 1.1.0)
- ✅ Per-app rules (off / no overlay / scene while an app is in front) and pause in fullscreen apps — #15
- ✅ CLI control of the running instance (`--set`, `--scene`, `--pause`, `--resume`, `--settings`, `--exit`) via `WM_COPYDATA` — shipped in v1.0
- ✅ Idle fade (dim after N minutes without input, restore on input; skipped while audio plays or an app is fullscreen) — #16

## Milestone v1.2 — comfort ✅ (released 1.2.0)
- ✅ Bedtime reminder + tooltip countdown, 20-20-20 eye breaks (OSD countdown) — #19
- ✅ Color filters: Grayscale, Amber night, Red night (plus Darkroom) — #18
- ✅ Dimmed mouse pointer (darkened copies of the system cursors while the overlay dims) — #20

## Milestone v1.3 — integrations ✅ (released 1.3.0)
- ~~Philips Hue / Home Assistant~~ — dropped (D18: SLC stays network-free)
- ✅ Ambient light sensor auto-brightness with a learned offset (D19) — #21

## 1.5 — computer breaks
- ✅ Hourly computer-break reminder (configurable interval and 5/10/15-minute length; gentle OSD countdown; wins over
  eye breaks)
- ✅ 1.5.2: the flyout no longer jumps when "Return to schedule" is clicked
- ✅ 1.5.3: a filter scene no longer stays highlighted after it is toggled off

## Launch — open source
- ✅ 1.5.4: MIT License (D20)
- ✅ Public repo with clean history, tags and releases (D22)
- 🟨 winget package `MONTINA-Ops.ScreenLightingControl` (D23) — [microsoft/winget-pkgs#447709](https://github.com/microsoft/winget-pkgs/pull/447709), in Microsoft's review
- ⬜ README: `winget install slc` once the package is accepted
