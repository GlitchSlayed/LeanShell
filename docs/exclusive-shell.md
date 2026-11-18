# Exclusive Shell Mode in LeanShell

LeanShell supports operating as Windows 11's primary shell replacement (**Phase 6**), running without `explorer.exe` launched at Windows sign-in.

## Architecture

When configured in **Exclusive Shell Mode** (`shell_mode = "exclusive"`):
1. **Per-User Winlogon Shell Registration**:
   - LeanShell writes its executable path to `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell` with transactional backup in `HKCU\Software\LeanShell\BackupShell`.
2. **Reversible Shell Experience Host Neutering (Phase 7)**:
   - When running in exclusive shell mode with `neuter_shell_hosts = true` (or via `--neuter-hosts`), LeanShell redirects bloated background Windows shell hosts (`StartMenuExperienceHost.exe`, `SearchHost.exe`, `Widgets.exe`, `WidgetService.exe`, `ShellExperienceHost.exe`, `CrossDeviceService.exe`) under `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\<exe>` (`Debugger = "systray.exe"`) and terminates active instances.
   - Prior IFEO values are backed up under `HKCU\Software\LeanShell\BackupIFEO` and fully reverted by `--restore-hosts`, `--unregister-shell`, `--restore-shell`, or `scripts/restore-shell.ps1`. Non-admin sessions degrade gracefully without failing startup.
3. **On-Demand File Explorer Coexistence & Auto-Reaper (Phase 7)**:
   - When the user launches `explorer.exe` (`Win+E`, launcher file reveal, or folder paths), Explorer opens strictly in file browsing mode without restoring the stock taskbar.
   - With `auto_reap_explorer = true`, once all Explorer folder windows (`CabinetWClass` / `ExploreWClass`) and active file-operation dialogs (`OperationStatusWindow` / `#32770`) close, LeanShell automatically reaps the idle `explorer.exe` process after a 1500 ms debounce.
4. **Native Shell Chords & Volume OSD (Phase 7)**:
   - With `claim_shell_chords = true`, LeanShell natively handles `Win+E` (Files), `Win+R` (Run via `> `), `Win+D` (Show Desktop toggle), `Win+I` (Settings), `Win+X` (Power menu in launcher), and hardware Volume/Mute keys with a Direct2D pill OSD.
5. **Emergency Safe-Mode Fallback**:
   - At Windows logon / boot, LeanShell checks physical keyboard state for the **Shift** key (`GetAsyncKeyState(VK_SHIFT) < 0`).
   - If **Shift** is held down while signing in:
     - LeanShell immediately reverts the `HKCU` Winlogon shell override and IFEO host overrides (best effort).
     - Spawns `explorer.exe`.
     - Terminates cleanly.
   - This ensures the user is **never stranded**, even without access to Task Manager or command prompt.
6. **Logon Startup Applications (Track T3)**:
   - When Explorer does not run at logon, Windows does not automatically dispatch startup applications.
   - LeanShell spawns a background thread (once per Windows Authentication LUID logon session) to safely enumerate and launch items from:
     - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce`
     - `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce` (read-only query)
     - User Startup folder: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
     - Common Startup folder: `%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup`
7. **Desktop Background Window**:
   - Creates a minimal `HWND_BOTTOM` window spanning the virtual multi-monitor desktop.
   - Renders the user's active desktop wallpaper (`IShellItemImageFactory` + `PaintDesktop` fallback) with zero background CPU/GPU overhead.
   - Provides right-click desktop menu for quick access to LeanShell Settings, File Explorer, Terminal, Display settings, and Shell Restoration.

---

## Commands & Configuration

### CLI Flags
```bash
# Register LeanShell as the user's primary shell in HKCU
leanshell.exe --register-shell

# Unregister LeanShell, restore IFEO host overrides, and restore Explorer
leanshell.exe --unregister-shell

# Neuter / restore background Shell Experience hosts explicitly (elevated for HKLM IFEO)
leanshell.exe --neuter-hosts
leanshell.exe --restore-hosts

# Print live shell ownership, Winlogon/IFEO state, and monitor work areas
leanshell.exe --diagnose

# Emergency CLI recovery (unhooks, restores IFEO hosts, taskbar & shell)
leanshell.exe --restore-shell
```

### Config File (`config.toml`)
```toml
# "exclusive" (primary Windows shell).
shell_mode = "exclusive"
neuter_shell_hosts = true
auto_reap_explorer = true
claim_shell_chords = true
```

---

## Testing in Local Windows 11 VM

Always test shell-level changes in the dedicated VM:
1. Start VM:
   ```bash
   quickemu --vm /home/vanshdatta/VMs/windows/windows-11.conf
   ```
2. In the VM, install the latest build and run:
   ```powershell
   ./leanshell.exe --register-shell
   ```
3. Sign out of Windows and sign back in.
4. Verify LeanShell boots directly with the dock/panel and wallpaper desktop.
5. Verify emergency fallback: Sign out, hold **Shift**, and sign in. Confirm Explorer starts normally.
