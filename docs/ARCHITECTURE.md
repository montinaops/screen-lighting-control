# SLC — Architecture

## 1. Tech stack

| Layer | Choice | Why |
|---|---|---|
| Language | **Rust** (stable, edition 2021) | Speed on par with C, a small static binary, and memory safety without a GC. |
| OS bindings | **`windows`** crate (Microsoft) | Official Win32 + COM projections (Direct2D, DirectWrite, Magnification, DXVA2). Only the features used are compiled in. |
| UI | Raw **Win32** windows + **Direct2D / DirectWrite** drawing | No UI framework; custom-drawn controls; GPU-accelerated; instant start. |
| Build | `cargo`, `opt-level="z"`, `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`, static CRT | Smallest and fastest binary. |
| Targets | `x86_64-pc-windows-msvc` (CI/release), `x86_64-pc-windows-gnu` (local dev) | Both produce one standalone exe. |
| Crates allowed | `windows` only (plus dev-only test crates) | Keep size and supply-chain risk down. Everything else is written in-house. |

## 2. Process and threads

```
slc.exe (GUI subsystem, per-monitor-DPI-aware v2)
├── UI thread ─ message loop
│    ├── hidden "controller" window (tray icon, hotkeys, timers, WM_DISPLAYCHANGE, power, session events)
│    ├── overlay windows (one per dimmed monitor)
│    ├── flyout window (Direct2D)
│    ├── settings window (Direct2D)
│    └── OSD window (Direct2D)
├── hardware worker thread ─ DDC/CI + panel IOCTL (slow, blocking I/O), debounced 250 ms
└── gamma worker thread ─ SetDeviceGammaRamp (can block up to one vsync per call)
```

Both workers are fed by a latest-value-wins `worker::Mailbox` and report back through `engine::Events`
(a queue plus a `WM_APP_ENGINE` post). The UI thread **predicts** the gamma result with `gamma::plan` and the learned
bound, so the overlay is right at once, then corrects it if the worker reports something different.
Worker results carry a monitor-list generation; stale results are ignored. Everything else runs on the UI thread. No async runtime. Timers use `SetTimer` (coalesced).

## 3. Module layout (`src/`)

| Module | Responsibility |
|---|---|
| `main.rs` | Entry point, CLI parsing, single instance, panic hook, message loop. |
| `app.rs` | `App` state machine: owns the model, reacts to events, calls `engine::apply`. |
| `model.rs` | Plain data: `Settings`, `MonitorState`, `Scene`, `Schedule`; defaults. |
| `config.rs` | Tiny INI reader/writer (no dependencies), portable vs installed paths. |
| `monitors.rs` | Enumerate monitors (`EnumDisplayMonitors`, `QueryDisplayConfig` for friendly names), stable IDs. |
| `engine/mod.rs` | The hybrid pipeline: splits brightness B into hardware / gamma / overlay per monitor (§4 of PRODUCT). |
| `engine/gamma.rs` | Ramp building (Kelvin × dim), `SetDeviceGammaRamp`, strength binary search, identity reset. |
| `engine/overlay.rs` | Layered, click-through, capture-excluded overlay windows. |
| `engine/hardware.rs` | DDC/CI via `dxva2` (`GetPhysicalMonitorsFromHMONITOR`, `Get/SetMonitorBrightness`), laptop panel via `IOCTL_VIDEO_*_DISPLAY_BRIGHTNESS`; worker thread + debounce. |
| `engine/magnify.rs` | Full-screen color matrix (Darkroom) via `Magnification.dll`. |
| `color.rs` | Kelvin → RGB white point, perceptual curves. |
| `solar.rs` | NOAA sunrise/sunset computation. |
| `schedule.rs` | Computes target (Kelvin, brightness) for a timestamp; transitions; overrides. |
| `cities.rs` | Embedded compact city table + search. |
| `hotkeys.rs` | `RegisterHotKey` table, parsing/formatting of bindings. |
| `tray.rs` | `Shell_NotifyIconW`, context menu, icon rendering. |
| `ui/` | `d2d.rs` (factory, render-target helpers, theme), `widgets.rs` (slider, button, chip, toggle, list), `flyout.rs`, `settings.rs`, `osd.rs`. |
| `install.rs` | Self-install/uninstall, Run key, shortcut (`IShellLinkW`), uninstall entry. |
| `safety.rs` | Dirty flag, crash restore, reset. |
| `system.rs` | Night Light / HDR detection, OS version checks, elevated relaunch. |

## 4. Data flow

```
input (hotkey / slider / schedule tick / scene)
      │
      ▼
App::set_target(monitor|master, brightness?, kelvin?)   → updates model, marks dirty
      │
      ▼
engine::apply(&model, &monitors)
  ├─ for each monitor: split(B, hw_share) → (hw_level, gamma_scale, overlay_alpha)
  ├─ gamma::apply(monitor, kelvin, gamma_scale)   — synchronous, < 1 ms
  ├─ overlay::apply(monitor, overlay_alpha)        — synchronous, SetLayeredWindowAttributes
  └─ hardware::request(monitor, hw_level)          — async, debounced on the worker
```

`apply` compares with the last applied values and skips unchanged stages, so repeated calls are almost free.

## 5. Settings file (`slc.ini`)

Plain INI, UTF-8, written atomically (temp file + `MoveFileExW(REPLACE_EXISTING)`), saved 1 s after the last change.

```ini
[general]
autostart=1
osd=1
theme=system

[master]
brightness=100
kelvin=6500

[monitor.<stable-id>]
name=DELL U2720Q
enabled=1
brightness=100
hw_share=50

[schedule]
enabled=1
mode=sun            ; sun | fixed
lat=-23.55
lon=-46.63
city=São Paulo
wake=07:00
day_k=6500
evening_k=3400
night_k=2700
sunset_minutes=40
sunrise_minutes=60

[scene.Reading]
brightness=80
kelvin=4200

[hotkeys]
brightness_up=Win+Alt+Up
...
```

Runtime state (dirty flag, original hardware brightness) goes in a separate `state.ini`, so user settings stay clean.

Stable monitor ID: the monitor device path from `QueryDisplayConfig` (`monitorDevicePath`), which is stable across
reboots and port changes for the same physical monitor.

## 6. Build and release

- `scripts/build.ps1` / `scripts/build.sh`: release build, prints the exe size.
- **GitHub Actions** (`.github/workflows/ci.yml`): on PR and push, `windows-latest`: `cargo fmt --check`,
  `cargo clippy -D warnings`, `cargo test`, release build, uploads `slc.exe`; fails if the exe is > 1 MB.
- **Release** (`release.yml`): on tag `v*`, tests, builds, checks the exe version matches the tag, and publishes
  `slc.exe` + SHA-256 with the CHANGELOG section as release notes.

## 7. Testing strategy

- **Unit tests** (pure logic, run anywhere): color math, pipeline split, solar calculations against NOAA reference
  values, schedule curve, INI round-trip, hotkey parsing, city search.
- **Smoke test** (Windows CI): `slc.exe --self-test` enumerates monitors, builds ramps, and creates/destroys an overlay
  without showing anything, exiting with 0/1.
- **Manual QA checklist**: `docs/QA.md`.

## 8. Coding conventions

- `unsafe` only in thin wrappers inside the Win32 modules; the rest is safe Rust.
- No panics in normal paths: Win32 failures are logged and degrade gracefully.
- Logging: a tiny ring buffer (last 200 lines) shown in About → "Copy diagnostics"; `--log` mirrors it to a file.
- Branches: `feat/<name>`, `fix/<name>`, `docs/<name>`; one PR per major feature; squash-merge.
