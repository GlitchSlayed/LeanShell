# Roadmap

Working name: **LeanShell** (fork of [SumirSeth/wisp](https://github.com/SumirSeth/wisp), MIT). Rename freely.

**Goals:**
- **Total Shell & Host Takeover:** Completely replace the Windows 11 shell (`HKCU\...\Winlogon\Shell`), neuter bloated background Experience hosts (`StartMenuExperienceHost.exe`, `SearchHost.exe`, `Widgets.exe`, `ShellExperienceHost.exe`, etc.) via reversible IFEO redirection, keep `explorer.exe` strictly on-demand for file browsing with auto-reaping when its last folder closes, and reverse every mutation cleanly via `--restore-shell` / `scripts/restore-shell.ps1`.
- **Both Taskbar & Dock Modes:** Support both a persistent **Taskbar Mode** (edge-docked native Windows AppBar / direct `SPI_SETWORKAREA` work area reservation) and a floating **Dock Mode** (auto-hiding, slide-up), both morphing into the launcher.
- **All-In on Native Win32 + Direct2D:** Pure `core` logic and Win32 `platform-win` subsystems driving Direct2D as the sole UI engine across the entire workspace (launcher, dock/panel, OSDs, flyouts, toasts, window-management overlays, and `leanshell-settings`), with zero external UI toolkits.
- **~0% Idle Resource Use:** Native Win32 message-driven design with no web views, no runtimes, and zero background polling.

**Principle:** every phase ships something usable and keeps an automatic, atomic recovery path. All shell, IFEO, service, and NTFS-level testing runs inside the snapshot-capable Windows 11 VM first.

## Budgets (enforced from Phase 0)

| Metric | Budget |
|---|---|
| Idle RAM, Direct2D backend | <= Wisp baseline (~20 MB) + 10% |
| Idle CPU, dock hidden | ~0% |
| Shortcut -> launcher drawn | <= 15 ms (Wisp baseline) |
| Typing -> first results | <= 24 ms (Wisp baseline) |
| Binary, Direct2D backend | < 3 MB |
| Idle wakeups/sec (dock hidden) | 0 (measured over 60 s; wakeups matter more than CPU%) |
| Private bytes / handle count, idle | Tracked per release, must not grow release over release |
| GPU usage, idle | 0% on the Direct2D backend |

Re-measure Wisp's baseline on your own machine in Phase 0; use those numbers, not the README's.

## Phase 0: Fork and baseline

- Fork, keep `LICENSE` and the original copyright notice, add your own line. Add an attribution note to the README.
- Rename crates and binary. Keep the nightly install script so the fork never touches an installed Wisp.
- Get CI green (fmt, clippy with `-D warnings`, tests).
- Run Wisp's `bench/` and record baselines in `bench/BASELINE.md`.
- Set up a Windows VM (snapshot-capable) or a second local user account for all shell-level testing.
- Write the recovery script (`scripts/restore-shell.ps1`): restores the Winlogon shell value, shows the taskbar, kills the fork.

**Exit:** fork builds, budgets recorded, recovery script tested in the VM.

## Phase 1: Core split

Restructure the workspace so UI toolkits are swappable:

- `core`: app index, search/query grammar, calculator, config, window and virtual-desktop model, tray icon model. No Win32 types, no drawing. Unit-testable.
- `platform-win`: HWND management, AppBar, blur/backdrop, hotkeys and hooks, tray host (`Shell_TrayWnd`), shell notifications.
- `render`: a `Renderer` trait. Input: a frame description built from app state (layout, text runs, icons, animation progress). Output: pixels in a window.
- `ui-d2d`: the current Direct2D implementation behind the trait.
- `settings`: the existing settings app.

**Exit:** behavior identical to Wisp, all UI state flows through `core`, no regressions in benchmarks.

## Phase 2: Aggressive taskbar takeover (explorer still running)

- Add `shell_mode = "companion" | "exclusive"`. This phase implements `companion`.
- Keep the taskbar dead persistently: re-hide on the `TaskbarCreated` broadcast, handle explorer restarts and crashes, handle new monitors and DPI changes.
- Watchdog: a tiny supervisor process (or mode of the same exe) that restores the taskbar if the main process dies. Make this opt-in/out via config, default on.
- Escape hatch: `--restore-shell` CLI flag and a documented key chord that quits and restores.
- Take over remaining taskbar jobs: taskbar button flashing/progress, jump to app on notification, multi-monitor docks.

**Exit:** the real taskbar never reappears during normal use; killing the process restores it within a second.

## Phase 3: Panel mode

- Modes: `dock` (floating, auto-hide), `panel` (docked to an edge, reserves work area), `center` (launcher only).
- Register panel as an AppBar (`ABM_NEW`, `ABM_QUERYPOS`, `ABM_SETPOS`, `ABM_REMOVE`) so maximized windows respect it. Optional `ABM_SETAUTOHIDEBAR`.
- Configurable edge (bottom/top/left/right), thickness, and monitor(s). Handle DPI and monitor hotplug.
- Panel content configurable via the existing TOML `surfaces` system.

**Exit:** panel works on all four edges and multi-monitor, maximized windows never overlap it, clean removal on quit.

## Phase 4: Launcher replaces Start

- Low-level keyboard hook (`WH_KEYBOARD_LL`) to claim the bare Win key. Chords (Win+E, Win+R, Win+D, Win+L, etc.) must pass through unchanged.
- Hook must be fast and never block; keep it on its own thread with a hard time budget.
- Fast launcher workflow: direct typed power commands (`lock`, `sleep`, `restart`, `shutdown`, `power`, `sign out`), settings search, files, calculator.
- Keep the existing grammar (`?files`, calc, `w window`, settings commands, media, etc.).
- Neutralize the built-in Start experience (StartMenuExperienceHost) on bare Win release via synthetic key intercept.

**Exit:** pressing Win opens the launcher; the stock Start never appears; chords still work.

## Phase 5: Native Direct2D Consolidation (Verified & Slint Removed)

- Benchmarked native Direct2D (`ui-d2d`) against alternative toolkit backends (Skia/Vello/Software) in `bench/BACKENDS.md`.
- Direct2D won decisively across every budget (~20 MB private bytes, 0% idle CPU/GPU, <3 MB binary size, zero third-party UI runtime).
- Removed `ui-slint` and converted `leanshell-settings` (`settings/`) to 100% native Win32 + Direct2D, unifying the entire workspace under one zero-dependency native graphics stack (`d2d1.dll`, `dwrite.dll`, `dwmapi.dll`).

**Exit:** 100% native Direct2D workspace (`leanshell.exe` and `leanshell-settings.exe`), benchmark comparison recorded in `bench/BACKENDS.md`.

## Phase 6: Exclusive shell (Remediated & Verified)

LeanShell's foundation is to **OWN** the desktop shell rather than coexist as an overlay:
- Registered as shell via transactional per-user Winlogon shell value (`HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`). Validates target binary exists before overwriting, and backs up prior shell configuration to `HKCU\Software\LeanShell\BackupShell`.
- Session ownership detection: runtime detection via `GetShellWindow()` clearly distinguishes active exclusive ownership from companion mode, preventing duplicate desktop spawns or re-dispatching startup apps when toggling settings in an Explorer session.
- CLI flags implemented: `--register-shell`, `--unregister-shell`, `--restore-shell`.
- Coordinated multi-step recovery: signals running instances via `WM_APP_QUIT`, restores work areas, clears taskbar window clipping regions via `SetWindowRgn`, restores HKCU shell, and spawns `explorer.exe` if absent.
- Emergency safe mode: holding Shift at logon reverts HKCU Winlogon shell and immediately falls back to Explorer. Emergency handoff on shutdown spawns Explorer to guarantee the user is never stranded.
- Standalone work area reservation: manages monitor work areas directly via `SystemParametersInfoW(SPI_SETWORKAREA, ...)` when Explorer is absent, ensuring maximized windows respect the panel.
- Primary `Shell_TrayWnd` host: hosts the notification area independently without relying on Explorer, and absorbs AppBar requests safely.
- Robust logon startup applications: parses command lines into executable targets and argument strings, handles quoted paths with flags, expands `REG_EXPAND_SZ` variables, initializes COM apartment, and respects the RunOnce `!` completion deferral contract.
- Desktop background window (`HWND_BOTTOM`) with native wallpaper rendering (`IShellItemImageFactory` + `PaintDesktop` fallback) and right-click desktop menu.
- Explorer remains available on demand for file browsing (`Win+E`, folder paths).
- Documented testing instructions in `docs/exclusive-shell.md`.

**Exit:** clean sign-in boots straight into LeanShell with no explorer.exe shell; holding Shift falls back safely; verified in local Windows 11 VM (`docs/vm-testing.md`).

## Phase 7: ExperienceHost Neutering, Explorer Auto-Reaper & Native Shell Chords (Pillar 1 — Implemented)

Eliminate background Microsoft shell bloat and restore missing OS shortcuts natively without `explorer.exe` or `ShellExperienceHost.exe`:
- **Reversible IFEO Host Neutering:** Block bloated Shell Experience hosts (`StartMenuExperienceHost.exe`, `SearchHost.exe`, `Widgets.exe`, `WidgetService.exe`, `ShellExperienceHost.exe`, `CrossDeviceService.exe`) via `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Image File Execution Options\<exe>` (`Debugger = "systray.exe"`) with transactional backup in `HKCU\Software\LeanShell\BackupIFEO`, CLI flags (`--neuter-hosts`, `--restore-hosts`), and automatic rollback inside `--restore-shell` and `scripts/restore-shell.ps1`.
- **On-Demand Explorer Auto-Reaper:** Allow `explorer.exe` to launch strictly for file browsing (`CabinetWClass` / `ExploreWClass`), monitor window destruction via `SetWinEventHook`, verify no active file operation dialogs (`OperationStatusWindow` / `#32770`) remain, and cleanly terminate idle background `explorer.exe` after a brief debounce.
- **Native Shell Chords:** Handle `Win+E` (Files), `Win+R` (Run via launcher `> `), `Win+D` (Show Desktop toggle), `Win+I` (Settings), and `Win+X` (Quick Power menu in launcher) directly in `WH_KEYBOARD_LL` + `WinKeyTracker` when LeanShell owns the shell or hides the taskbar.
- **Direct2D Volume/Mute OSD:** Intercept hardware media volume keys (`VK_VOLUME_UP`, `VK_VOLUME_DOWN`, `VK_VOLUME_MUTE`) and render an ultra-lightweight, zero-idle-cost Direct2D pill overlay that works in Dock, Panel, and Center modes.

**Exit:** zero ExperienceHost processes running, `explorer.exe` self-reaps after closing its last folder window, `Win+E/R/D/I/X` + volume OSD work natively without Explorer, and `--restore-shell` / `restore-shell.ps1` restore everything cleanly.

## Phase 8: Hybrid Window Management Suite (Pillar 2 — Implemented)

First-class **Mouse + Keyboard** window management (Track T4):
- **Direct2D `Alt+Tab` & `Win+Tab` Switchers (`src/wm.rs`):** Zero-poll Direct2D switchers replacing Explorer's Task View and `win32k` switchers, supporting keyboard cycling, mouse hover/click selection, clickable `[×]` close buttons, middle-click close, scroll wheel, and Task View Virtual Desktop pills + `Tiling: On/Off` header button.
- **Mouse & Keyboard Snap/Tiling (`core/src/wm.rs`, `platform-win/src/wm.rs`):** `Win+Arrow` snap state machine (halves, quarters, maximize, restore, minimize), mouse drag-to-edge/corner Direct2D snap preview overlay (`LeanShell.SnapOverlay`), per-monitor Master-Stack dynamic tiling (`Win+T` / menu / launcher) with drag-to-swap slot preview and mouse border resize ratio updates, and DWM invisible frame margin compensation (`DWMWA_EXTENDED_FRAME_BOUNDS`).
- **DWM Window Styling:** Configurable active foreground window DWM border color (`wm_borders`, `wm_border_color`) via `DwmSetWindowAttribute(DWMWA_BORDER_COLOR)`, reset cleanly on shutdown and `--restore-shell`.

**Exit:** mouse and keyboard workflows can tile, snap, switch, and manage virtual desktops seamlessly without `explorer.exe`.

## Phase 9: Native Notifications, Flyouts & Utilities (Pillar 3)

Replace remaining `ShellExperienceHost` and `TextInputHost` surfaces in Direct2D (Track T6):
- **Direct2D Toast & Balloon Notifications:** Intercept tray balloons and system notifications into lightweight Direct2D popups plus an interactive Notification History surface.
- **Interactive Clock/Calendar & Quick-Settings Flyouts:** Clicking the panel clock/status area opens LeanShell's Direct2D calendar, clock, and quick-controls flyout.
- **Built-in Clipboard History (`Win+V`) & Screen Snip (`Win+Shift+S`):** Event-driven clipboard ring (`AddClipboardFormatListener`) inside the launcher and lightweight native snip overlay without `ScreenSketch`/`TextInputHost` overhead.

**Exit:** daily-driver notifications, calendar, clipboard history, and screenshots work natively with zero UWP/WebView hosts.

## Phase 10: Raw NTFS MFT/USN File Indexer & Reversible Service Diet (Pillar 4)

Remove dependency on `WSearch` and strip background Windows services safely (Tracks T2 & T5):
- **Built-in NTFS MFT/USN Journal Indexer:** Direct `FSCTL_ENUM_USN_DATA` initial volume scan + `FSCTL_READ_USN_JOURNAL` incremental updates for instant full-drive offline search with near-zero idle memory/CPU.
- **Tiered Reversible Service Profiles:** Inventory idle wakeups/RAM and ship opt-in reversible service profiles (`shell-redundant`, `telemetry-bloat`, `gaming-ultra-lean`) with transactional backup and `--restore-services` / `--restore-shell` rollback, never touching Defender, Windows Update, crypto, RPC/DCOM, or boot-critical services by default.

**Exit:** `WSearch` disabled while file search beats stock latency; tiered service profiles measurably cut idle wakeups/RAM and restore atomically.

## Phase 11: Rich Taskbar Protocol, DWM Previews & Desktop Surface (Pillar 5)

Polish the panel/dock and desktop into a complete daily-driver shell:
- **DWM Live Window Thumbnail Previews:** Hovering dock/panel icons displays zero-copy `DwmRegisterThumbnail` previews with mouse-accessible close/minimize controls.
- **`ITaskbarList3` & Window-Flash Protocol:** Support progress bars, overlay badges, and taskbar button flashing directly on dock/panel app slots.
- **Rich Panel Bar Widgets & Upgraded Desktop Surface:** Pin live workspace pills, media ticker, CPU/RAM/network sparklines, and notification bell onto panel edges; upgrade desktop background with per-monitor DPI wallpaper scaling and styled Direct2D context menu.

## Cross-cutting tracks

These run alongside the phases. Each item lands in the earliest phase that needs it.

### T1: Internal/undocumented API layer

The fork deliberately uses internal Windows APIs where they beat documented ones. Contain the risk:

- All internal APIs live in `platform-win/internal/`, one module per API. Nothing else calls them directly.
- Each module has runtime feature detection (build number plus probing), a documented-API or no-op fallback, and an entry in `docs/internal-apis.md` (what it does, which builds it was verified on, how it fails).
- Likely candidates: virtual desktop COM interfaces (GUIDs change between builds), composition/blur attributes, tray and notification internals, the taskbar button protocol (`TaskbarCreated`, taskbar list interfaces), Start/Alt+Tab host processes.
- Compatibility matrix: Windows 11 releases plus an Insider build, smoke-tested in VMs. On an unsupported build the fork degrades and never crashes.

### T2: Own file index (Fallback Scan Implemented; NTFS MFT/USN in Phase 10)

Launcher file search currently relies on the Windows Search index / Everything with user-folder offline fallback. Phase 10 replaces `WSearch` entirely:
- Integrated zero-allocation offline directory scanning across user folders (`Desktop`, `Documents`, `Downloads`, etc.) as an instant local fallback when `WSearch` is absent or stopped.
- Phase 10 target: memory-mapped NTFS MFT (`FSCTL_ENUM_USN_DATA`) + USN change journal (`FSCTL_READ_USN_JOURNAL`) index for full-drive offline coverage.

### T3: Session, startup, host neutering & shell duties (Implemented + Extended in Phase 7)

Needed for exclusive mode:
- Startup apps (Run, RunOnce with `!` deferral, Startup folders) on background thread with COM apartment and LUID session guard.
- Safe-mode Shift fallback and transactional Winlogon registry registration with backup.
- Reversible IFEO neutering of bloated Shell Experience hosts and on-demand Explorer auto-reaping (Phase 7).
- Native Windows Shell context menus: `show_shell_context_menu` in `platform-win/src/shell_menu.rs` binding to `IShellItem` and `IContextMenu`.
- Minimal desktop background window (`HWND_BOTTOM`) with native wallpaper rendering and right-click desktop menu.

### T4: Window management (Phase 8)

- First-class Mouse + Keyboard workflow: drag-to-snap zones, interactive resize, `Win+Arrow` snapping, optional per-workspace dynamic tiling.
- Direct2D `Alt+Tab` and `Win+Tab` Task View replacements, virtual desktop switching UI, and DWM border color/corner attributes.

### T5: Services diet (Phase 10)

Inventory, measure, decide, replace, one service at a time:

- Inventory script: on a clean VM, list services with idle CPU, wakeups and memory.
- Tiered reversible profiles (`shell-redundant`, `telemetry-bloat`, `gaming-ultra-lean`) with purpose, dependents, cost, and exact restore commands (`--restore-services` and `--restore-shell`).
- Never touched by default: security (Defender, firewall), Windows Update, crypto, RPC/DCOM, and anything needed to boot or sign in.

### T6: Notifications, OSDs and system flyouts (Phases 7 & 9)

- Phase 7: Direct2D Volume/Mute OSD on hardware volume keys.
- Phase 9: Direct2D toast popups, notification history, clock/calendar flyout, clipboard history (`Win+V`), and native screen snip (`Win+Shift+S`).

### T7: Customization

Theme files, TOML surfaces and widgets (clock, CPU, media, workspaces), config hot-reload, per-monitor config, keybinding config. Everything must fit the budgets.

### T8: Measurement and diagnostics (Implemented `--diagnose`)

- Idle wakeups/sec as a first-class metric (ETW or `perf-trace`), plus working set vs private bytes, handles and GPU usage.
- Automated "idle for 60 s" benchmark in a VM.
- Implemented `--diagnose` CLI command: inspects live session ownership (`GetShellWindow`), Winlogon registry registration, backup shell, IFEO host neutering status, active taskbars, and monitor work areas.
- Coordinated shell recovery via `--restore-shell` and watchdog supervisor.

### T9: Packaging, signing, auto-update & security (Native Setup, MSIX & Auto-Updater Implemented)

- **Native Win32 + Direct2D Setup (`setup/` → `leanshell-setup.exe`):** Machine-wide `%ProgramFiles%\LeanShell` installer with UAC elevation, Windows 11 Installed Apps (`HKLM\...\Uninstall\LeanShell`), Start Menu shortcuts, WinGet `--silent` / `--uninstall` flags, optional Exclusive Shell + IFEO host neutering, and un-elevated desktop launch (`explorer.exe`).
- **Windows 11 MSIX + `.appinstaller` (`packaging/msix/`):** `runFullTrust` + `allowElevation` MSIX manifest, `.appinstaller` XML auto-update descriptor, and `build-msix.ps1`.
- **Built-In Background Auto-Updater (`core/src/updater.rs`, `settings/src/update.rs`, `src/app.rs`):** Configurable `auto_update = "auto" | "notify" | "off"` + `update_check_hours`, SHA-256 verification, zero-taskbar-flash hot restart (`WM_APP_QUIT` `WPARAM(1)` + `disarm_for_hot_restart`), `%TEMP%` self-handoff + elevated `--apply-staged-update`, and automatic `.old` rollback if the new binary exits early.
- **Future Hardening:** EV/OV Authenticode signing for SmartScreen reputation, UI Automation tree exposure, high contrast, IME polish.

## Resolved Design Decisions

- **Privilege & Invasiveness Boundary:** Full system takeover is sanctioned (admin privileges for `HKLM` IFEO redirection, Windows Services, raw NTFS MFT/USN reading), contingent on transactional backups under `HKCU\Software\LeanShell\Backup*` and atomic reversal in `--restore-shell` and `scripts/restore-shell.ps1`.
- **Renderer Architecture:** 100% native Win32 + Direct2D (`ui-d2d`, native D2D surfaces, and native D2D `leanshell-settings`) is the sole UI layer; Slint was evaluated and removed completely from the repository.
- **Explorer & Host Posture:** Block `StartMenuExperienceHost`, `SearchHost`, `Widgets`, `WidgetService`, `ShellExperienceHost`, and `CrossDeviceService` via IFEO; allow `explorer.exe` strictly on-demand for folder browsing and auto-reap it once its last folder window closes.
- **Window Management UX:** Hybrid floating + optional per-workspace tiling where both keyboard shortcuts and mouse interaction are first-class citizens.
