# Release build on Windows. Prints the resulting exe size.
$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)
cargo build --release
$exe = Get-Item "target\release\slc.exe"
"{0} — {1:N0} bytes" -f $exe.FullName, $exe.Length
