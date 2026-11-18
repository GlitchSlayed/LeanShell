<#
.SYNOPSIS
    Emergency restore script for LeanShell.
.DESCRIPTION
    Restores default Windows shell behavior:
    1. Kills running LeanShell/Wisp processes.
    2. Resets the user's Winlogon Shell registry entry back to default (explorer.exe).
    3. Unhides and shows the Windows Taskbar (Shell_TrayWnd).
    4. Ensures explorer.exe is running.
#>

[CmdletBinding()]
param()

$ErrorActionPreference = "Continue"

# Clear LIB/INCLUDE if set to nonexistent/cross-compilation paths that break Add-Type (csc)
$env:LIB = ""
$env:INCLUDE = ""


Write-Host "==> [1/5] Terminating LeanShell / Wisp processes (releases WH_KEYBOARD_LL hooks)..." -ForegroundColor Cyan
Get-Process -Name "leanshell", "leanshell-nightly", "wisp", "wisp-nightly" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "    Killing process $($_.Name) (PID: $($_.Id))" -ForegroundColor Yellow
    Stop-Process -Id $_.Id -Force
}

Write-Host "==> [2/5] Restoring user Winlogon shell registry value..." -ForegroundColor Cyan
$winlogonKey = "HKCU:\Software\Microsoft\Windows NT\CurrentVersion\Winlogon"
if (Test-Path $winlogonKey) {
    $currentShell = (Get-ItemProperty -Path $winlogonKey -Name "Shell" -ErrorAction SilentlyContinue).Shell
    if ($currentShell) {
        Write-Host "    Found custom shell value: '$currentShell'. Removing to restore default explorer..." -ForegroundColor Yellow
        Remove-ItemProperty -Path $winlogonKey -Name "Shell" -Force -ErrorAction SilentlyContinue
    } else {
        Write-Host "    No custom Winlogon Shell override found in HKCU (already default)." -ForegroundColor Green
    }
} else {
    Write-Host "    Winlogon key does not exist in HKCU (default behavior intact)." -ForegroundColor Green
}

Write-Host "==> [3/5] Restoring IFEO Shell Experience host overrides..." -ForegroundColor Cyan
$ifeoBase = "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options"
$backupIfeoBase = "HKCU:\Software\LeanShell\BackupIFEO"
$hosts = @(
    "StartMenuExperienceHost.exe",
    "SearchHost.exe",
    "Widgets.exe",
    "WidgetService.exe",
    "ShellExperienceHost.exe",
    "CrossDeviceService.exe"
)
foreach ($hostExe in $hosts) {
    $ifeoKey = Join-Path $ifeoBase $hostExe
    $backupKey = Join-Path $backupIfeoBase $hostExe
    $originalDebugger = $null
    $managed = $false
    if (Test-Path $backupKey) {
        $props = Get-ItemProperty -Path $backupKey -ErrorAction SilentlyContinue
        if ($props.OriginalDebugger) { $originalDebugger = $props.OriginalDebugger }
        if ($props.ManagedByLeanShell) { $managed = $true }
    }
    if (Test-Path $ifeoKey) {
        $currentDebugger = (Get-ItemProperty -Path $ifeoKey -Name "Debugger" -ErrorAction SilentlyContinue).Debugger
        try {
            if ($originalDebugger) {
                Set-ItemProperty -Path $ifeoKey -Name "Debugger" -Value $originalDebugger -ErrorAction Stop
                Write-Host "    Restored original Debugger for $hostExe." -ForegroundColor Yellow
            } elseif ($managed -or ($currentDebugger -and $currentDebugger.Trim().ToLower() -eq "systray.exe")) {
                Remove-ItemProperty -Path $ifeoKey -Name "Debugger" -Force -ErrorAction Stop
                Write-Host "    Removed LeanShell IFEO Debugger for $hostExe." -ForegroundColor Yellow
            }
        } catch {
            Write-Warning "    Could not revert HKLM IFEO override for $hostExe (run as Administrator if needed): $_"
        }
    }
}
if (Test-Path $backupIfeoBase) {
    Remove-Item -Path $backupIfeoBase -Recurse -Force -ErrorAction SilentlyContinue
}

Write-Host "==> [4/5] Unhiding Windows Taskbar..." -ForegroundColor Cyan
$definition = @"
using System;
using System.Runtime.InteropServices;

public class TaskbarRestorer {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr FindWindowEx(IntPtr parentHandle, IntPtr childAfter, string className, string windowTitle);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [DllImport("user32.dll")]
    public static extern int SetWindowRgn(IntPtr hWnd, IntPtr hRgn, bool bRedraw);

    public const int SW_SHOW = 5;

    public static void ShowTaskbar() {
        IntPtr trayHwnd = FindWindow("Shell_TrayWnd", null);
        if (trayHwnd != IntPtr.Zero) {
            SetWindowRgn(trayHwnd, IntPtr.Zero, true);
            ShowWindow(trayHwnd, SW_SHOW);
        }
        // Enumerate and restore all secondary taskbars across all monitors
        IntPtr current = IntPtr.Zero;
        while ((current = FindWindowEx(IntPtr.Zero, current, "Shell_SecondaryTrayWnd", null)) != IntPtr.Zero) {
            SetWindowRgn(current, IntPtr.Zero, true);
            ShowWindow(current, SW_SHOW);
        }
    }
}
"@

try {
    Add-Type -TypeDefinition $definition -Language CSharp -ErrorAction SilentlyContinue
    [TaskbarRestorer]::ShowTaskbar()
    Write-Host "    Shell_TrayWnd and Shell_SecondaryTrayWnd shown successfully." -ForegroundColor Green
} catch {
    Write-Warning "    Could not invoke ShowWindow on taskbar: $_"
}

Write-Host "==> [5/5] Ensuring explorer.exe is running..." -ForegroundColor Cyan
$explorer = Get-Process -Name "explorer" -ErrorAction SilentlyContinue
if (-not $explorer) {
    Write-Host "    Starting explorer.exe..." -ForegroundColor Yellow
    Start-Process -FilePath "explorer.exe"
} else {
    Write-Host "    explorer.exe is running." -ForegroundColor Green
}

Write-Host "`n[SUCCESS] Shell restored to default Windows Explorer." -ForegroundColor Green
