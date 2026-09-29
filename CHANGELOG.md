# Changelog

All notable changes to SLC. Versions follow [SemVer](https://semver.org/).

## [Unreleased]
### Added
- Monitor enumeration with friendly names, stable ids, internal-panel and HDR detection (CCD API).
- Color temperature engine: Kelvin → white point, gamma ramps with a learned Windows range bound and search fallback,
  warmth-over-dimming priority; hybrid pipeline split math.
- Tray "Warmth" submenu, `--reset`, and `--self-test` (monitor report + gamma probe with timings).
- Re-enumerates monitors and re-applies the state after display changes; neutral gamma on exit.
- Project skeleton: size-optimized Rust build, tray icon (procedural, DPI-aware), context menu, single instance,
  in-memory log with `--log`, command-line parsing, CI with size budget.
