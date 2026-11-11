# Changelog

User-facing changes in each release. Each release on GitHub lists these notes and its pull requests.

## 0.0.5 - 2026-11-10

### Stabilization, Keyboard Hook Fixes & Centered Panel Alignment

- **Windows Key Neutralization & Unstuck Modifier Fix**: Resolved the Windows key bug where bare Win tap left the Win modifier key logically stuck down in the OS keyboard buffer (causing keystrokes in search to trigger Windows shortcuts like Win+E, Win+R, Win+D). The low-level keyboard hook now injects a Control masking key while Win is down to suppress StartMenuExperienceHost, and safely passes the Win keyup to `CallNextHookEx` so Windows cleanly resets its modifier state.
- **Panel Status Layout & Scaling Fix**: Fixed layout calculation in panel mode where monitor width in DIPs was erroneously multiplied by icon layout scale, which had broken the positioning of tray, volume, and clock status items when changing `dock_icon_size`.
- **Clock & Date End Padding**: Added generous right padding (`PANEL_END_PAD = 14 DIP`) preventing the date and time from touching or overflowing the screen border in panel mode.
- **Centered App & Search Icons (`panel_align`)**: Added `panel_align = "center" | "left"` (default `"center"`) in `config.toml` and LeanShell Settings, centering the search icon and pinned/running app cluster in panel mode while keeping status items right-aligned.
- **Click Accuracy for Centered Apps**: Updated slot hit-testing and separator rendering to respect centered layout offsets.

## 0.0.4 - 2026-11-10

### Milestone 6: Exclusive Shell Mode (Boot Directly into LeanShell)

- **Exclusive Shell Mode (Phase 6)**: Registered LeanShell as Windows 11 primary user shell via per-user `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell`. Never touches machine-wide registry.
- **Fail-Safe Startup Mode**: Holding **Shift** at Windows logon/boot automatically triggers emergency safe-mode fallback, reverting HKCU Winlogon shell and launching Windows Explorer cleanly.
- **Logon Startup Applications (Track T3)**: LeanShell automatically executes user and system startup items (HKCU/HKLM Run and RunOnce keys, user and common Startup folders) on a background thread with zero boot delay.
- **Minimal Desktop Background**: Lightweight Win32 `HWND_BOTTOM` desktop surface rendering the active desktop wallpaper via native GDI `PaintDesktop` and hosting right-click desktop options.
- **CLI Shell Management**: Added `--register-shell` and `--unregister-shell` commands for seamless scripting and manual control.
- **Onboarding Mode Selector**: Added dedicated Shell Mode step ("Companion" vs "Exclusive Shell") to the first-boot onboarding wizard in Settings.

## 0.0.3 - 2026-11-10

### Milestone 5: Renderer Backends (Direct2D & Slint)

- **Pluggable Renderer Architecture (Phase 5)**: Established modular rendering backends via `leanshell_render::Renderer` with Direct2D as the reference default and an optional feature-gated Slint backend (`ui-slint`).
- **Slint Subsystem Support**: Configurable Slint subsystems supporting Skia (default high-fidelity 2D), Vello (experimental GPU compute with modern shaders), and Software CPU fallback.
- **Runtime Backend Selection**: Added `backend = "direct2d" | "slint"` and `slint_renderer = "skia" | "vello" | "software"` keys to `config.toml`.
- **DWM Backdrop Integration**: Re-implemented Acrylic, Mica, and solid composition backdrops with Slint windows via raw HWND integration.
- **Settings App Engine Selector**: Added interactive Rendering Backend and Slint Subsystem dropdown selectors to the Settings app Appearance page.
- **Comprehensive Benchmarks**: Documented memory consumption, binary size delta, first-frame latency, and licensing trade-offs across all backends in `bench/BACKENDS.md`.

## 0.0.2 - 2026-11-10

### Milestone 4: Launcher Replaces Start & Panel Polish

- **Bare Windows Key Interception (Phase 4)**: Added a dedicated, non-blocking low-level keyboard hook (`WH_KEYBOARD_LL`) that claims the bare Windows key to open the LeanShell launcher instead of the stock Windows Start menu.
- **Full Chord Transparency**: All Windows key chords (Win+E, Win+R, Win+D, Win+L, Win+X, Win+Tab, Win+Shift+S, Win+I, etc.) pass through completely unaltered.
- **Direct Power Actions**: Typing power actions (`lock`, `sleep`, `restart`, `reboot`, `shutdown`, `power off`, `sign out`, `log off`, or `power`) instantly lists and executes Win32 power management operations with safe confirmation for destructive actions.
- **Panel Edge & Geometry Fixes**: Complete multi-edge placement (Bottom, Top, Left, Right) with full-width monitor expansion and right-aligned system tray, volume, and clock.
- **Panel Context Menu**: Right-clicking empty space on the panel or dock provides instant access to LeanShell Settings, taskbar restoration, and Windows taskbar settings.
- **Configurable Win Key Claim**: `claim_win_key` setting in `config.toml` and Settings app to toggle Win key interception.

## 0.0.1 - 2026-11-10

### LeanShell Initial Early Experimental Release

- **LeanShell Rebranding & Core Philosophy**: Forked from Wisp with strict ~0% idle resource use goals.
- **Core Decoupling (Phase 1)**: Pure, toolkit-independent `core` crate for search grammar, calculator, config, history, and models. Agnostic `render` crate with swappable `Renderer` trait.
- **Aggressive Taskbar Takeover (Phase 2)**: Persistent `TaskbarCreated` suppression, `--restore-shell` CLI recovery flag, watchdog supervisor configuration, and emergency recovery hotkey (Ctrl+Alt+Shift+Escape).
- **Panel Mode (Phase 3)**: Persistent edge-docked taskbar mode (`display_mode = "panel"`) that reserves screen work areas via AppBar across all 4 display edges (Bottom, Top, Left, Right).

## Unreleased

### New

- Pin apps to the dock: right-click an app and choose Pin to dock. Pinned apps stay in the dock, before running apps, and start with a click. (#54, by @SorbetMango, fixes #31)
- Apps with more than one window open show a second dot under their dock icon. (#55)
- The autohide dock starts to slide in sooner when you move the pointer to the bottom edge. (#25, by @RayTrunk, fixes #20)

## 0.6.1 - 2026-11-09

### Fixes

- The release zip has Wisp's files at its top level, so the install path stays the same across updates. (#50)
- Wisp installed with winget updates through `winget upgrade` instead of the Update button in Settings. (#50)

## 0.6.0 - 2026-11-09

### New

- **See an app's windows.** Hover an app with several windows to list them. Click one to switch to it, or middle-click to close it. (#21, by @JulienMaille)
- **Choose the dock's size.** Set the app icon size in Settings, from 24 to 40. The dock's height, spacing and status area scale with it. (#39, by @JulienMaille)

### Fixes

- Wisp hides the taskbar reliably when it starts. Before, the taskbar sometimes stayed visible for several seconds, especially with a second screen. (#30, by @RayTrunk, fixes #28)
- The Windows taskbar no longer flashes when you switch the dock between autohide and Keep the dock visible. (#24, by @RayTrunk, fixes #18)
- Notification icons keep reaching Wisp when Explorer shows its taskbar. (#38, fixes #19)
- Right-clicking the dock no longer shows a "Wisp" title strip across its top. (#34, by @RayTrunk, fixes #33)
- The best match stays selected while typing in the launcher when the mouse rests over the results. Move the mouse to highlight another result. (#36, fixes #35)
- Time searches now find English city names on non-English Windows, while still finding names in the Windows display language. (#37, fixes #26)
- The settings app is a third smaller to download (15.2 MB → 10.0 MB). (#46)
- Updates download the release zip, and Wisp tells you where to get Settings if it is missing. (#47)

## 0.5.0 - 2026-11-07

### New

- **Keep the dock visible.** A new option in Settings keeps the dock on screen and reserves its space, so maximized windows stop above it instead of going behind it. Full-screen apps still hide it, including apps that go full screen from a maximized window. (#9)
- **Click the app in front to minimize it**, like a taskbar button. Click it again to bring it back. (#14, by @JulienMaille)

### Fixes

- Wisp now hides the taskbar when you start it from the Start menu. Before, it could fail with "Could not find primary taskbar". (#8)
- Clicking an app in the dock now switches to it while Action Center, Start or search is open. (#13, by @JulienMaille, fixes #11)
- Dock icons keep their place when clicked instead of moving to the front. (#5, by @JulienMaille)
- Tray icons are sharp instead of pixelated. (#3, by @JulienMaille, fixes #2)
- Windows 10: icons and text use the Windows 10 fonts instead of drawing as boxes. (#10, for #1)
- The tray no longer comes up empty after starting Wisp from the Start menu. (#9)
- Turning "Keep the dock visible" on or off no longer freezes the dock and launcher for up to two seconds. (#9)
- Taskbar hiding retries reliably after Explorer restarts and display changes. (#8)
- An icon no longer stays raised after hovering the hidden-icons arrow. (#16)
- The "Show hidden icons" tooltip no longer covers its own flyout. (#17)
- Configs from older versions pick up new settings from the defaults. Configs from Wisp 0.3 keep the taskbar as before. (#8)

### Thanks

@JulienMaille contributed their first pull requests to Wisp in this release: #3, #5, #13 and #14.

## Earlier releases

See the [releases page](https://github.com/SumirSeth/wisp/releases) for 0.4.2 and earlier.
