# SLC — Decision log

Product decisions made by the product owner through A/B questions (2026-09-29), plus the technical decisions that
follow from them.

| # | Topic | Decision | Notes |
|---|---|---|---|
| D1 | Platform | **Windows only** — Windows 10 2004+ and Windows 11 | 2004 is the minimum for capture-excluded overlays (`WDA_EXCLUDEFROMCAPTURE`). |
| D2 | Priority | **Maximum performance, minimum size, single `.exe`** | Stressed by the owner; guides every other technical choice. |
| D3 | Language | **Rust** (`windows` crate, raw Win32) | Owner asked for the fastest/tiniest option; Rust matches C in speed and size (< 1 MB static exe) with memory safety. |
| D4 | UI | **Native Win32 + Direct2D**, custom-drawn | No UI framework: instant startup, lowest memory. |
| D5 | Repository | ~~Private~~ → **public** (D20), `montinaops/screen-lighting-control` | |
| D6 | Dimming | **Hybrid pipeline**: hardware (DDC/CI, panel) → gamma → overlay | The main thing that sets SLC apart from Dimmer and f.lux. |
| D7 | App shape | **Tray-first**: flyout for quick control, settings window on demand | |
| D8 | Warmth | **Automatic schedule on by default** (f.lux-style) with manual override | |
| D9 | Location | **Manual city / coordinates**, offline; no Windows location API | Privacy; no permissions. |
| D10 | Distribution | **Portable exe + install mode built into the same exe** | No separate setup.exe, so there's still one file. |
| D11 | Scope | **Lean core first as v1.0**, then roadmap milestones as separate PRs | Chosen by the engineer at the owner's request. |
| D12 | UI language | **English only** | |
| D13 | Hotkeys | **SLC-specific set** (`Win+Alt+…`) | Avoids clashing with f.lux or other apps. |
| D14 | Deepest dim | **~1% with a safety net** (panic hotkey, crash restore, confirm-on-first-use below 5%) | |
| D15 | Workflow | One PR per major feature, self-merged (squash); docs kept in `docs/` | |
| D16 | Darkroom | Implemented with the Magnification API color matrix | No gamma range limit; Windows removes it automatically if SLC dies. |
| D17 | Dependencies | Only the `windows` crate | Size and supply-chain risk. |
| D18 | Smart lights | **Dropped** (Philips Hue / Home Assistant) — owner, 2026-09-30 | Keeps SLC strictly network-free (principle 4). |
| D19 | Light sensor | **Build ambient-light auto-brightness** even though it can't be tested on the owner's hardware — owner, 2026-09-30 | Hidden when no sensor exists; covered by unit tests and the no-sensor path. |
| D20 | License | **Open source, MIT** — owner, 2026-10-06 | Free. The About page and `Cargo.toml` say MIT; city data stays CC BY 4.0 (NOTICE). |
| D21 | Launch distribution | **GitHub Releases + winget**, **unsigned** for now, no landing page — owner, 2026-10-06 | $0. README explains the SmartScreen prompt; signing can come later. |
| D22 | Going public | **Replace the private repo with a new public one** of the same name, history rewritten to the GitHub noreply address; old repo deleted — owner, 2026-10-06 | GitHub's read-only PR refs kept the old author email, and only Support could purge them. Code, tags and releases 1.0.0–1.5.4 carried over unchanged; PR #1–#42 discussions were exported to a local backup and are not on GitHub. `(#N)` in commit subjects before 1.5.4 refers to those old PRs, not to this repo's. |
| D23 | winget package | **`exe` installer running `slc.exe --install`**, not a portable package — decided by Claude, pending review, 2026-10-06 | As a portable package SLC would keep `slc.ini` in winget's package folder, which upgrades replace. `--install` uses the normal install (Programs\SLC, settings in `%APPDATA%`) and its Apps & features entry, so winget sees the version and uninstalls quietly. ID `MONTINA-Ops.ScreenLightingControl`, moniker `slc`. |
