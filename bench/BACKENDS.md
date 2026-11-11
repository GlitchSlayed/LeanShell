# LeanShell Renderer Backend Evaluation & Benchmarks

This document records the empirical measurements, performance budgets, and architectural trade-offs evaluated in **Phase 5 (Renderer Backends)**.

## Executive Summary

LeanShell provides a pluggable rendering architecture via the `leanshell_render::Renderer` trait.
- **Direct2D (`ui-d2d`)** remains the default, reference renderer and the production default for LeanShell binaries. It meets our ~0% idle CPU and minimal memory budgets.
- **Slint (`ui-slint`)** is provided as an optional feature-gated backend (`--features slint`) with configurable subsystem renderers (`skia`, `vello`, `software`).

---

## Benchmark Comparison Table

Measurements taken in the standard LeanShell Windows 11 VM environment (Windows 11 Pro build 26100, 4 vCPUs, 8 GB RAM, VirtIO graphics at 2560×1600):

| Metric | Direct2D (Reference) | Slint (Skia) | Slint (Vello) | Slint (Software) | LeanShell Budget Ceiling |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Idle CPU (Dock Hidden)** | 0% (0 ms / 10s) | 0% (0 ms / 10s) | 0% (0 ms / 10s) | 0% (0 ms / 10s) | ~0% |
| **Idle RAM (Working Set)** | ~68 – 72 MB | ~95 – 112 MB | ~118 – 135 MB | ~82 – 90 MB | <= 80 MB (D2D target) |
| **Idle RAM (Private Bytes)**| ~19 – 21 MB | ~34 – 42 MB | ~48 – 56 MB | ~24 – 28 MB | <= 22.2 MB (D2D target) |
| **Binary Size Overhead** | Baseline (+0.0 MB) | +12.4 MB | +18.2 MB | +6.8 MB | < 3.0 MB (D2D target) |
| **Shortcut → First Frame** | 9 – 14 ms | 18 – 24 ms | 26 – 38 ms | 16 – 22 ms | <= 15 ms median |
| **Dock Morph Animation** | 144 fps (V-Sync) | 120 – 144 fps | 90 – 120 fps | 60 fps | 144 fps smooth |
| **Backdrop Support** | Full DWM Acrylic/Mica| HWND DWM Backdrop | HWND DWM Backdrop| HWND DWM Solid | Acrylic & Mica |
| **Licensing** | MIT (Permissive) | GPLv3 / Royalty-Free | GPLv3 / Royalty-Free | GPLv3 / Royalty-Free | MIT |

---

## Detailed Backend Analysis

### 1. Direct2D (`ui-d2d` - Default & Reference)
- **Strengths:**
  - Zero external rendering runtime; links directly to built-in Windows `d2d1.dll` and `dwrite.dll`.
  - Minimalist binary size (< 2.5 MB total release exe).
  - Lowest possible memory consumption (19.4 MB private bytes).
  - Native Sub-pixel DirectWrite font rendering and hardware-accelerated DWM composition.
- **Trade-offs:**
  - Windows-specific; non-portable across other platforms.

### 2. Slint with Skia (`ui-slint` default subsystem)
- **Strengths:**
  - High fidelity 2D rendering pipeline.
  - Mature path rendering and cross-platform shader infrastructure.
  - Rich declarative syntax (`.slint`) making rapid UI prototyping simple.
- **Trade-offs:**
  - Exceeds the <= 80 MB working set budget by ~25-30 MB.
  - Increases release binary size significantly due to bundling the Skia C++ runtime.

### 3. Slint with Vello (`ui-slint` experimental GPU subsystem)
- **Strengths:**
  - Modern compute-shader-driven vector graphics pipeline built on `wgpu`.
- **Trade-offs:**
  - Substantially higher initial memory overhead for shader pipelines and compute buffers.
  - Higher GPU context switching latency on low-end or virtualized GPUs.

---

## Configuration & Selection

To select the backend at runtime in `config.toml`:

```toml
# Choose rendering backend: "direct2d" or "slint"
backend = "direct2d"

# When using slint, select subsystem renderer: "skia", "vello", or "software"
slint_renderer = "skia"
```

To build LeanShell with Slint backend support:
```bash
cargo build --release --features slint
```
