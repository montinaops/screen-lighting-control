# Changelog

All notable changes to SLC. Versions follow [SemVer](https://semver.org/).

## [Unreleased]
### Added
- Automatic schedule: offline NOAA sunrise/sunset, wake-time aware day start, smooth mired-space transitions,
  day/evening/night warmth, optional night brightness ceiling, fixed-times mode, f.lux-style overrides until the next
  phase, "Automatic schedule" / "Return to schedule" in the tray, re-evaluated on clock/time-zone changes.
- Embedded offline city list (1,305 cities incl. all capitals; GeoNames, CC BY 4.0) with accent-insensitive search.
- Global hotkeys (`Win+Alt+…`, rebindable, conflicts reported in the tooltip), with the panic hotkey registered first.
- On-screen display (Direct2D/DirectWrite) for brightness, warmth, scenes and pause, following the light/dark theme.
- Pause (1 hour / until resumed), panic restore, scene cycling; tray Scenes and Pause menus; `--scene`, `--pause`,
  `--resume`, `--exit` for the running instance.
### Fixed
- A stale instance mutex (hung process) no longer blocks startup forever.
- D2D/DWrite factories are never released at process exit (avoids a teardown deadlock).
- Settings persistence (`slc.ini`, atomic writes, saved 1 s after a change) with portable/installed path resolution;
  per-monitor brightness, hardware share, enable flag and original backlight; schedule, scenes and hotkeys model.
- Safety net: session flag with crash recovery (neutral gamma + notice), panic hook and unhandled-exception filter.
- Robustness: re-apply after unlock and resume (re-probing monitors), clean shutdown on sign-out, and a 5 s check that
  re-applies our ramp when another program overwrites it.
- Hardware brightness stage: DDC/CI (VCP 0x10, with retry) for external monitors and the display-brightness IOCTL for
  laptop panels, on a debounced worker thread; SLC adopts each monitor's current backlight on first sight.
- Gamma writes moved to a worker thread; overlay opacity predicted from the learned bound and corrected on completion.
- Per-monitor brightness with a master control that keeps relative offsets; `--set brightness=N monitor=M`.
- `--self-test` reports hardware brightness support and the current level.
- Overlay dimming stage: per-monitor click-through, topmost, layered black windows, **excluded from screen capture**
  (`WDA_EXCLUDEFROMCAPTURE`), re-raised on foreground changes, destroyed at 0% so undimmed monitors cost nothing.
- Pipeline wiring: brightness → gamma (as far as Windows allows) → overlay for the rest; redundant gamma writes skipped.
- Tray "Brightness" submenu; `slc.exe --set brightness=N kelvin=K` controls the running instance (WM_COPYDATA).
- Monitor enumeration with friendly names, stable ids, internal-panel and HDR detection (CCD API).
- Color temperature engine: Kelvin → white point, gamma ramps with a learned Windows range bound and search fallback,
  warmth-over-dimming priority; hybrid pipeline split math.
- Tray "Warmth" submenu, `--reset`, and `--self-test` (monitor report + gamma probe with timings).
- Re-enumerates monitors and re-applies the state after display changes; neutral gamma on exit.
- Project skeleton: size-optimized Rust build, tray icon (procedural, DPI-aware), context menu, single instance,
  in-memory log with `--log`, command-line parsing, CI with size budget.
