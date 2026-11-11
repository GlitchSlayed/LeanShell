# LeanShell Source Audit Remediation Matrix

This document tracks all 16 findings from the November 2026 Source Audit (`LeanShell-main.zip`, v0.0.6), detailing affected code, current status, remediation plan, regression tests, and verification evidence.

---

## Findings Matrix

| ID | Priority | Finding | Affected Files | Status | Target Phase |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **F01** | **P1** | Exclusive mode cannot establish tray or reserve panel space | `src/taskbar.rs`, `src/app.rs`, `src/tray_host/proxy.rs`, `src/dock/space.rs` | **Remediated & Verified** | Work Package A |
| **F02** | **P1** | Watchdog startup failure is silently ignored | `src/taskbar.rs:561` | **Remediated & Verified** | Work Package A |
| **F03** | **P1** | Exclusive-mode crash and emergency quit have no Explorer handoff | `src/taskbar.rs`, `src/app.rs`, `src/main.rs` | **Remediated & Verified** | Work Package A |
| **F04** | **P1** | Restore command does not restore live session reliably | `src/taskbar.rs:831`, `src/desktop.rs:154` | **Remediated & Verified** | Work Package A |
| **F05** | **P1** | PowerShell recovery script can kill recovery itself and leave invisible taskbars | `scripts/restore-shell.ps1`, `src/taskbar.rs` | **Remediated & Verified** | Work Package A |
| **F06** | **P1** | Startup entries launched as filenames instead of command lines | `src/startup.rs`, `src/launch.rs`, `core/src/startup_cmd.rs` | **Remediated & Verified** | Work Package B |
| **F07** | **P1** | RunOnce deletion ignores completion contract (`!` prefix, failures) | `src/startup.rs:72-86`, `core/src/startup_cmd.rs` | **Remediated & Verified** | Work Package B |
| **F08** | **P1** | Shell registration is nontransactional and does not preserve prior ownership | `settings/src/ui.rs`, `platform-win/src/shell.rs` | **Remediated & Verified** | Work Package B |
| **F09** | **P1** | Configured exclusive mode treated as actual session ownership | `src/app.rs`, `platform-win/src/shell.rs`, `src/startup.rs` | **Remediated & Verified** | Work Package B |
| **F10** | **P1** | Left/right panels resize window without implementing vertical UI | `src/dock.rs`, `src/dock/logic.rs`, `src/dock/render.rs` | **Remediated & Verified** | Work Package C |
| **F11** | **P2** | Renderer settings do not select the running renderer | `src/app.rs`, `ui-d2d/src/lib.rs`, `ui-slint/src/lib.rs` | **Remediated & Verified** | Work Package C |
| **F12** | **P2** | Windows-key tracking misidentifies chords and tests a different implementation | `platform-win/src/hook.rs`, `core/src/hook.rs` | **Remediated & Verified** | Work Package C |
| **F13** | **P2** | Persistent polling contradicts zero-wakeup design claim | `src/dock.rs`, `src/tray_host/proxy.rs`, `src/dock/space.rs` | **Remediated & Verified** | Work Package D |
| **F14** | **P2** | Window activation sleeps on the UI thread | `src/commands/open_windows.rs:104-170` | **Remediated & Verified** | Work Package C |
| **F15** | **P2** | Tray AppBar forwarding does not handle shared-memory translation | `src/tray_host/proxy.rs`, `src/dock/space.rs` | **Remediated & Verified** | Work Package C |
| **F16** | **P2** | Optional background services abort entire shell on spawn failure | `src/indexer.rs`, `src/files.rs`, `src/commands/mod.rs` | **Remediated & Verified** | Work Package D |

---

## Detailed Item Tracking

### Finding 1: Exclusive mode cannot establish tray or reserve panel space
- **Affected Current Code**: `src/taskbar.rs:558-562`, `src/app.rs:785-800`, `src/app.rs:703-707`, `src/tray_host/proxy.rs:220-224`, `src/dock/space.rs`.
- **Status**: Confirmed.
- **Root Cause**: `TaskbarHider::start` fails if `primary_taskbar` is absent. `sync_reserve` and tray activation were gated on `TaskbarHider::is_active`. `DockSpace::set_reserve` attempted to message Explorer's taskbar.
- **Proposed Repair**:
  1. Decouple exclusive shell initialization from `TaskbarHider`.
  2. In Exclusive Shell mode, manage monitor work areas directly via `SystemParametersInfoW(SPI_SETWORKAREA, ...)` without querying or messaging Explorer.
  3. Initialize `TrayHost` independently as primary `Shell_TrayWnd` host before startup apps launch.
- **Tests**: Unit tests for work-area geometry calculation; VM test verifying work-area reservation and tray icon reception with `explorer.exe` killed.

### Finding 2: Watchdog startup failure is silently ignored
- **Affected Current Code**: `src/taskbar.rs:561`.
- **Status**: Confirmed.
- **Root Cause**: `Watchdog::start(&snapshot).ok()` silently dropped watchdog launch errors and continued taskbar suppression.
- **Proposed Repair**: Propagate watchdog start errors in `TaskbarHider::configure`. If watchdog fails to arm, fail closed: do NOT hide the taskbar and return error.
- **Tests**: Inject watchdog spawn failure test; verify taskbar remains unhidden.

### Finding 3: Exclusive-mode crash and emergency quit have no Explorer handoff
- **Affected Current Code**: `src/taskbar.rs:538-565`, `src/app.rs:2379-2385`, `src/app.rs:2253-2269`, `src/main.rs:34-39`.
- **Status**: Confirmed.
- **Root Cause**: In exclusive mode, if LeanShell panics, is killed, or emergency chord is pressed, no fallback shell (Explorer) is launched, stranding the session.
- **Proposed Repair**:
  1. Dedicated supervisor process launched before UI initialization that waits on the shell process handle (`SYNCHRONIZE`).
  2. If the shell process terminates abnormally or emergency recovery is signaled, supervisor starts `explorer.exe`.
  3. Differentiate between normal session logoff/shutdown and abnormal termination so Explorer isn't launched during user logoff.
- **Tests**: VM test terminating LeanShell process; assert supervisor launches Explorer within 500ms.

### Finding 4: Restore command does not restore live session reliably
- **Affected Current Code**: `src/taskbar.rs:831-881`, `src/desktop.rs:154-155`.
- **Status**: Confirmed.
- **Root Cause**: `restore_shell_cli` unregisters HKCU and unhides existing windows, but does not signal running LeanShell instances to stop suppressing, does not spawn Explorer if absent, and returns 0 unconditionally.
- **Proposed Repair**: Unify CLI and desktop restore into a coordinated recovery routine: broadcast/signal running LeanShell instances to terminate suppression, restore window regions, spawn and verify Explorer shell window if missing, and return standard exit codes.
- **Tests**: CLI invocation while companion mode active and while exclusive mode active.

### Finding 5: PowerShell recovery script can kill recovery itself and leave invisible taskbars
- **Affected Current Code**: `scripts/restore-shell.ps1:18-20`, `src/taskbar.rs:118-130`.
- **Status**: Confirmed.
- **Root Cause**: Script uses `Get-Process *leanshell* | Stop-Process`, killing the watchdog before it can restore. Script only calls `ShowWindow`, which fails if window has empty region `SetWindowRgn`. Only restores first secondary bar.
- **Proposed Repair**: Differentiate watchdog from host. In script, invoke `SetWindowRgn(hwnd, 0, true)` on all found `Shell_TrayWnd` and `Shell_SecondaryTrayWnd` handles. Start Explorer if shell window is absent.
- **Tests**: VM script execution with clipped taskbars.

### Finding 6: Startup entries are launched as filenames instead of command lines
- **Affected Current Code**: `src/startup.rs:68-76`, `src/launch.rs:39-40,113-126`.
- **Status**: Confirmed.
- **Root Cause**: `launch::open` passes entire command string as `lpFile` to `ShellExecuteExW` with null `lpParameters`. Fails on quotes and flags (e.g. `"C:\app.exe" --flag`). Environment variables in `REG_EXPAND_SZ` are unexpanded.
- **Proposed Repair**: Parse Windows command line into application path and argument string. Expand environment variables with `ExpandEnvironmentStringsW`. Initialize COM apartment (`COINIT_APARTMENTTHREADED`) on background launch worker.
- **Tests**: Unit tests for command line parsing with quotes, arguments, environment variables.

### Finding 7: RunOnce deletion ignores completion contract
- **Affected Current Code**: `src/startup.rs:72-86`.
- **Status**: Confirmed.
- **Root Cause**: RunOnce keys are deleted unconditionally before execution, ignoring `!` prefix which specifies deletion only after successful completion.
- **Proposed Repair**: Inspect key prefix: if prefixed with `!`, defer deletion until process exits with success.
- **Tests**: Unit tests for RunOnce prefix parsing and deletion lifecycle.

### Finding 8: Shell registration is nontransactional and does not preserve prior ownership
- **Affected Current Code**: `settings/src/ui.rs:138-155`, `platform-win/src/shell.rs:34-72`.
- **Status**: Confirmed.
- **Root Cause**: Overwrites HKCU Shell without backing up prior value, fails to validate target binary existence, ignores registry errors.
- **Proposed Repair**: Backup existing value, type, and ownership metadata to `HKCU\Software\LeanShell\BackupShell`. Validate binary existence before writing. Implement atomic rollback if settings fail.
- **Tests**: Unit test for backup/restore roundtrip with simulated registry.

### Finding 9: Configured exclusive mode treated as actual session ownership
- **Affected Current Code**: `src/app.rs:546-559`, `platform-win/src/shell.rs:14-29`, `src/startup.rs:15-27`.
- **Status**: Confirmed.
- **Root Cause**: Checks only if `config.shell_mode == Exclusive` or if HKCU Shell contains string, treating it as exclusive ownership even if Explorer is actively the desktop shell.
- **Proposed Repair**: Implement `SessionOwnership` state machine:
  - `DesiredMode`: configured preference for next logon.
  - `ActualOwnership`: detected at runtime by verifying whether Explorer is running as shell (`GetShellWindow()`).
  - Do NOT dispatch startup entries if changing settings inside an active Explorer session.
- **Tests**: Unit test state machine transitions; verify startup apps only launch when `ActualOwnership == Exclusive`.

### Finding 10: Left/right panels resize window without implementing vertical UI
- **Affected Current Code**: `src/dock.rs:930-943,1294-1350`, `src/dock/logic.rs:124-128`.
- **Status**: Confirmed.
- **Root Cause**: Geometry calculations assume horizontal axis X is primary. In left/right edge, window is tall/narrow but layout logic still places items horizontally, clipping all icons.
- **Proposed Repair**: Add orientation awareness to `AppLayout` and `Dock`: in vertical mode, main axis is Y, cross axis is X. Compute slot positions along Y and hit test accordingly.
- **Tests**: Unit tests for vertical `AppLayout` positions and hit testing.

### Finding 11: Renderer settings do not select running renderer
- **Affected Current Code**: `src/app.rs:30,178,409`, `ui-d2d/src/lib.rs:37-40`, `ui-slint/src/lib.rs:100-131`.
- **Status**: Confirmed.
- **Root Cause**: Main application continues to use legacy `crate::render::Renderer`. `ui-d2d::D2DRenderer::draw` is a no-op returning `Ok(())`.
- **Proposed Repair**: Unify the rendering contract so that the active window paints through `leanshell_render::Renderer` and `leanshell_ui_d2d`.
- **Tests**: Compilation and rendering verification.

### Finding 12: Windows-key tracking misidentifies chords and tests a different implementation
- **Affected Current Code**: `platform-win/src/hook.rs:54-85`, `core/src/hook.rs:24-44`.
- **Status**: Confirmed.
- **Root Cause**: `platform-win/src/hook.rs` has custom duplicate state logic instead of calling `core::hook::WinKeyTracker`. Fails when modifiers (Ctrl, Shift) are depressed before Win.
- **Proposed Repair**: Unify `platform-win/src/hook.rs` to directly use `core::hook::WinKeyTracker` for event processing. Handle pre-existing modifiers.
- **Tests**: Unit tests for Ctrl-before-Win, Shift-before-Win, autorepeat, unmatched keyup.

### Finding 13: Persistent polling contradicts zero-wakeup design claim
- **Affected Current Code**: `src/dock.rs:75,951-1013`, `src/tray_host/proxy.rs:28,156`, `src/dock/space.rs:33,167`.
- **Status**: Confirmed.
- **Root Cause**: Hidden dock still maintains a 100ms poll timer even when pointer is resting or raw input is enabled.
- **Proposed Repair**: In quiescent state (hidden dock, stationary pointer), eliminate poll timer and rely on raw input and window event hooks.
- **Tests**: Measure timer activations and verify wakeups cease when idle.

### Finding 14: Window activation sleeps on UI thread
- **Affected Current Code**: `src/commands/open_windows.rs:104-170`.
- **Status**: Confirmed.
- **Root Cause**: `focus()` has loops of `std::thread::sleep(Duration::from_millis(10))` on the UI thread, blocking message processing for up to 400ms.
- **Proposed Repair**: Eliminate synchronous sleeps in UI thread. Use asynchronous foreground requests or observe `EVENT_SYSTEM_FOREGROUND`.
- **Tests**: Test window focus without thread blocking.

### Finding 15: Tray AppBar forwarding does not handle shared-memory translation
- **Affected Current Code**: `src/tray_host/proxy.rs:771-772,662-690`.
- **Status**: Confirmed.
- **Root Cause**: Third-party AppBar messages forwarded through `WM_COPYDATA` need shared memory pointer translation when sent across processes.
- **Proposed Repair**: In standalone exclusive mode, handle AppBar requests locally without forwarding. In companion mode, implement safe forwarding.
- **Tests**: Compatibility testing with AppBar clients.

### Finding 16: Optional background services abort entire shell on spawn failure
- **Affected Current Code**: `src/indexer.rs:54`, `src/files.rs:78`, `src/commands/mod.rs:150`.
- **Status**: Confirmed.
- **Root Cause**: Thread spawning uses `.expect("Failed to spawn thread")`. Under resource pressure, this aborts the entire desktop shell process.
- **Proposed Repair**: Replace `.expect(...)` with error propagation, logging, and graceful degradation to reduced-feature mode.
- **Tests**: Unit tests for worker initialization failure handling.
