# Exclusive Shell Mode in LeanShell

LeanShell supports operating as Windows 11's primary shell replacement (**Phase 6**), running without `explorer.exe` launched at Windows sign-in.

## Architecture

When configured in **Exclusive Shell Mode** (`shell_mode = "exclusive"`):
1. **Per-User Winlogon Shell Registration**:
   - LeanShell writes its executable path to `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`.
   - **Non-Negotiable Safety Rule**: Registration is strictly per-user (`HKEY_CURRENT_USER`). It never writes to machine-wide `HKLM`.
2. **Emergency Safe-Mode Fallback**:
   - At Windows logon / boot, LeanShell checks physical keyboard state for the **Shift** key (`GetAsyncKeyState(VK_SHIFT) < 0`).
   - If **Shift** is held down while signing in:
     - LeanShell immediately reverts the `HKCU` Winlogon shell override.
     - Spawns `explorer.exe`.
     - Terminates cleanly.
   - This ensures the user is **never stranded**, even without access to Task Manager or command prompt.
3. **Logon Startup Applications (Track T3)**:
   - When Explorer does not run at logon, Windows does not automatically dispatch startup applications.
   - LeanShell spawns a background thread to safely enumerate and launch items from:
     - `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce`
     - `HKLM\Software\Microsoft\Windows\CurrentVersion\Run` and `RunOnce` (read-only query)
     - User Startup folder: `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup`
     - Common Startup folder: `%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup`
4. **Desktop Background Window**:
   - Creates a minimal `HWND_BOTTOM` window spanning the virtual multi-monitor desktop.
   - Uses native `PaintDesktop` to render the user's active desktop wallpaper and pattern with zero CPU or GPU decoding overhead.
   - Provides right-click desktop menu for quick access to LeanShell Settings, File Explorer, Terminal, and Shell Restoration.
5. **On-Demand File Explorer Coexistence**:
   - When the user launches `explorer.exe` (via `Win+E`, Run dialog, or launcher search), Explorer starts purely in file browsing mode without restoring the default taskbar.

---

## Commands & Configuration

### CLI Flags
```bash
# Register LeanShell as the user's primary shell in HKCU
leanshell.exe --register-shell

# Unregister LeanShell and restore default Windows Explorer
leanshell.exe --unregister-shell

# Emergency CLI recovery (unhooks, restores taskbar & shell)
leanshell.exe --restore-shell
```

### Config File (`config.toml`)
```toml
# "companion" (runs alongside Windows while hiding taskbar)
# or "exclusive" (primary Windows shell).
shell_mode = "exclusive"
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
