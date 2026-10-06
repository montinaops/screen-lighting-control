#!/usr/bin/env bash
# Run the Windows-host cargo from WSL with the MSVC toolchain, the one releases use
# (Visual Studio Build Tools must be installed). Build output goes to
# %USERPROFILE%\.slc-tools\target-msvc, so the exe is at
# target-msvc\release\slc.exe. Used by .local-ci and scripts/release.sh; for everyday
# builds without Visual Studio, scripts/cargo.sh (gnullvm) works too.
# Usage: scripts/cargo-msvc.sh build --release | test | clippy ...
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WIN_HOME="$(wslpath "$(cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r')")"
export CARGO_TARGET_DIR="$(wslpath -w "$WIN_HOME")\\.slc-tools\\target-msvc"
export RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc
export WSLENV="${WSLENV:-}:CARGO_TARGET_DIR:RUSTUP_TOOLCHAIN"
cd "$ROOT"
exec "$WIN_HOME/.cargo/bin/cargo.exe" "$@"
