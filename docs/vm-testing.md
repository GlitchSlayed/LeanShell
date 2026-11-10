# Windows 11 VM Testing Environment

This repository uses a dedicated local Windows 11 Quickemu virtual machine for shell-level, input hook, and taskbar takeover testing on Fedora Linux.

---

## 1. Environment Overview

- **Host OS:** Fedora Linux 45 (Workstation Edition) x86_64
- **VM Engine:** Quickemu 4.9.9 / QEMU 11.1.3 (KVM accelerated)
- **VM Directory:** `/home/vanshdatta/VMs/windows`
- **VM Configuration:** `/home/vanshdatta/VMs/windows/windows-11.conf`
- **Launcher:** `/home/vanshdatta/VMs/windows/launch-windows-11.sh`
- **Resolution:** 2560×1600 (VirGL 3D acceleration enabled)
- **Networking:** User-mode NAT with host SSH port forward (`localhost:22220` -> guest `22`)

---

## 2. Baseline Snapshot

A clean snapshot named **`clean-install`** was created with all VirtIO guest tools, stable BCD settings, and a pristine user environment.

### Restoring to Clean Baseline
Before testing dangerous changes (exclusive shell mode, low-level keyboard hooks, taskbar suppression), or if Windows fails to boot:

```bash
# 1. Stop the VM if running
quickemu --vm /home/vanshdatta/VMs/windows/windows-11.conf --kill

# 2. Roll back to the clean baseline
quickemu --vm /home/vanshdatta/VMs/windows/windows-11.conf --snapshot apply clean-install

# 3. Start the VM again
/home/vanshdatta/VMs/windows/launch-windows-11.sh
```

### Viewing Snapshots
```bash
quickemu --vm /home/vanshdatta/VMs/windows/windows-11.conf --snapshot info
```

### Creating New Snapshots
```bash
quickemu --vm /home/vanshdatta/VMs/windows/windows-11.conf --snapshot create <snapshot-tag>
```

---

## 3. Host KVM MSR Requirement

Windows nested virtualization (Hyper-V / Virtual Machine Platform / Dev Home) requires KVM on the Fedora host to ignore unhandled MSRs:

```bash
# Immediate runtime activation (no reboot needed)
echo 1 | sudo tee /sys/module/kvm/parameters/ignore_msrs

# Persistent across host reboots
echo "options kvm ignore_msrs=1" | sudo tee /etc/modprobe.d/kvm.conf
```

---

## 4. Testing & Safety Protocol

1. **Never test exclusive shell on the host.** Host session is Linux Fedora; all Windows shell tests must strictly run inside the VM.
2. **Always test `scripts/restore-shell.ps1`** inside the VM to ensure the taskbar and default explorer shell can be restored if the process is terminated.
3. If an input hook or shell replacement crashes or hangs the guest, use the snapshot rollback procedure above.

---

## 5. Emergency Recovery Mechanisms

1. **CLI Flag (`--restore-shell`)**:
   ```powershell
   leanshell.exe --restore-shell
   ```
   Unclips and restores all hidden taskbars (`Shell_TrayWnd` and `Shell_SecondaryTrayWnd`), restores monitor work areas, resets the user's Winlogon Shell registry entry (`HKCU`), and exits.

2. **Emergency Key Chord**:
   Press <kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>Esc</kbd> to immediately restore the taskbar and shut down LeanShell.

3. **PowerShell Recovery Script**:
   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\restore-shell.ps1
   ```
