# End-to-end smoke test of a built slc.exe on a clean Windows machine (CI or local).
# Usage: pwsh scripts/smoke.ps1 -Exe path\to\slc.exe
# Checks: CLI, self-test, tray app start/control/exit, and a full install -> uninstall round trip
# that must leave nothing behind.
param([Parameter(Mandatory)][string]$Exe)
$ErrorActionPreference = "Stop"
$work = Join-Path $env:RUNNER_TEMP "slc-smoke"
if (-not $env:RUNNER_TEMP) { $work = Join-Path $env:TEMP "slc-smoke" }
Remove-Item $work -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $work | Out-Null
Copy-Item $Exe "$work\slc.exe"
$slc = "$work\slc.exe"
$failures = @()
function Step($name, [scriptblock]$body) {
    try { & $body; "PASS  $name" } catch { "FAIL  $name :: $_"; $script:failures += $name }
}
function Run([string[]]$argv) {
    $p = Start-Process $slc -ArgumentList $argv -Wait -PassThru -WindowStyle Hidden
    return $p.ExitCode
}
function SlcRunning { @(Get-Process slc -ErrorAction SilentlyContinue).Count -gt 0 }
function WaitUntil([scriptblock]$cond, [int]$seconds) {
    $t = [Diagnostics.Stopwatch]::StartNew()
    while ($t.Elapsed.TotalSeconds -lt $seconds) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 250 }
    return $false
}

$os = Get-CimInstance Win32_OperatingSystem
"OS: $($os.Caption) $($os.Version) build $($os.BuildNumber)"

Step "version" { if ((Run @('--version')) -ne 0) { throw "exit code" } }
Step "self-test" { if ((Run @('--self-test')) -ne 0) { throw "exit code" } }
Step "unknown argument is rejected" { if ((Run @('--bogus')) -eq 0) { throw "accepted" } }

Step "tray app starts, takes commands and exits cleanly" {
    $log = "$work\run.log"
    Start-Process $slc -ArgumentList @('--log', $log) | Out-Null
    if (-not (WaitUntil { (Test-Path $log) -and (Select-String -Path $log -Pattern 'controller window created' -Quiet) } 15)) {
        throw "no controller window: $(Get-Content $log -Raw -ErrorAction SilentlyContinue)"
    }
    foreach ($a in @(@('--set', 'brightness=40'), @('--set', 'kelvin=3400'), @('--pause', '0'), @('--resume'), @('--filter', 'grayscale'), @('--filter', 'none'))) {
        if ((Run $a) -ne 0) { throw "command failed: $a" }
    }
    if ((Run @('--exit')) -ne 0) { throw "--exit failed" }
    if (-not (WaitUntil { -not (SlcRunning) } 10)) { throw "process did not exit" }
    if (-not (Select-String -Path $log -Pattern '^.* exit 0' -Quiet)) { throw "no clean exit in log" }
    if (-not (Test-Path "$work\slc.ini")) { throw "portable settings were not written" }
}

Step "reset works with no instance running" { if ((Run @('--reset')) -ne 0) { throw "exit code" } }

$prog = Join-Path $env:LOCALAPPDATA "Programs\SLC"
$roam = Join-Path $env:APPDATA "SLC"
$lnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Screen Lighting Control.lnk"
$unKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\MONTINA.SLC"
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"

Step "install creates program, shortcut, uninstall entry and autostart, and starts SLC" {
    if ((Run @('--install')) -ne 0) { throw "exit code" }
    if (-not (Test-Path "$prog\slc.exe")) { throw "exe not installed" }
    if (-not (Test-Path $lnk)) { throw "no Start menu shortcut" }
    if (-not (Test-Path $unKey)) { throw "no uninstall entry" }
    if (-not (Get-ItemProperty $runKey -Name 'Screen Lighting Control' -ErrorAction SilentlyContinue)) { throw "no autostart" }
    if (-not (WaitUntil { SlcRunning } 10)) { throw "installed copy not running" }
    if ((Get-ItemProperty $unKey).DisplayVersion -eq $null) { throw "no DisplayVersion" }
}

Step "uninstall leaves nothing behind" {
    $p = Start-Process "$prog\slc.exe" -ArgumentList @('--uninstall', '--quiet') -Wait -PassThru -WorkingDirectory $env:TEMP
    if ($p.ExitCode -ne 0) { throw "exit code $($p.ExitCode)" }
    if (-not (WaitUntil { -not (Test-Path $prog) } 30)) { throw "program folder still exists" }
    $left = @()
    if (Test-Path $roam) { $left += $roam }
    if (Test-Path $lnk) { $left += $lnk }
    if (Test-Path $unKey) { $left += $unKey }
    if (Test-Path "HKCU:\Software\MONTINA") { $left += "HKCU:\Software\MONTINA" }
    if (Get-ItemProperty $runKey -Name 'Screen Lighting Control' -ErrorAction SilentlyContinue) { $left += "Run entry" }
    if (SlcRunning) { $left += "running process" }
    if ($left.Count) { throw "left behind: $($left -join ', ')" }
}

if ($failures.Count) { throw "$($failures.Count) smoke step(s) failed: $($failures -join '; ')" }
"All smoke steps passed."
