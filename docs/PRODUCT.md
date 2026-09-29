# Screen Lighting Control (SLC) — Product Specification

> Status: **living document**. This is the source of truth for what SLC is and how it behaves.
> Decisions and their reasons are in [`DECISIONS.md`](DECISIONS.md), the technical design is in
> [`ARCHITECTURE.md`](ARCHITECTURE.md), and the delivery plan is in [`ROADMAP.md`](ROADMAP.md).
> Background research: [`research.md`](research.md).

---

## 1. Vision

SLC is a **tiny, extremely fast Windows utility** that controls how much light your screens give off and what
color it is. It combines:

- **Dimmer**'s ability to dim *below* the monitor's hardware minimum, per monitor, and
- **f.lux**'s automatic, time-of-day color temperature,

and improves on both with a **hybrid dimming pipeline**: real backlight first, then gamma, then overlay. Everything
lives in **one portable `.exe`** of a few hundred KB.

### Product principles
1. **Performance first.** Starts instantly, uses about 0% CPU when idle, a few MB of RAM, and no runtime dependencies.
2. **Never trap the user in the dark.** Every dimming path has a way out (panic hotkey, restore after a crash, reset on launch).
3. **Use the best available method.** Prefer real hardware control; fall back to software only when needed.
4. **Private by default.** No network access, no telemetry, no location services. Location is typed in by the user.
5. **Invisible when not needed.** Tray-first; the UI appears only when asked for.

### Target users
- People working at night who find the screen too bright even at minimum brightness.
- People who want f.lux-style warmth without f.lux's range limits, flicker and dated UI.
- Multi-monitor desk setups with mixed laptop panels and external DDC/CI monitors.

### Non-goals (for now)
- macOS / Linux (Windows 10 2004+ and Windows 11 only).
- ICC profile editing or professional color calibration.
- Any cloud features or accounts.

---

## 2. Platform and packaging

| Item | Requirement |
|---|---|
| OS | Windows 10 version 2004 (build 19041) or newer, Windows 11. x64. |
| Artifact | A single `slc.exe`. No DLLs, no runtime, no installer needed. Target size **< 1 MB**. |
| Startup | Tray icon visible in **< 100 ms** from launch on a typical PC. |
| Idle cost | **~0% CPU** (event-driven, no busy loops) and **< 10 MB** working set while idle. |
| Settings | `slc.ini`: next to the exe in portable mode, or in `%APPDATA%\SLC\slc.ini` when installed. |
| Install | Built into the same exe (`slc.exe --install` or the Settings button). See §10. |
| Language | English only. |
| Privileges | Runs as a standard user. Admin only for the optional "Expand color range" registry change. |

---

## 3. Concepts and terminology

| Term | Meaning |
|---|---|
| **Brightness** | What the user sees: a 1–100% value per monitor (100 = full). 1% is nearly black. |
| **Warmth / color temperature** | White point in Kelvin, 1200K (very warm) to 6500K (neutral daylight). |
| **Hardware brightness** | The real backlight level, set through DDC/CI (external monitors) or the display brightness IOCTL (laptop panels). |
| **Gamma** | The GPU's color lookup table (gamma ramp) for each monitor. Used for warmth and software dimming. |
| **Overlay** | A click-through, topmost, semi-transparent black window per monitor, hidden from screen capture. Used for the deepest dimming. |
| **Master** | A control that moves every monitor together while keeping each one's relative offset. |
| **Scene** | A named preset of brightness plus warmth (plus optional effects), e.g. "Reading". |
| **Schedule** | The automatic day/night curve of warmth (and optionally brightness). |
| **Pause** | Temporarily turn SLC's effects off (identity gamma, no overlay, hardware brightness left alone). |
| **Panic restore** | Immediately undo all effects: full brightness, neutral color, pause for 1 hour. |

---

## 4. Hybrid brightness pipeline (core feature)

One brightness value **B** (1–100%) per monitor is split across three stages, always in this order:

1. **Hardware stage.** If the monitor supports hardware brightness (DDC/CI VCP 0x10, or the laptop panel IOCTL), the
   top part of the slider range moves the real backlight from its maximum down to its minimum.
2. **Gamma stage.** Below the hardware minimum, the gamma ramp scales output down, until the largest reduction
   Windows allows for that monitor (found automatically, see §5.3).
3. **Overlay stage.** Whatever dimming is still needed comes from the black overlay's opacity, down to 1% brightness.

### 4.1 Mapping
- Let `H` be the **hardware share** (default 50%; configurable 0–90%; forced to 0 when there is no hardware control).
- For `B ≥ 100 − H` (the top of the slider): hardware = `(B − (100 − H)) / H` of its range; software factor = 1.0.
- For `B < 100 − H`: hardware = minimum; software factor `S = B / (100 − H)` (1.0 down to ≈0.01).
- The software factor `S` is applied as gamma scale `g = max(S, gamma_floor)` and overlay opacity
  `α = 1 − S / g` (the overlay removes whatever gamma could not).
- All software factors work on **gamma-encoded** values (what the GPU lookup table maps and what DWM blends
  in). Encoded values are close to perceptually uniform, so equal slider steps look equally big.

### 4.2 Hardware behavior
- DDC/CI writes are **slow (50–200 ms) and wear the monitor's settings memory**. So:
  - they run on a dedicated worker thread and never block the UI;
  - they are **debounced** (latest value wins, written 250 ms after the last change);
  - schedule-driven changes are written at most **once per 60 s** per monitor, and only if the value changed.
- The original hardware brightness is read and remembered at first run; "Reset" and uninstall restore it.
- **Starting SLC never changes the screen**: the first time a monitor is seen, SLC adopts its current backlight level
  as the brightness value (`B = knee + current × H`).
- External monitors use the low-level VCP 0x10 calls (`Get/SetVCPFeature`), which more monitors support than the
  high-level MCCS brightness API. Each call is retried once, since some monitors drop the first command after idling.
  Measured probe time: ~140 ms for 2 monitors.
- Monitors that don't answer DDC/CI are marked *software-only* and never retried until the display configuration changes.

### 4.3 Overlay behavior
- One overlay per monitor: `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`.
- **Hidden from screenshots and screen recordings** via `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)`.
- Never takes focus and never intercepts the mouse.
- Stays on top: re-asserted when the foreground window changes (WinEvent hook) and when the taskbar is recreated.
- The SLC flyout and settings window are **always above the overlay**, so the controls stay visible.
- The overlay is destroyed (not just hidden) when opacity is 0, so there is no compositor cost.

---

## 5. Color temperature (warmth)

### 5.1 Model
- White point from Kelvin using a blackbody approximation, normalized so 6500K = (1, 1, 1).
- Range 1200K–6500K. Presets match f.lux naming so users feel at home:
  Ember 1200K · Candle 1900K · Warm Incandescent 2300K · Incandescent 2700K · Halogen 3400K ·
  Fluorescent 4200K · Daylight 6500K.

### 5.2 Gamma ramp composition
For each monitor and channel `c ∈ {R,G,B}` and index `i ∈ 0..255`:

```
ramp[c][i] = 65535 · clamp( (i/255) · white[c] · g )
```
where `g` is the gamma dimming factor from §4.1. A single ramp carries both warmth and software dimming.

### 5.3 Windows range limit and automatic fallback
Windows rejects gamma ramps that differ "too much" from identity unless the registry value
`HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\ICM\GdiICMGammaRange = 256` is set.
- Measured on Windows 10 (2 × Lenovo T2254pC): a ramp entry may deviate from identity by at most **~50% of full
  scale**. So without the registry change, 2700K gets only ~77% of its warmth and 1200K ~50%, and gamma can't dim
  while warmth uses the whole budget.
- SLC **learns each monitor's deviation bound**, starting from 0.5. It first tries the full ramp (1 call), then the
  strongest ramp predicted to fit (1 more call). Only if that fails does it binary-search, blending toward identity
  (~8 calls), and re-learn the bound. Each `SetDeviceGammaRamp` can block up to one vsync (~16.7 ms).
- If warmth is being limited, the UI shows a hint: **"Expand color range"**. It runs `slc.exe --expand-range` elevated
  (UAC), sets the registry value, and asks the user to sign out or reboot.
- Priority: warmth is applied first; dimming that gamma can't do goes to the overlay (§4.1).

### 5.4 Special color modes
| Mode | Behavior |
|---|---|
| **Darkroom** | Red only, inverted (like f.lux). Uses the Windows Magnification API's full-screen color matrix (`MagSetFullscreenColorEffect`), which has no range limit and is **removed automatically by Windows if SLC exits or crashes**. |
| **Movie** | 2.5 hours at a moderate warmth (3400K) that keeps skin tones, then returns to the schedule. |
| **Grayscale** | Luminance only (no color at all). |
| **Amber night** | Luminance tinted (1, 0.62, 0.12): very little blue light, without the orange cast of 1200K. |
| **Red night** | Luminance in red only (not inverted). |

---

## 6. Schedule (automatic warmth)

- **On by default.** Uses the user's **city or coordinates** (offline list of about 1,000 major cities built into
  the exe, plus free lat/lon entry). No network access, no Windows location service.
- Sunrise and sunset are computed locally (NOAA solar position algorithm).
- Default curve:
  - **Day** (sunrise+30 min to sunset): Daylight 6500K.
  - **Evening** (sunset to bedtime−1 h): Halogen 3400K.
  - **Night** (bedtime−1 h to sunrise): Incandescent 2700K.
  - Transitions: **smooth**, default 40 minutes around sunset and 60 minutes around sunrise, updated at most every 30 s
    (and only when the change is visible, ≥ 25K).
- Optional **brightness schedule**: night brightness value per monitor (off by default).
- Alternative mode: **fixed times** instead of sun-based.
- A **wake time** (default 07:00) sets bedtime (wake − 8 h) and pulls the sunrise transition earlier if needed.
- Manual changes act as an **override** until the next schedule phase change (like f.lux); the tray menu offers
  "Return to schedule".
- **Curve model**: three overlapping weights (day, evening, night) with smoothstep ramps. Day is centered on day start
  and sunset; night ramps in over 30 min before `bedtime − 1 h` and lasts until daylight has fully arrived. Colors are
  interpolated in **mired** space (1e6/K), so equal steps look equally different. Tested: no minute-to-minute jump
  above 250K over a whole day. Polar day/night are handled (always day / always night).
- Without a location, sun mode falls back to the fixed times until a city is chosen; the settings window asks for one
  on first run.
- **Preview**: in Settings, scrub a 24-hour timeline to see and apply any time's look for 5 seconds.

---

## 7. Scenes (presets)

- Built-in scenes: **Daylight**, **Reading** (80%, 4200K), **Evening** (60%, 3400K), **Night** (35%, 2700K),
  **Movie** (§5.4), **Darkroom** (§5.4).
- Users can save the current state as a scene, rename it, delete it and give it a hotkey.
- A scene applies to all monitors, or per monitor when "per-monitor" is enabled for that scene.
- Scenes are stored in `slc.ini`.

---

## 8. User interface

### 8.1 Tray icon
- Always present while running. The icon shows state (normal / paused / darkroom).
- **Left-click**: opens the flyout. **Right-click**: context menu. **Mouse wheel over the icon**: master brightness ±2%.
- The tooltip shows the current state, e.g. `SLC — 45% · 2700K · Night`.

### 8.2 Flyout (quick controls)
A small borderless panel above the tray, drawn with Direct2D, following the system light/dark theme and accent color:
- **Master brightness** slider (100 = bright, left = dark) with a numeric %.
- **Warmth** slider (Kelvin, labeled with the preset name).
- **Per-monitor** section (expandable): one brightness slider per monitor with its name
  (e.g. "DELL U2720Q", "Built-in display") and an icon for the method in use (hardware / software).
- Scene chips (one click to apply).
- Buttons: **Pause** (menu: 1 hour / until sunrise / indefinitely), **Settings**.
- Closes when it loses focus or on Esc.

### 8.3 Context menu
Pause/Resume · Disable for 1 hour · Scenes ▸ · Darkroom · Movie mode · Reset everything · Settings… · Exit.

### 8.4 Settings window
A single resizable window with pages on the left:
- **General**: start with Windows, portable/installed status and Install/Uninstall button, theme.
- **Displays**: list of monitors with detected capabilities (DDC/CI, panel, gamma range limited?), per-monitor
  hardware share, include/exclude each monitor, "Identify" (flashes the monitor number), "Expand color range".
- **Schedule**: location (city search / lat-lon), wake time, day/evening/night temperatures, transition lengths,
  brightness schedule, a 24 h timeline preview graph with a draggable scrubber.
- **Scenes**: list, edit, hotkey assignment.
- **Hotkeys**: all bindings, rebindable; conflicts shown in red.
- **Rules** (roadmap v1.1): per-app rules.
- **About**: version, links, diagnostics copy button.

---

## 9. Hotkeys (global, rebindable)

| Default | Action |
|---|---|
| `Win+Alt+Up` / `Win+Alt+Down` | Master brightness +5% / −5% (auto-repeat) |
| `Win+Alt+Left` / `Win+Alt+Right` | Warmer (−250K) / cooler (+250K) |
| `Win+Alt+Home` | Pause / resume |
| `Win+Alt+End` | **Panic restore**: 100% brightness, 6500K, pause for 1 hour |
| `Win+Alt+Insert` | Darkroom on/off |
| `Win+Alt+PgUp` / `Win+Alt+PgDn` | Next / previous scene |

If a hotkey is already taken by another app, it's reported in Settings and in the tray tooltip, and SLC keeps working.
A small **on-screen display (OSD)** shows the new value for 1.2 s after a hotkey change (optional, on by default).

---

## 10. Install / portable modes

- **Portable (default)**: run `slc.exe` from anywhere. If `slc.ini` sits next to it (or the folder is writable and
  no installed copy exists), settings are stored there.
- **Install** (`slc.exe --install` or the Settings button, no admin needed): copies the exe to
  `%LOCALAPPDATA%\Programs\SLC\slc.exe`, creates a Start Menu shortcut, registers an uninstall entry under
  `HKCU\...\Uninstall\SLC`, enables *Start with Windows* (`HKCU\...\Run`), and moves settings to `%APPDATA%\SLC`.
- **Uninstall** (`slc.exe --uninstall`, Settings, or Windows "Apps"): restores the hardware brightness and gamma,
  removes the Run entry, shortcut, uninstall key and program files, and optionally the settings.
- **Single instance**: a second launch brings the first instance's flyout forward (named mutex + window message).

## 11. Command line

```
slc.exe                      start (or focus the running instance)
slc.exe --reset              restore neutral gamma, 100% brightness, remove overlays, then exit
slc.exe --install / --uninstall
slc.exe --expand-range       (elevated) set GdiICMGammaRange=256
slc.exe --set brightness=40 [monitor=2]     (roadmap v1.1: forwarded to the running instance)
slc.exe --scene Night                        (roadmap v1.1)
slc.exe --pause 60                           (roadmap v1.1)
```

---

## 12. Safety net (never trapped in the dark)

1. **Panic hotkey** (`Win+Alt+End`), registered first, before anything else.
2. **Crash restore**: a panic hook and an unhandled-exception filter restore identity gamma before the process ends.
   Overlays and Darkroom disappear on their own when the process dies.
3. **Dirty flag**: SLC writes `session=running` to its state file at start and `session=clean` on exit. If it starts
   and finds a dirty session, it resets gamma first and shows a notice.
4. **Floor**: brightness below 5% asks "Keep this setting?" with a 10-second revert countdown the first time it's
   used on each monitor.
5. **Session end / sign-out / display change**: effects are re-applied or cleaned up correctly (`WM_ENDSESSION`,
   `WM_DISPLAYCHANGE`, `WM_POWERBROADCAST`, session lock/unlock).
6. **`slc.exe --reset`** always works, even if the settings are corrupt.

---

## 13. Robustness and events

| Event | Reaction |
|---|---|
| Monitor plugged / unplugged / resolution or DPI changed | Re-enumerate monitors, rebuild overlays, re-apply state (debounced 500 ms). |
| Resume from sleep, unlock | Re-apply gamma (Windows/drivers often reset it) and re-verify overlays. |
| Another app resets gamma (e.g. game, driver) | Periodic cheap check every 5 s of one ramp value; re-apply if changed. |
| Windows Night Light is on | Detected; a warning offers to open Night Light settings (both fighting causes flicker). |
| HDR enabled on a monitor | Gamma doesn't apply reliably; that monitor uses the overlay + hardware only; UI shows "HDR". |
| Fullscreen exclusive game | Overlay may not show; hardware + gamma still apply. |

---

## 13a. Per-app rules (v1.1)

- Rules match the foreground window's executable name (e.g. `photoshop.exe`), stored in `[rules]` as
  `exe=disable | no_overlay | scene:<name>`.
- **Off**: neutral software effects while the app is in front (hardware untouched).
  **No overlay**: backlight + gamma only (for anti-cheat and capture tools). **Scene**: that scene's brightness/warmth.
- **Pause in fullscreen apps**: effects off while the foreground window covers its whole monitor (shell windows
  excluded); re-checked every 5 s.
- The foreground `WinEvent` hook is installed only while rules exist, fullscreen pausing is on, or Settings is open
  (to list recent apps). SLC's own windows never change the active rule.

## 13b. Dim when idle (v1.1)

- Off by default. After `idle_minutes` (1–30) without keyboard/mouse input, brightness fades over 2 s to at most
  `idle_level` (a ceiling, like the night brightness), and returns instantly on the next input.
- Skipped while sound is playing on the default output (videos, music), while a fullscreen app is in front, or while
  paused. Windows has no public API to read other programs' "keep display on" requests without admin rights, so
  audio is the proxy.
- Polls `GetLastInputInfo` every 5 s (every 100–250 ms only while fading/dimmed).

## 14. Performance budget

| Metric | Budget |
|---|---|
| `slc.exe` size | < 1 MB (release, stripped, LTO, `opt-level="z"`, static CRT) |
| Cold start to tray icon | < 100 ms |
| Idle CPU | ~0% (no polling except the 5 s gamma check and the 30 s schedule tick) |
| Memory (idle, flyout closed) | < 10 MB working set |
| Slider → visible change (gamma/overlay) | < 16 ms |
| **Measured (v1.0.0)** | 716 KB exe · 65 ms to tray · 2.7 MB working set idle (trimmed after startup) · ~0% CPU |
| Threads | UI thread + hardware worker (DDC/CI, panel) + gamma worker (`SetDeviceGammaRamp` blocks up to a vsync) |

---

## 15. Roadmap summary

- **v1.0 (core)**: everything in §4–§12 except items marked "roadmap".
- **v1.1**: per-app rules (disable/scene per exe, fullscreen detection), CLI control of the running instance, idle fade.
- **v1.2**: wind-down reminders + 20-20-20 breaks, grayscale/amber night modes, software cursor option.
- **v1.3**: smart lights (Philips Hue local API, Home Assistant) following the schedule; ambient light sensor.

Details and status: [`ROADMAP.md`](ROADMAP.md).
