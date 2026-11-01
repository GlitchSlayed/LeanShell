$ErrorActionPreference = 'Stop'
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class WispPerfVerify {
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindow(string className, IntPtr title);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
}
'@
$repo = Split-Path $PSScriptRoot -Parent
$installed = Join-Path $env:LOCALAPPDATA 'Programs\Wisp Nightly\wisp.exe'
$built = Join-Path $repo 'target\nightly\release\wisp.exe'
$launcher = [WispPerfVerify]::FindWindow('Wisp.Nightly.Launcher', [IntPtr]::Zero)
$tray = [WispPerfVerify]::FindWindow('Shell_TrayWnd', [IntPtr]::Zero)
[uint32]$launcherPid = 0
[uint32]$trayPid = 0
[void][WispPerfVerify]::GetWindowThreadProcessId($launcher, [ref]$launcherPid)
[void][WispPerfVerify]::GetWindowThreadProcessId($tray, [ref]$trayPid)
if ($launcherPid -eq 0) { throw 'Nightly launcher not found' }
$process = Get-Process -Id $launcherPid
$buildHash = (Get-FileHash -LiteralPath $built -Algorithm SHA256).Hash
$installHash = (Get-FileHash -LiteralPath $installed -Algorithm SHA256).Hash
$result = [pscustomobject]@{
 verified_at = [DateTime]::UtcNow.ToString('o')
 launcher_pid = $launcherPid
 process_path = $process.Path
 process_started = $process.StartTime.ToString('o')
 shell_tray_pid = $trayPid
 shell_tray_owned_by_nightly = ($launcherPid -eq $trayPid)
 build_sha256 = $buildHash
 installed_sha256 = $installHash
 installed_matches_build = ($buildHash -eq $installHash)
 running_from_install = ($process.Path -eq $installed)
}
$result | ConvertTo-Json | Set-Content (Join-Path $repo 'target/perf/nightly-verification.json')
$result | ConvertTo-Json
if (-not $result.shell_tray_owned_by_nightly -or -not $result.installed_matches_build -or -not $result.running_from_install) { throw 'Nightly verification failed' }
