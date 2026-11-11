# Roadmap

Working name: **LeanShell** (fork of [SumirSeth/wisp](https://github.com/SumirSeth/wisp), MIT). Rename freely.

**Goals:**
- **Total Shell Replacement:** Completely replace the Windows 11 shell so that `explorer.exe`'s taskbar and `StartMenuExperienceHost.exe` never even attempt to launch on boot/logon. Explorer remains available strictly on-demand for file management.
- **Both Taskbar & Dock Modes:** Support both a persistent **Taskbar Mode** (edge-docked native Windows AppBar reserving work area for maximized windows) and a floating **Dock Mode** (auto-hiding, slide-up), both morphing into the launcher.
- **Multi-Backend Architecture with Slint Option:** Decoupled `core` behind a swappable `Renderer` trait. Reference default is Direct2D (`ui-d2d`) for minimum resource footprint; optional Slint (`ui-slint`) backend supporting Skia and Vello renderers.
- **~0% Idle Resource Use:** Native Win32 message-driven design with no web views, no runtimes, and zero background polling.

**Principle:** every phase ships something usable and keeps a recovery path. The riskiest step (replacing explorer as the shell) comes last.

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

## Phase 5: Renderer backends (Implemented)

- Direct2D stays the default and the reference implementation.
- Added `ui-slint` backend behind the `slint` Cargo feature and `backend = "direct2d" | "slint"` config key.
- Within the Slint backend, default renderer **Skia**, secondary **Vello**, plus **Software** fallback. Select via config (`slint_renderer = "skia" | "vello" | "software"`) and `SLINT_BACKEND` environment variable.
- DWM blur/backdrop (Acrylic/Mica) integrated with Slint window raw HWND handles.
- Benchmarked each backend/renderer combo on idle RAM, idle CPU, first-frame latency, binary size delta, and documented trade-offs in `bench/BACKENDS.md`.

**Exit:** both backends selectable at runtime or build time, benchmark table in `bench/BACKENDS.md`, documented trade-offs.

## Phase 6: Exclusive shell (explorer not running)

Highest risk. Do all testing in the VM or a separate user account.

- Register as the shell via the per-user Winlogon shell value (never machine-wide).
- Provide what explorer provided: desktop window (wallpaper, optional icons), tray host (already done), notifications, clock/calendar, volume/network/battery flyouts.
- Explorer remains available on demand for file browsing; verify that behavior.
- Research which Windows 11 flyouts and shell experiences depend on explorer and need replacing.
- Boot-to-shell test, sign-out/shutdown flow, lock screen return, fast user switching.
- Safe mode: holding a modifier at logon falls back to explorer; recovery script and docs.
- Take on everything explorer did at logon: run startup apps (Run/RunOnce keys, startup folders, logon tasks), session and power events, Alt+Tab and Task View replacements (see T3, T4, T6).
- Goal: the default shell never launches and the system boots directly into the fork.

**Exit:** a clean sign-in boots straight into the fork with no explorer.exe shell; everything in daily use works or has a documented gap.

## Phase 7: Long term

- Window borders and snapping (T4).
- Replace most Windows services over time (T5), one at a time, each opt-in, each with a restore path.
- Plugin/surface ecosystem on the TOML surface system (T7).

## Cross-cutting tracks

These run alongside the phases. Each item lands in the earliest phase that needs it.

### T1: Internal/undocumented API layer

The fork deliberately uses internal Windows APIs where they beat documented ones. Contain the risk:

- All internal APIs live in `platform-win/internal/`, one module per API. Nothing else calls them directly.
- Each module has runtime feature detection (build number plus probing), a documented-API or no-op fallback, and an entry in `docs/internal-apis.md` (what it does, which builds it was verified on, how it fails).
- Likely candidates: virtual desktop COM interfaces (GUIDs change between builds), composition/blur attributes, tray and notification internals, the taskbar button protocol (`TaskbarCreated`, taskbar list interfaces), Start/Alt+Tab host processes.
- Compatibility matrix: Windows 11 releases plus an Insider build, smoke-tested in VMs. On an unsupported build the fork degrades and never crashes.

### T2: Own file index

Launcher file search currently relies on the Windows Search index. If services get trimmed (T5), that breaks.

- Build a small index from NTFS enumeration plus the USN journal for incremental updates. Compact on-disk format, memory-mapped, zero idle wakeups.
- Opt-in, with Windows Search as the fallback.
- Exit: query latency at or below the current path, idle cost ~0.

### T3: Session, startup and shell duties

Needed for exclusive mode:

- Startup apps, logon/logoff/shutdown, lock/unlock, sleep/hibernate/resume, display changes, fast startup, session notifications.
- Opening items properly: file associations and Open With, context menus for launcher results, drag and drop (already in Wisp for results), default-app handling.
- A minimal desktop: wallpaper, optional icons, right-click menu.

### T4: Window management

- Window borders (DWM border color and corner attributes), snapping and layouts, optional tiling.
- Alt+Tab and Task View replacements, plus a virtual desktop UI (Wisp already has scroll-to-switch).
- Verify what is lost in exclusive mode, since some of these are hosted by explorer-related processes.

### T5: Services diet (long-term goal: replace most Windows services)

Inventory, measure, decide, replace, one service at a time:

- Inventory script: on a clean VM, list services with idle CPU, wakeups and memory.
- A profile entry per service: purpose, dependents, cost, replace/disable/leave, and the restore command.
- Everything opt-in and reversible through service configuration (never deleting anything). A profile file applies changes, `--restore-services` undoes them.
- Never touched by default: security (Defender, firewall), Windows Update, crypto, RPC/DCOM, and anything needed to boot or sign in.
- First candidates are the ones the shell itself makes redundant (search indexing once T2 lands, Start and notification hosts), then telemetry and similar background services.
- Write lean replacements where needed rather than just disabling.

### T6: Notifications and system flyouts

Needed before exclusive mode is livable: toasts, quick settings (volume, network, Bluetooth, brightness, battery), calendar/clock flyout, action center. Build on Wisp's `now` and `sound` panels, then add dedicated surfaces.

### T7: Customization

Theme files, TOML surfaces and widgets (clock, CPU, media, workspaces), config hot-reload, per-monitor config, keybinding config. Everything must fit the budgets. Scripting is a later question.

### T8: Measurement and diagnostics

- Idle wakeups/sec as a first-class metric (ETW or `perf-trace`), plus working set vs private bytes, handles and GPU usage.
- An automated "idle for 60 s" benchmark in a VM.
- Crash handling: minidumps, log file, auto-restore of the taskbar, and a `--diagnose` command.

### T9: Packaging, signing, security

- Portable zip plus installer, user-initiated updates.
- Code signing to avoid SmartScreen warnings.
- Never elevated by default. As the shell, everything the user launches inherits the fork's token, so avoid privilege surprises.
- Accessibility: UI Automation, high contrast, IME, per-monitor DPI.

## Open questions

- Which Windows 11 shell components break without explorer (answer by testing in Phase 6 prep).
- Final project name and license posture for the Slint backend.
- Whether the watchdog should be a separate binary.
- How far down the services list is worth going once the shell and file index replace the biggest idle consumers.
- Which taskbar and notification protocols apps still expect (and how much explorer provides in-process) when explorer is not running.
