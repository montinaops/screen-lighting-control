# CLAUDE.md

SLC (Screen Lighting Control) is a tiny Windows tray utility for screen brightness and color temperature. **Performance and size come first**: one static exe, no runtime, no network. Read `docs/PRODUCT.md` (spec), `docs/ARCHITECTURE.md`, and `docs/DECISIONS.md` before designing anything; `docs/DEVELOPMENT.md` has the full toolchain notes.

## Commands (from WSL)

`scripts/cargo.sh` runs the Windows `cargo.exe` with the local `x86_64-pc-windows-gnullvm` + llvm-mingw toolchain. Use it instead of Linux `cargo`, which can't build this crate.

```sh
scripts/cargo.sh fmt --all -- --check
scripts/cargo.sh clippy --all-targets -- -D warnings
scripts/cargo.sh test
scripts/cargo.sh build --release   # -> %USERPROFILE%\.slc-tools\target\x86_64-pc-windows-gnullvm\release\slc.exe
```

`.local-ci` and releases use the MSVC target (`scripts/cargo-msvc.sh`, the Windows cargo with Visual Studio Build Tools); that build is the one that counts.

## Checks before merging (`.local-ci`)

There is no GitHub Actions (its minutes cost money on a private repo, Windows minutes double). `local-ci` (claude-toolkit) runs `.local-ci` on this machine and posts a `local-ci` status to the PR; merge only when it's green on the head commit.

- fmt, clippy with `-D warnings`, tests, release build.
- Release `slc.exe` ≤ 1 MiB (size budget, PRODUCT §14). Report the size change in the PR when it grows.
- `slc.exe --version`, and `scripts/smoke.ps1` in a throwaway Windows Sandbox VM (`win-sandbox`; Windows 10, the host's build). Windows 11 is no longer covered automatically.

## Don't disturb the real install

The user runs SLC day to day. `scripts/smoke.ps1` does an install → uninstall round trip that **uninstalls the real copy**: never run it on this machine, only inside Windows Sandbox (as `.local-ci` does). Ask before stopping the running `slc.exe`, replacing the installed copy, or changing display gamma/brightness live. Back up the user's settings before testing against them and restore them afterwards.

## Rules

- Never leave the user in the dark: the panic hotkey, the confirmation below 5%, `--reset`, and crash recovery must keep working. Any change to dimming paths needs a test or live check of the restore path.
- Native Win32 + Direct2D only. No UI framework, no new heavy dependencies; justify any new crate against the size budget.
- Decision logic (schedule, solar math, color, safety) goes in pure functions with unit tests, separate from Win32 calls.
- Product decisions go in `docs/DECISIONS.md` (numbered `D<n>` rows).
- Workflow: `feat/`, `fix/`, `docs/` branches; one PR per roadmap item, merged by you (squash) once `local-ci` passes; update `CHANGELOG.md`, `docs/ROADMAP.md`, and `docs/QA.md` in the same PR. Commit subjects look like `feat: computer-break reminders; release 1.5.0 (#33)`.
- Releases: bump `version` in `Cargo.toml`, merge, then run `scripts/release.sh` on the updated `main`: it tests, builds with MSVC, checks the exe's version, tags `vX.Y.Z`, and publishes `slc.exe` plus its `.sha256` with the CHANGELOG section.
- Open source (MIT, D20), public repo: never commit secrets, personal paths, or personal email addresses.
