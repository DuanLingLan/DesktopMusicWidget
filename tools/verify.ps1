# Verification harness for DesktopMusicWidget.
#
# Generates real (silent) WAV fixtures, then checks:
#   1. decoding + tag reading publishes a now-playing title
#   2. the first-run wizard appears when there is no config anywhere
#   3. the settings window is built with all of its controls
#
# Run from a full-language PowerShell (the constrained inline host cannot use
# System.IO.BinaryWriter):
#
#     powershell -File tools\verify.ps1
[CmdletBinding()]
param(
    [string]$Exe,
    [string]$Diag
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# `$PSScriptRoot` is not always populated, so fall back to the command's path.
$scriptDir = if ($PSScriptRoot) {
    $PSScriptRoot
} else {
    Split-Path -Parent $MyInvocation.MyCommand.Definition
}
$repoRoot = Split-Path -Parent $scriptDir
if (-not $Exe) { $Exe = Join-Path $repoRoot 'target\release\desktop-music-widget.exe' }
if (-not $Diag) { $Diag = Join-Path $repoRoot 'target\release\diag.exe' }

$exe = (Resolve-Path $Exe).Path
$diagExe = (Resolve-Path $Diag).Path
$root = Join-Path $env:TEMP 'dmw-verify'
Remove-Item $root -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path $root | Out-Null

$pass = 0
$fail = 0
# `$Ok` is deliberately untyped: `-match` can return an array, and a strict
# [bool] parameter would either coerce oddly or throw.
function Check([string]$Name, $Ok, [string]$Detail = '') {
    $ok = if ($Ok -is [array]) { $Ok.Count -gt 0 } else { [bool]$Ok }
    if ($ok) { $script:pass++; Write-Host ("  PASS  {0} {1}" -f $Name, $Detail) }
    else { $script:fail++; Write-Host ("  FAIL  {0} {1}" -f $Name, $Detail) }
}
function Log-Has([string]$Log, [string]$Pattern) {
    if (-not $Log) { return $false }
    return [regex]::IsMatch($Log, $Pattern)
}

# --- real WAV fixtures -----------------------------------------------------
# NOTE: the duration parameter must not be called `$Ms` — PowerShell variable
# names are case-insensitive, so it would collide with the `$ms` stream below.
function New-Wav {
    param([string]$Path, [int]$Milliseconds = 400, [int]$Rate = 8000)
    $samples = [int]($Rate * $Milliseconds / 1000)
    $dataBytes = $samples * 2
    $stream = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter($stream)
    $ascii = [System.Text.Encoding]::ASCII
    $bw.Write($ascii.GetBytes('RIFF'))
    $bw.Write([uint32](36 + $dataBytes))
    $bw.Write($ascii.GetBytes('WAVE'))
    $bw.Write($ascii.GetBytes('fmt '))
    $bw.Write([uint32]16)
    $bw.Write([uint16]1)
    $bw.Write([uint16]1)
    $bw.Write([uint32]$Rate)
    $bw.Write([uint32]($Rate * 2))
    $bw.Write([uint16]2)
    $bw.Write([uint16]16)
    $bw.Write($ascii.GetBytes('data'))
    $bw.Write([uint32]$dataBytes)
    $silence = New-Object 'byte[]' $dataBytes
    $bw.Write($silence, 0, $dataBytes)
    $bw.Flush()
    [System.IO.File]::WriteAllBytes($Path, $stream.ToArray())
    $bw.Dispose()
    $stream.Dispose()
}

$music = Join-Path $root 'Music\Album'
New-Item -ItemType Directory -Path $music -Force | Out-Null
New-Wav (Join-Path $music 'First Track.wav')
New-Wav (Join-Path $music 'Second Track.wav')
Set-Content (Join-Path $root 'Music\notes.txt') 'not audio'

function Write-Config([string]$Dir, [string]$Library, [bool]$Autoplay) {
    New-Item -ItemType Directory -Path $Dir -Force | Out-Null
    $escaped = $Library.Replace('\', '\\')
    @"
config_version = 2
language = "en"
library = ["$escaped"]
scan_recursive = true
autoplay = $($Autoplay.ToString().ToLower())
volume = 55
hotkeys_enabled = false
autostart = false
"@ | Set-Content (Join-Path $Dir 'config.toml')
}

function Stop-Quietly($p) {
    if ($p -and -not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
    Start-Sleep -Milliseconds 300
}

# The single-instance mutex is deliberately global, so a leftover copy of THIS
# build makes every later launch exit immediately. Only processes running this
# exact binary are touched: the pre-rename build (HorizMusicWidget) shares the
# process name and must never be disturbed.
function Clear-OurInstances {
    Get-Process -Name 'desktop-music-widget' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -eq $exe } |
        ForEach-Object {
            Write-Host ("  (clearing leftover instance pid {0})" -f $_.Id)
            Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue
        }
    Start-Sleep -Milliseconds 500
}

Clear-OurInstances
Write-Host "`n[1] decode + now-playing"
$cfg = Join-Path $root 'cfg-play'
Write-Config -Dir $cfg -Library (Join-Path $root 'Music') -Autoplay $false
# --play bypasses the walk so the queue arrives deterministically.
$tracks = @(Get-ChildItem $music -Filter *.wav | ForEach-Object { $_.FullName })
$quoted = @($tracks | ForEach-Object { '"' + $_ + '"' })
$p = Start-Process -FilePath $exe -ArgumentList (@('--config-dir', ('"' + $cfg + '"'), '--play') + $quoted) -PassThru
Start-Sleep -Seconds 4
$log = Get-Content (Join-Path $cfg 'widget.log') -Raw
Check 'process stayed alive' (-not $p.HasExited)
Check 'a track was published' (Log-Has $log 'now playing: First Track|now playing: Second Track')
Check 'fallback title came from the file stem' (Log-Has $log 'First Track')
Check 'no decode failures' (-not (Log-Has $log 'cannot decode'))
Stop-Quietly $p

Clear-OurInstances
Write-Host "`n[2] first-run wizard (no config anywhere)"
# An empty APPDATA hides the legacy config, so this really is a first run.
$fakeAppData = Join-Path $root 'fake-appdata'
New-Item -ItemType Directory -Path $fakeAppData -Force | Out-Null
$cfg2 = Join-Path $root 'cfg-first-run'
New-Item -ItemType Directory -Path $cfg2 -Force | Out-Null
$psi = New-Object System.Diagnostics.ProcessStartInfo
$psi.FileName = $exe
$psi.Arguments = "--config-dir `"$cfg2`""
$psi.UseShellExecute = $false
$psi.EnvironmentVariables['APPDATA'] = $fakeAppData
$p2 = [System.Diagnostics.Process]::Start($psi)
Start-Sleep -Seconds 3
# The picker is a modal dialog, so the process sits on it: look for its window.
$found = @()
Add-Type -Namespace V -Name U -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr l);
public delegate bool EnumWindowsProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
'@
$cb = [V.U+EnumWindowsProc] {
    param($h, $l)
    $owner = 0
    [void][V.U]::GetWindowThreadProcessId($h, [ref]$owner)
    if ($owner -eq $script:p2.Id -and [V.U]::IsWindowVisible($h)) {
        $sb = New-Object System.Text.StringBuilder 256
        [void][V.U]::GetClassName($h, $sb, 256)
        $script:found += $sb.ToString()
    }
    return $true
}
[void][V.U]::EnumWindows($cb, [IntPtr]::Zero)
$log2 = Get-Content (Join-Path $cfg2 'widget.log') -Raw -ErrorAction SilentlyContinue
Check 'config was created fresh (not migrated)' (Log-Has $log2 'note=Fresh')
Check 'folder picker is open' (Log-Has ($found -join ',') '#32770')
if (-not (Log-Has ($found -join ',') '#32770')) { Write-Host ("    windows seen: {0}" -f ($found -join ', ')) }
Write-Host ("    process alive: {0}" -f (-not $p2.HasExited))
Stop-Quietly $p2

Clear-OurInstances
Write-Host "`n[3] settings window"
$cfg3 = Join-Path $root 'cfg-settings'
Write-Config -Dir $cfg3 -Library (Join-Path $root 'Music') -Autoplay $false
$p3 = Start-Process -FilePath $exe -ArgumentList '--config-dir', $cfg3, '--open-settings' -PassThru
Start-Sleep -Seconds 4
$diagOut = & $diagExe 2>&1 | Out-String
Check 'widget window found' (Log-Has $diagOut 'WIDGET')
Check 'stacked above the desktop band' (Log-Has $diagOut 'OK   widget #\d+ is ABOVE')
Check 'settings window reported' (Log-Has $diagOut 'child controls: \d+')
Check 'all controls created' (Log-Has $diagOut 'OK   all controls created')
Check 'settings lists the configured music folder' (Log-Has $diagOut '\[ListBox\]\s+\(1 items\)[\s\S]{0,200}dmw-verify')
Check 'settings shows the config path' (Log-Has $diagOut 'config.toml')
Check 'numeric fields are populated' (Log-Has $diagOut '\[Edit\] 340')
Check 'sliders show the configured values' ((Log-Has $diagOut '\[Static\] 55') -and (Log-Has $diagOut '78%'))
Stop-Quietly $p3

Write-Host "`n=== $pass passed, $fail failed ==="
if ($fail -gt 0) { exit 1 }
