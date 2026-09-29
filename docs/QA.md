# SLC — Release QA checklist

Run on a real Windows machine before tagging a release. ✅ = verified for v1.0.0 on Windows 10 22H2
(2 × Lenovo T2254pC over DDC/CI, 1680×1050, UTC+2); ⚠️ = not verifiable on that machine.

## Build and budget
- ✅ CI green (fmt, clippy `-D warnings`, tests, MSVC release build, size budget)
- ✅ Exe ≤ 1 MiB (716 KB gnullvm build; CI checks the MSVC build)
- ✅ Startup to tray < 100 ms (65 ms measured, from settings load to controller window)
- ✅ Idle: working set 2.7 MB after trim (budget 10 MB), private 2.5 MB, ~0% CPU over 30 s

## Dimming pipeline
- ✅ Hardware probe finds DDC/CI displays (~130 ms); `--self-test` reports them
- ✅ Starting SLC does not change the backlight (adopts the current level)
- ✅ Per-display brightness moves only that display's backlight (read back with `--self-test`)
- ✅ Below the backlight share, gamma then overlay dim further; overlay is **not captured** in screenshots
  (BitBlt with CAPTUREBLT: same mean at 100% and 20%)
- ✅ Gamma watchdog re-applies when another program resets the ramp (≤ 5 s)
- ⚠️ Laptop panel (IOCTL) path: no laptop available
- ⚠️ HDR display path: no HDR display available

## Color
- ✅ Warmth presets apply; Windows range limit is detected and learned (2700K → 77% without the registry change)
- ✅ Darkroom matrix active system-wide; **removed by Windows after a hard kill**
- ✅ Movie mode starts (3400K, 2.5 h)
- ⚠️ Expand color range (elevated) — not run on the owner's machine on purpose (machine-wide registry value)

## Schedule
- ✅ Sun times match NOAA within 3 min (unit tests); whole-day curve has no jumps > 250K/min
- ✅ City search ("sao pau" → São Paulo) sets location; timeline preview renders
- ✅ Manual warmth becomes an override until the next phase; "Return to schedule" works

## UI
- ✅ Tray icon, tooltip, context menu, paused/darkroom glyphs
- ✅ Flyout opens above the tray, updates live, closes on focus loss; deep-dim banner counts down and reverts to 5%
- ✅ Settings: all six pages render; wrapped descriptions; Identify shows numbers
- ✅ OSD for brightness / warmth / scenes / pause
- ✅ Hotkeys fire (simulated key presses); 0 conflicts on the test machine

## Safety
- ✅ Crash (`taskkill /F`) → next start resets gamma and shows a notice
- ✅ Panic hotkey restores full brightness and pauses
- ✅ `--exit` closes cleanly and restores neutral gamma

## Install
- ✅ `--install`: exe, migrated settings, Start menu shortcut, Apps & features entry, autostart; installed copy runs
- ✅ `--uninstall --quiet`: stops the app, restores backlight, removes folder, shortcut, entries and autostart
