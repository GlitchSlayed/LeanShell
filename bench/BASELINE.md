# LeanShell Baseline Metrics

This document records the upstream Wisp performance baselines and LeanShell budget ceilings established in **Phase 0**.

## Performance Budgets & Baselines

| Metric | Upstream Baseline (Wisp 0.2.0) | LeanShell Budget Ceiling | Unit |
| :--- | :--- | :--- | :--- |
| **Idle RAM (Private Memory)** | ~19.4 – 20.2 MB | <= 22.2 MB (Baseline + 10%) | MB |
| **Idle RAM (Working Set)** | ~67.6 – 71.8 MB | <= 80.0 MB | MB |
| **Idle CPU (Dock Hidden)** | 0% (0 ms CPU over 10 s) | ~0% | % / ms |
| **Idle Wakeups / sec (Dock Hidden)** | 0 wakeups | 0 wakeups (measured over 60 s) | wakeups/s |
| **Shortcut → Launcher Drawn** | 9 – 15 ms (p90: 15 – 18 ms) | <= 15 ms median | ms |
| **Typing → First Results** | 9 – 24 ms (p90: 18 – 37 ms) | <= 24 ms median | ms |
| **Typing → Results Settled** | 125 – 188 ms | <= 200 ms median | ms |
| **Binary Size (Direct2D Backend)** | ~0.8 – 1.5 MB | < 3.0 MB | MB |
| **GPU Usage (Idle, Direct2D)** | 0% | 0% | % |
| **Threads (Idle)** | 14 – 18 threads | <= 20 threads | count |

## Reference Hardware Baseline (Wisp 0.2.0 Reference)
- **CPU:** 12th Gen Intel(R) Core(TM) i5-12500H
- **Memory:** 16 GB RAM
- **OS:** Windows 11 Home Single Language (build 26300)
- **Display:** 144 Hz

## Local VM Testing Baseline Environment
- **Host:** Fedora Linux 45 Workstation, Intel Core i7-1360P, 32 GB RAM, KVM
- **Guest VM:** Windows 11 Pro (build 26100.9457), 4 vCPUs, 8 GB RAM, VirtIO graphics (2560×1600)
- **Base Snapshot:** `clean-install`
