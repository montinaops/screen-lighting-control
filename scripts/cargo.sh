#!/usr/bin/env bash
# Run the Windows-host cargo from WSL against this repository.
# Local builds use the x86_64-pc-windows-gnullvm target with llvm-mingw (no Visual Studio needed);
# CI and releases use x86_64-pc-windows-msvc.
# Usage: scripts/cargo.sh build --release | test | clippy ...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WIN_HOME="$(wslpath "$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')")"
WIN_HOME_W="$(wslpath -w "$WIN_HOME")"
CARGO="$WIN_HOME/.cargo/bin/cargo.exe"
LLVM_MINGW="$WIN_HOME_W\\.slc-tools\\llvm-mingw\\bin"
export CARGO_TARGET_DIR="$WIN_HOME_W\\.slc-tools\\target"
export CARGO_BUILD_TARGET="x86_64-pc-windows-gnullvm"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER="$LLVM_MINGW\\x86_64-w64-mingw32-clang.exe"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS="-C target-feature=+crt-static -C link-arg=-Wno-unused-command-line-argument -C dlltool=$LLVM_MINGW\\llvm-dlltool.exe"
export WSLENV="${WSLENV:-}:CARGO_TARGET_DIR:CARGO_BUILD_TARGET:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_RUSTFLAGS"
cd "$ROOT"
exec "$CARGO" "$@"
