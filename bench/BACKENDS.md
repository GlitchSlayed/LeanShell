# LeanShell Renderer Evaluation & Benchmarks

This document records the empirical measurements, performance budgets, and architectural trade-offs evaluated in **Phase 5**, which led to dropping Slint completely and consolidating the entire LeanShell workspace (`leanshell.exe` and `leanshell-settings.exe`) on **100% native Win32 + Direct2D**.

## Executive Summary

- **Native Win32 + Direct2D (`ui-d2d` and native D2D surfaces)** is the **sole renderer** across every LeanShell binary and surface (launcher, dock/panel, OSDs, flyouts, switchers, snap overlays, and `leanshell-settings`), meeting our ~0% idle CPU, minimal memory (~19–21 MB private bytes), and <3 MB binary budgets under MIT licensing.
- **Slint (`ui-slint` and `settings/*.slint`)** was benchmarked and **completely removed** from the repository: every tested Slint backend (Skia, Vello, Software) exceeded LeanShell's memory, binary size, and latency targets while introducing GPLv3/royalty-free licensing constraints.

---

## Historical Benchmark Comparison Table

Measurements taken in the standard LeanShell Windows 11 VM environment (Windows 11 Pro build 26100, 4 vCPUs, 8 GB RAM, VirtIO graphics at 2560×1600):

| Metric | Direct2D (Shipped) | Slint (Skia, Removed) | Slint (Vello, Removed) | Slint (Software, Removed) | LeanShell Budget Ceiling |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Idle CPU (Dock Hidden)** | 0% (0 ms / 10s) | 0% (0 ms / 10s) | 0% (0 ms / 10s) | 0% (0 ms / 10s) | ~0% |
| **Idle RAM (Working Set)** | ~68 – 72 MB | ~95 – 112 MB | ~118 – 135 MB | ~82 – 90 MB | <= 80 MB |
| **Idle RAM (Private Bytes)**| ~19 – 21 MB | ~34 – 42 MB | ~48 – 56 MB | ~24 – 28 MB | <= 22.2 MB |
| **Binary Size Overhead** | Baseline (+0.0 MB) | +12.4 MB | +18.2 MB | +6.8 MB | < 3.0 MB |
| **Shortcut → First Frame** | 9 – 14 ms | 18 – 24 ms | 26 – 38 ms | 16 – 22 ms | <= 15 ms median |
| **Dock Morph Animation** | 144 fps (V-Sync) | 120 – 144 fps | 90 – 120 fps | 60 fps | 144 fps smooth |
| **Backdrop Support** | Full DWM Acrylic/Mica| HWND DWM Backdrop | HWND DWM Backdrop| HWND DWM Solid | Acrylic & Mica |
| **Licensing** | MIT (Permissive) | GPLv3 / Royalty-Free | GPLv3 / Royalty-Free | GPLv3 / Royalty-Free | MIT |

---

## Architectural Decision: Why Native Win32 + Direct2D Won

1. **Zero external rendering runtime:** Links directly to inbox Windows `d2d1.dll`, `dwrite.dll`, and `dwmapi.dll`.
2. **Minimalist binary footprint:** Keeps release executables under 3 MB (`leanshell.exe` and `leanshell-settings.exe`).
3. **Lowest possible memory consumption:** 19.4 MB private bytes (2–3× leaner than Skia/Vello).
4. **Uniform Win32 event loop & MIT license:** Every window in the workspace uses pure Win32 + Direct2D without mixed UI toolkits or incompatible licenses. Legacy `backend` / `slint_renderer` keys in older user `config.toml` files are harmlessly ignored on startup.
