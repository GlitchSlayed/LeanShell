# AGENTS.md

Instructions for AI coding agents working in this repository. Read fully before making changes. (Symlink or copy as `CLAUDE.md` if your tool expects that name.)

## Project

LeanShell (working name) is a Windows 11 shell/taskbar/Start replacement written in Rust, forked from [SumirSeth/wisp](https://github.com/SumirSeth/wisp) (MIT). It hides or replaces the taskbar, provides a dock/panel UI, and includes a built-in launcher that replaces the Start menu. See `ROADMAP.md` for phases and budgets.

The defining constraint is **~0% idle resource use**. A hidden dock should cost almost nothing in CPU and memory. Treat performance as a feature, not an afterthought.

## Principles

1. **Performance first.** Native Win32 + Direct2D, no web views, no runtimes, no background polling. Prefer event-driven code (window messages, `SetWinEventHook`, `FSCTL_READ_USN_JOURNAL`, notifications) over timers.
2. **Never strand the user.** Any change that affects the shell, taskbar, registry shell values, IFEO host redirection, Windows services, or input hooks must have a tested recovery path. See Safety below.
3. **Small, reversible steps.** Prefer many small changes that each build, pass checks, and keep behavior unchanged unless the task says otherwise.
4. **Core stays pure.** Pure logic and state machines live in `core` with no Win32 types and no drawing. Platform mechanisms live in `platform-win`. Direct2D is the sole active UI layer.

## Target layout

```
core/          app index, search grammar, calculator, config, window/desktop/tray models, host/service policies (no Win32, no UI)
platform-win/  HWND, AppBar, blur, hotkeys and hooks, tray host, shell/IFEO/reaper/service/USN OS integration
render/        Renderer trait and frame description types
ui-d2d/        Direct2D renderer (sole UI layer across launcher, dock, OSDs, flyouts, WM overlays)
settings/      native Win32 + Direct2D settings and onboarding app
setup/         native Win32 + Direct2D setup installer, uninstaller, and elevated swap helper
packaging/     WinGet manifests and Windows 11 MSIX + .appinstaller packaging
bench/         benchmarks, baselines, methodology
scripts/       install-nightly, restore-shell, measurement scripts
```

Extract new pure logic into `core` and new Win32 system subsystems into `platform-win` rather than growing `src/` monolithically. Do not do unrelated large-scale file moves opportunistically inside focused feature changes.

## Commands

Run all of these on Windows (CI runs `windows-latest`):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

On the Linux development host, verify formatting, pure `core` tests, and Windows cross-compilation of the entire native Win32 + Direct2D workspace:

```
cargo fmt --all --check
cargo test --package leanshell-core
cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu -- -D warnings
```

If a feature-gated build exists (nightly, perf-trace), also run clippy with those features, matching `.github/workflows`. Use the nightly install script to try changes without touching an installed copy.

## Safety rules (non-negotiable)

- **Reversible Deep System Takeover:** Admin-privileged `HKLM` mutations (IFEO ExperienceHost suppression, tiered Windows service configuration) and raw NTFS volume reads (MFT/USN indexing) are permitted when **every** mutation is transactionally backed up under `HKCU\Software\LeanShell\Backup*` and cleanly undone by both `leanshell.exe --restore-shell` and `scripts/restore-shell.ps1`.
- **Winlogon Shell Registration:** Shell registration uses per-user `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Winlogon\Shell` with transactional backup in `BackupShell` and Shift-held safe-mode bypass at logon.
- **VM-only Shell Testing:** Never run, install, or register an exclusive shell or system takeover on a developer's main Windows session. Shell-level testing happens exclusively in the dedicated local Windows 11 VM (`/home/vanshdatta/VMs/windows/windows-11.conf`, snapshot `clean-install`; see `docs/vm-testing.md`).
- **Fail-Safe Recovery:** Any code that hides the taskbar, registers a hook, alters IFEO keys, modifies services, or owns the desktop shell must pair cleanup on exit **and** on crash (watchdog / supervisor process) with full rollback in `--restore-shell` and `scripts/restore-shell.ps1`.
- **Keyboard Hooks:** `WH_KEYBOARD_LL` must return fast, run on a dedicated thread, never block, and pass through every key or chord it does not explicitly claim.
- **Pure Unit Tests:** Tests must never modify the real registry, kill real processes, hide the real taskbar, modify real services, or install OS hooks. Abstract those behind pure decision logic in `core` or trait fakes.
- **Zero Telemetry / Background Network:** Do not add telemetry, network calls on startup, or unprompted auto-updaters.

## Code conventions

- Use the Win32 bindings already in `Cargo.toml`. Do not add a second binding crate.
- Every `unsafe` block gets a `// SAFETY:` comment stating the invariant. Wrap raw handles in RAII types that close or destroy on drop.
- No allocation or locking on hot paths: window procedure, hooks, per-frame drawing, keystroke-to-results. If you must allocate, justify it in a comment.
- No `unwrap()`/`expect()` outside tests and truly unreachable states. Surface errors; the shell must degrade gracefully.
- Prefer message-driven design. Do not add polling threads or periodic timers without measuring and justifying the idle cost.
- Dependencies: justify every new crate (size, idle cost, maintenance). Prefer std and existing deps. Check binary size delta.
- Keep `core` free of `windows`/UI dependencies. If you need a platform capability in core, define a trait in core and implement it in `platform-win`.

## Performance workflow

- Budgets are in `ROADMAP.md`, baselines in `bench/BASELINE.md`.
- For any change touching startup, rendering, search, hooks, or the window procedure: measure before and after (the repo's bench scripts, `perf-trace` feature) and include numbers in the PR description.
- A change that regresses a budget needs explicit sign-off from the maintainer; do not merge it quietly.

## Internal and undocumented Windows APIs

Using them is part of the project goal, with rules:

- They live only in `platform-win/internal/`, one module per API, never called from other crates.
- Every module must detect availability at runtime (build number and probing), provide a documented-API or no-op fallback, and fail safely. An unsupported Windows build must degrade, not crash or hang.
- Document each one in `docs/internal-apis.md`: purpose, builds verified, failure behavior.
- State your assumptions about undocumented behavior explicitly and verify them in the VM; do not guess GUIDs, struct layouts, or message IDs from memory.

## Architecture Direction (Phases 7–11)

- **Total Shell Replacement + Host Elimination:** Boot directly into LeanShell (`shell_mode = "exclusive"`), suppress stock Windows Shell Experience hosts (`StartMenuExperienceHost`, `SearchHost`, `Widgets`, `ShellExperienceHost`) via reversible IFEO redirection, allow `explorer.exe` strictly on-demand for file browsing, and auto-reap Explorer after its last folder window closes.
- **First-Class Hybrid Window Management:** Native Direct2D `Alt+Tab` and `Win+Tab` switchers, mouse + keyboard snap/tiling per workspace, and DWM active border styling.
- **Native System UX:** Direct2D Volume/Brightness OSDs, toast notifications + history, clock/calendar flyout, clipboard history (`Win+V`), and native screen snip (`Win+Shift+S`).
- **Own File Index & Reversible Service Diet:** Raw NTFS MFT/USN change-journal indexing built into LeanShell so `WSearch` can be disabled, plus tiered reversible Windows service profiles (`shell-redundant`, `telemetry-bloat`, `gaming-ultra-lean`), never touching security, update, crypto, or boot-critical services by default.
- Details and sequencing are in `ROADMAP.md` (Phases 7–11 and tracks T1 to T9).

## Renderers

- **100% Native Direct2D (`ui-d2d` and native Win32 + D2D surfaces)** is the sole rendering engine across the entire workspace (launcher, dock, OSDs, flyouts, toasts, window-management overlays, and `settings/`).
- No third-party GUI toolkits, web views, GPU shader runtimes, or declarative non-native UI frameworks may be introduced. Everything uses built-in Windows `d2d1.dll`, `dwrite.dll`, and `dwmapi.dll`.

## Licensing and attribution

- Keep the original MIT `LICENSE` and Sumir Seth's copyright notice. Add, do not replace.
- Do not paste code from other projects without checking license compatibility and noting the source.

## How to work

1. For anything larger than a small fix, write a short plan first (what changes, which files, how it will be verified, what could break) and wait for confirmation if the change touches shell, hooks, IFEO, services, or registry behavior.
2. Make minimal, focused diffs. Do not reformat unrelated code, rename things opportunistically, or bundle refactors with features.
3. Add or update tests for logic in `core`. For platform code, add unit tests against pure decision helpers/fakes and document VM test steps.
4. Update docs (`README`, `ROADMAP.md`, `docs/exclusive-shell.md`, config docs) when behavior or config keys change.
5. Commit messages: imperative mood, one logical change per commit, reference the roadmap phase when relevant (e.g. `hosts: add reversible IFEO host neutering (Phase 7)`).
6. When unsure about a Windows behavior (undocumented APIs, IFEO, raw NTFS USN, shell internals), say so, state your assumption, and verify in the local Windows 11 VM instead of guessing.

## Definition of done

- Builds clean, fmt/clippy/tests pass.
- Budgets not regressed (numbers attached when relevant).
- Recovery path intact (`--restore-shell` and `scripts/restore-shell.ps1`) for anything touching shell, IFEO, hooks, or services.
- Docs and config references updated.
- No stray debug output, dead code, or commented-out blocks.

## Out of scope unless asked

- Permanently deleting Windows components or disabling Defender/Windows Update/boot-critical services by default.
- Adding third-party UI toolkits, GPU shader frameworks, or webview/Electron renderers.
- Adding network telemetry, accounts, or plugin marketplaces.
