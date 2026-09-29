# SLC — Decision log

Product decisions made by the product owner through A/B questions (2026-09-29), plus the technical decisions that
follow from them.

| # | Topic | Decision | Notes |
|---|---|---|---|
| D1 | Platform | **Windows only** — Windows 10 2004+ and Windows 11 | 2004 is the minimum for capture-excluded overlays (`WDA_EXCLUDEFROMCAPTURE`). |
| D2 | Priority | **Maximum performance, minimum size, single `.exe`** | Stressed by the owner; guides every other technical choice. |
| D3 | Language | **Rust** (`windows` crate, raw Win32) | Owner asked for the fastest/tiniest option; Rust matches C in speed and size (< 1 MB static exe) with memory safety. |
| D4 | UI | **Native Win32 + Direct2D**, custom-drawn | No UI framework: instant startup, lowest memory. |
| D5 | Repository | **Private**, `MONTINA-Ops/screen-lighting-control` | |
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
