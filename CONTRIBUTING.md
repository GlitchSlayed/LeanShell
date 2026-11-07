# Contributing to Wisp

Thanks for helping. Bug reports, fixes and features are all welcome.

## Building, not just coding

> Building something has never been about writing code. When I say I built it, it means I have put effort, thought and intention behind every single thing, and got Claude to write the code because it writes better code than almost all humans do.
>
> — Sumir Seth, maintainer

Wisp is built in that spirit. A good change starts with a reason: a bug you hit, something that felt wrong, an idea you thought through. The code serves that intention.

## AI agents

AI agents are welcome. Use them as code writers, tinkerers and partners for ideas. Much of Wisp is written this way.

**You own what you submit.** The person who opens a pull request or makes a commit owns that change, whoever or whatever typed it. Be ready to explain it, defend it and fix it.

Before you open a pull request:

- **Understand every line.** If you can't explain why a line is there, it isn't ready.
- **Verify it.** Test the change on a real desktop, write tests where they fit, and run the [benchmarks](#performance). Say in the pull request how you checked it.
- **Think it through.** Consider the edge cases: other monitors, scale settings, virtual desktops, autohide on and off, Windows 10.

**Not accepted:** changes made end to end by an agent with no verifiable check, no real testing and no deliberate thought behind them. They will be closed.

## Before you start

- **Bugs:** open an issue first if it isn't already reported. Include your Windows version, what you did, what you expected and what happened.
- **Features:** open an issue or discussion first. Wisp aims to be a slim, fast dock and launcher, not a full taskbar replacement, so it helps to agree on the idea before you write code.

## Making a change

1. Fork the repository and create a branch from `main`.
2. Try your change with `scripts\install-nightly.ps1`. It installs **Wisp Nightly** beside your normal Wisp, with its own folder, config and shortcut (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Space</kbd>).
3. If your change touches anything the user sees or waits on, measure it. See [Performance](#performance).
4. Run the checks CI runs:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo clippy --workspace --all-targets --features wisp/nightly,wisp-settings/nightly,wisp/perf-trace -- -D warnings
   cargo test --workspace
   ```

5. Keep your branch rebased on `main`.

## Performance

Speed is Wisp's main feature, so a change must not make it slower. Measure every change to the dock, the launcher, search, drawing or startup **before and after**, on the same machine.

1. **Frame timings.** Build the demo with tracing, then capture on `main` and on your branch:

   ```powershell
   cargo build --release --example launcher_demo --features perf-trace
   scripts\measure-perf.ps1 -Phase before   # on main
   scripts\measure-perf.ps1 -Phase after    # on your branch
   ```

   It records the morph, typing, slide and idle scenarios at 100% and 125% scale, and saves traces and CPU and memory figures in `target\perf`.

2. **Real latency and memory.** Run [`bench/bench.ps1`](bench/bench.ps1) against your installed build. It measures shortcut to launcher, typing to results, memory and idle CPU from outside the process. Add `-SkipPowerToys` if you don't have PowerToys. Don't touch the keyboard or mouse while it runs.

3. **Stress it.** Open and close the launcher fast, sweep the pointer across the dock, and switch virtual desktops quickly, with autohide on and off. Watch for dropped frames, stalls and CPU that stays up.

Put the before and after numbers in your pull request. Any regression needs a reason that is worth it, and it's better to find another way. Frames should stay within the display's frame budget (6.9 ms at 144 Hz), and the dock should use almost no CPU when idle.

## Pull requests

- Keep each pull request to one change.
- **Write the title as a changelog line**, in plain language, such as "Minimize an app when its dock icon is clicked in front". Release notes list pull requests by title.
- In the description, say what changed, why, and how you verified it, with before and after numbers for anything performance-related. Link the issue it fixes with `Fixes #123`.
- Write commit messages the same way: a short summary line, then the reason for the change.
- Add a user-facing change to the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md).

## Writing style

Write code comments, docs and UI text in plain, active language. Use short sentences. Name what the user sees and does. For example, write "The dock hides over full-screen apps", not "Fullscreen detection suppresses dock visibility".

## License

By contributing, you agree that your contributions are licensed under the [MIT License](LICENSE).
