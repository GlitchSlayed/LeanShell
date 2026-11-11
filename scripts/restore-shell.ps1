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

Write-Host "==> [1/4] Terminating LeanShell / Wisp processes (releases WH_KEYBOARD_LL hooks)..." -ForegroundColor Cyan
Get-Process -Name "leanshell", "leanshell-nightly", "wisp", "wisp-nightly" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "    Killing process $($_.Name) (PID: $($_.Id))" -ForegroundColor Yellow
    Stop-Process -Id $_.Id -Force
}

Write-Host "==> [2/4] Restoring user Winlogon shell registry value..." -ForegroundColor Cyan
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

Write-Host "==> [3/4] Unhiding Windows Taskbar..." -ForegroundColor Cyan
$definition = @"
using System;
using System.Runtime.InteropServices;

public class TaskbarRestorer {
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr FindWindow(string lpClassName, string lpWindowName);

    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    public const int SW_SHOW = 5;

    public static void ShowTaskbar() {
        IntPtr trayHwnd = FindWindow("Shell_TrayWnd", null);
        if (trayHwnd != IntPtr.Zero) {
            ShowWindow(trayHwnd, SW_SHOW);
        }
        IntPtr secondaryTray = FindWindow("Shell_SecondaryTrayWnd", null);
        if (secondaryTray != IntPtr.Zero) {
            ShowWindow(secondaryTray, SW_SHOW);
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

Write-Host "==> [4/4] Ensuring explorer.exe is running..." -ForegroundColor Cyan
$explorer = Get-Process -Name "explorer" -ErrorAction SilentlyContinue
if (-not $explorer) {
    Write-Host "    Starting explorer.exe..." -ForegroundColor Yellow
    Start-Process -FilePath "explorer.exe"
} else {
    Write-Host "    explorer.exe is running." -ForegroundColor Green
}

Write-Host "`n[SUCCESS] Shell restored to default Windows Explorer." -ForegroundColor Green
