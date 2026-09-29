# Changelog

All notable changes to SLC. Versions follow [SemVer](https://semver.org/).

## [Unreleased]
### Added
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
