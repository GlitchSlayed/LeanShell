# AGENTS.md

Instructions for AI coding agents working in this repository. Read fully before making changes. (Symlink or copy as `CLAUDE.md` if your tool expects that name.)

## Project

LeanShell (working name) is a Windows 11 shell/taskbar/Start replacement written in Rust, forked from [SumirSeth/wisp](https://github.com/SumirSeth/wisp) (MIT). It hides or replaces the taskbar, provides a dock/panel UI, and includes a built-in launcher that replaces the Start menu. See `ROADMAP.md` for phases and budgets.

The defining constraint is **~0% idle resource use**. A hidden dock should cost almost nothing in CPU and memory. Treat performance as a feature, not an afterthought.

## Principles

1. **Performance first.** Native Win32, no web views, no runtimes, no background polling. Prefer event-driven code (window messages, notifications) over timers.
2. **Never strand the user.** Any change that affects the shell, taskbar, registry shell values, or input hooks must have a tested recovery path. See Safety below.
3. **Small, reversible steps.** Prefer many small changes that each build, pass checks, and keep behavior unchanged unless the task says otherwise.
4. **Core stays pure.** Logic lives in `core` with no Win32 types and no drawing. Platform code lives in `platform-win`. Rendering lives behind the `Renderer` trait.

## Target layout

```
core/          app index, search grammar, calculator, config, window/desktop/tray models (no Win32, no UI)
platform-win/  HWND, AppBar, blur, hotkeys and hooks, tray host, shell notifications
render/        Renderer trait and frame description types
ui-d2d/        Direct2D renderer (default, reference implementation)
ui-slint/      Slint renderer (optional feature; Skia default, Vello secondary)
settings/      settings app
bench/         benchmarks, baselines, methodology
scripts/       install-nightly, restore-shell, measurement scripts
```

Until the core split lands (Phase 1), the layout is still Wisp's. Do not do the split opportunistically inside unrelated changes.

## Commands

Run all of these before declaring work done:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

If a feature-gated build exists (nightly, perf-trace, slint), also run clippy with those features, matching `.github/workflows`. Use the nightly install script to try changes without touching an installed copy.

## Safety rules (non-negotiable)

- Never write to `HKLM` or anything machine-wide. Shell registration is per-user only.
- Never run, install, or register an exclusive shell on the developer's main session. Shell-level testing happens in the dedicated local Windows 11 VM (`/home/vanshdatta/VMs/windows/windows-11.conf`, snapshot `clean-install`; see `docs/vm-testing.md`).
- Any code that hides the taskbar, registers a hook, or changes the shell value must be paired with cleanup on exit **and** on crash (watchdog or restore path), plus a mention in `scripts/restore-shell.ps1`.
- Keyboard hooks (`WH_KEYBOARD_LL`) must return fast, run on a dedicated thread, never block, and pass through every key they do not explicitly own.
- Tests must not modify the real registry, hide the real taskbar, or install hooks. Abstract those behind traits and use fakes.
- Do not add telemetry, network calls on startup, or auto-updaters that run unprompted.

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

## Long-term goals (context only, do not start unprompted)

- Boot directly into the fork with the default shell never launching.
- Replace most Windows services over time. Always opt-in, reversible through service configuration, never touching security, update, crypto, or boot-critical services by default.
- Own file index to remove the dependency on Windows Search.
- Details and sequencing are in `ROADMAP.md` (phases and tracks T1 to T9). Work on the phase or track the task names, nothing else.

## Renderers

- Direct2D (`ui-d2d`) is the default and the reference. It must always build and pass tests.
- Slint is an optional backend behind a Cargo feature. Within it, Skia is the default renderer and Vello is secondary/opt-in. Do not make Slint a default dependency.
- Do not copy Slint-specific types into `core` or `render`. The `Renderer` trait describes *what to draw*, not *how*.
- Slint's license differs from this repo's MIT license. Do not ship or vendor it without the maintainer deciding the licensing posture.

## Licensing and attribution

- Keep the original MIT `LICENSE` and Sumir Seth's copyright notice. Add, do not replace.
- Do not paste code from other projects without checking license compatibility and noting the source.

## How to work

1. For anything larger than a small fix, write a short plan first (what changes, which files, how it will be verified, what could break) and wait for confirmation if the change touches shell, hooks, or registry behavior.
2. Make minimal, focused diffs. Do not reformat unrelated code, rename things opportunistically, or bundle refactors with features.
3. Add or update tests for logic in `core`. For platform code, add tests against fakes where possible and document manual test steps otherwise.
4. Update docs (`README`, `ROADMAP.md`, config docs) when behavior or config keys change.
5. Commit messages: imperative mood, one logical change per commit, reference the roadmap phase when relevant (e.g. `panel: register AppBar on bottom edge (Phase 3)`).
6. When unsure about a Windows behavior (undocumented APIs, shell internals), say so, state your assumption, and propose a way to verify it in the VM instead of guessing.

## Definition of done

- Builds clean, fmt/clippy/tests pass.
- Budgets not regressed (numbers attached when relevant).
- Recovery path intact for anything touching shell behavior.
- Docs and config references updated.
- No stray debug output, dead code, or commented-out blocks.

## Out of scope unless asked

- Replacing Windows services (Phase 7).
- Making Slint or any GPU renderer the default.
- Adding network features, accounts, or plugin marketplaces.
