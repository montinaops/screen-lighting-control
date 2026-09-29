# SLC — Development guide

## Toolchains

| Where | Target | Linker / tools |
|---|---|---|
| CI and releases (GitHub Actions, `windows-latest`) | `x86_64-pc-windows-msvc` | MSVC `link.exe` (static CRT via `.cargo/config.toml`) |
| Local dev without Visual Studio | `x86_64-pc-windows-gnullvm` | [llvm-mingw](https://github.com/mstorsjo/llvm-mingw) (`clang` as the linker, `llvm-dlltool` for raw-dylib imports) |

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

## Branching and PRs
- `feat/<name>`, `fix/<name>`, `docs/<name>`; one PR per roadmap item (see `ROADMAP.md`); squash-merge after CI passes.
- Update `ROADMAP.md` status and `CHANGELOG.md` in the same PR.
