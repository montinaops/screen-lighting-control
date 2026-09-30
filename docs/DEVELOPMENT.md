# SLC — Development guide

## Toolchains

| Where | Target | Linker / tools |
|---|---|---|
| CI and releases (GitHub Actions, `windows-latest`) | `x86_64-pc-windows-msvc` | MSVC `link.exe` (static CRT via `.cargo/config.toml`) |
| Local dev without Visual Studio | `x86_64-pc-windows-gnullvm` | [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (`clang` as the linker, `llvm-dlltool` for raw-dylib imports) |

`build.rs` embeds the icon, manifest and version info with `embed-resource`: MSVC uses `rc.exe` from the Windows SDK;
the GNU-flavored local build needs `windres` + `clang` on PATH — `scripts/cargo.sh` exposes a minimal
`%USERPROFILE%\.slc-tools\rcbin` (llvm-mingw's `x86_64-w64-mingw32-windres.exe` renamed to `windres.exe`, plus
`clang.exe`, `clang-23.exe` and the LLVM DLLs). Putting all of llvm-mingw on PATH breaks host build scripts.

The `windows` crate links Win32 APIs with `raw-dylib`, so GNU-flavored targets need a `dlltool`. The classic
`x86_64-pc-windows-gnu` target combined with `llvm-dlltool` produces broken binaries, so use `gnullvm` with llvm-mingw.

### One-time setup (Windows, no admin needed)
1. Install Rust: `rustup-init.exe -y --default-host x86_64-pc-windows-gnu --profile minimal`
2. `rustup component add rustfmt clippy` and `rustup target add x86_64-pc-windows-gnullvm`
3. Extract the llvm-mingw `ucrt-x86_64` release zip to `%USERPROFILE%\.slc-tools\llvm-mingw`.

(With Visual Studio Build Tools installed you can just run `cargo build --release` for the MSVC target.)

### Building from WSL
`scripts/cargo.sh` runs the Windows `cargo.exe` with the right environment:

```bash
scripts/cargo.sh build --release   # -> %USERPROFILE%\.slc-tools\target\x86_64-pc-windows-gnullvm\release\slc.exe
scripts/cargo.sh test
scripts/cargo.sh clippy --all-targets -- -D warnings
scripts/cargo.sh fmt --all
```

### Building on Windows
`scripts/build.ps1` (MSVC toolchain) builds a release and prints the exe size.

## Checks required before merging (enforced by CI)
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- Release `slc.exe` ≤ 1 MiB (size budget from PRODUCT §14)

## Smoke test (CI)
`scripts/smoke.ps1 -Exe <slc.exe>` runs end to end on a clean machine: CLI, self-test, tray app start → commands →
clean exit, `--reset`, and an install → uninstall round trip that must leave **nothing** behind (folders, shortcut,
registry, autostart, process). CI runs it on `windows-2022` (Windows 10-based) and `windows-2025` (Windows 11 24H2-based).
Don't run it on a machine where SLC is installed for real: the round trip uninstalls it.

## Branching and PRs
- `feat/<name>`, `fix/<name>`, `docs/<name>`; one PR per roadmap item (see `ROADMAP.md`); squash-merge after CI passes.
- Update `ROADMAP.md` status and `CHANGELOG.md` in the same PR.
