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

CI and releases use the MSVC target on `windows-latest`; that build is the one that counts.

## Checks before merging (enforced by CI)

- fmt, clippy with `-D warnings`, tests, release build.
- Release `slc.exe` ≤ 1 MiB (size budget, PRODUCT §14). Report the size change in the PR when it grows.
- `slc.exe --self-test`, and `scripts/smoke.ps1` on Windows 10 and 11 runners.

## Don't disturb the real install

The user runs SLC day to day. `scripts/smoke.ps1` does an install → uninstall round trip that **uninstalls the real copy**: never run it on this machine. Ask before stopping the running `slc.exe`, replacing the installed copy, or changing display gamma/brightness live. Back up the user's settings before testing against them and restore them afterwards.

## Rules

- Never leave the user in the dark: the panic hotkey, the confirmation below 5%, `--reset`, and crash recovery must keep working. Any change to dimming paths needs a test or live check of the restore path.
- Native Win32 + Direct2D only. No UI framework, no new heavy dependencies; justify any new crate against the size budget.
- Decision logic (schedule, solar math, color, safety) goes in pure functions with unit tests, separate from Win32 calls.
- Product decisions go in `docs/DECISIONS.md` (numbered `D<n>` rows).
- Workflow: `feat/`, `fix/`, `docs/` branches; one PR per roadmap item, merged by you (squash) once CI passes; update `CHANGELOG.md`, `docs/ROADMAP.md`, and `docs/QA.md` in the same PR. Commit subjects look like `feat: computer-break reminders; release 1.5.0 (#33)`.
- Releases: bump `version` in `Cargo.toml`, merge, then push tag `vX.Y.Z` on the merge commit. `release.yml` checks that the version matches the tag, builds, and publishes `slc.exe` plus its `.sha256`.
- Proprietary code: never paste it into public gists, issues, or other services.
