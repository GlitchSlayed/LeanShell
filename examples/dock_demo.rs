//! Runs an isolated dock for twenty seconds, without the launcher mutex.
//!
//! DOCK_DEMO_TRAY hosts the notification area even if the taskbar stays;
//! DOCK_DEMO_TRAY_CLICK names a tooltip to click, then right-click, after
//! five seconds, through the dock's own handlers.
#![allow(dead_code)]

#[path = "../src/commands/audio.rs"]
pub mod audio;
#[path = "../src/channel.rs"]
mod channel;
#[path = "../src/config.rs"]
mod config;
#[path = "../src/dock.rs"]
mod dock;
#[path = "../src/history.rs"]
mod history;
#[path = "../src/indexer.rs"]
mod indexer;
#[path = "../src/launch.rs"]
mod launch;
#[path = "../src/morph.rs"]
mod morph;
#[path = "../src/commands/open_windows.rs"]
pub mod open_windows;
mod commands {
    pub use crate::{audio, open_windows};
}
#[path = "../src/taskbar.rs"]
mod taskbar;
#[path = "../src/theme.rs"]
mod theme;
#[path = "../src/tray_host.rs"]
mod tray_host;
#[path = "../src/util.rs"]
mod util;

use std::collections::HashMap;
use std::sync::mpsc::channel;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, Result, w};

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_TIMER && wp.0 == 2 {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        if msg == WM_CLOSE {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        if msg == WM_QUERYENDSESSION {
            return LRESULT(1);
        }
        if msg == WM_ENDSESSION && wp.0 != 0 {
            taskbar::emergency_restore();
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn run() -> Result<()> {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("Wisp.Dock.Demo"),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(Error::from_thread());
        }
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.lpszClassName,
            w!("Dock demo recovery"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        let before = taskbar::appbar_state();
        println!("Before: ABM_GETSTATE={before}");
        // Another Wisp may already control the taskbar; the dock runs regardless.
        let mut hider = taskbar::TaskbarHider::hide(hwnd)
            .inspect_err(|error| println!("Taskbar left alone: {error}"))
            .ok();
        let outcome = (|| {
            let mut dock = dock::Dock::new(hwnd)?;
            if std::env::var_os("DOCK_DEMO_TRAY").is_some() {
                dock.set_tray(true);
                println!(
                    "FindWindow(Shell_TrayWnd) = {:?}",
                    FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or_default().0
                );
                SetTimer(Some(hwnd), 3, 5000, None);
            }
            let (tx, rx) = channel();
            let indexer = indexer::Indexer::spawn(hwnd, WM_APP + 2, tx, 48);
            indexer.rescan();
            let mut apps = Vec::new();
            let mut icons = HashMap::new();
            dock.show();
            // DOCK_DEMO_HOLD keeps the dock revealed for screenshots: an app slot
            // index or a status part name to hover, or anything else for none.
            // DOCK_DEMO_LIGHT picks the light theme; DOCK_DEMO_DPI overrides the DPI.
            if let Ok(hover) = std::env::var("DOCK_DEMO_HOLD") {
                if std::env::var_os("DOCK_DEMO_LIGHT").is_some() {
                    dock.apply_theme(theme::Palette::new(false, true), config::Backdrop::Acrylic);
                }
                let dpi = std::env::var("DOCK_DEMO_DPI").ok().and_then(|dpi| dpi.parse().ok());
                dock.hold(hover.parse().ok(), Some(hover.as_str()), dpi);
            }
            let dock_hwnd = FindWindowW(w!("Wisp.Dock"), None)?;
            let mut rect = RECT::default();
            GetWindowRect(dock_hwnd, &mut rect)?;
            println!("Dock DPI={} rect={rect:?}", GetDpiForWindow(dock_hwnd));
            if SetTimer(Some(hwnd), 2, 20_000, None) == 0 {
                return Err(Error::from_thread());
            }
            let mut message = MSG::default();
            loop {
                match GetMessageW(&mut message, None, 0, 0).0 {
                    -1 => return Err(Error::from_thread()),
                    0 => break,
                    _ => {}
                }
                if message.message == WM_TIMER && message.hwnd == hwnd && message.wParam.0 == 3 {
                    let _ = KillTimer(Some(hwnd), 3);
                    for (tip, exe, inline) in dock.tray_summary() {
                        let place = if inline { "dock" } else { "flyout" };
                        println!("Tray [{place}] {:?} {exe}", tip.replace('\n', " / "));
                    }
                    if let Ok(tip) = std::env::var("DOCK_DEMO_TRAY_CLICK") {
                        for action in [tray_host::Action::Click, tray_host::Action::Menu] {
                            println!("Tray {action:?} {tip:?}: {}", dock.use_tray_icon(&tip, action));
                        }
                    }
                }
                if let Some(hider) = &mut hider {
                    hider.on_message(message.message)?;
                }
                let mut changed = false;
                while let Ok(event) = rx.try_recv() {
                    match event {
                        indexer::IndexEvent::Apps(found) => {
                            apps = found;
                        }
                        indexer::IndexEvent::Icons(batch) => icons.extend(batch),
                    }
                    changed = true;
                }
                if changed {
                    println!("Index: {} apps, {} icons", apps.len(), icons.len());
                    dock.set_apps(&apps, &icons);
                }
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            Ok(())
        })();
        let restored = hider.as_mut().map_or(Ok(()), taskbar::TaskbarHider::restore);
        let _ = KillTimer(Some(hwnd), 2);
        let _ = DestroyWindow(hwnd);
        println!("After: ABM_GETSTATE={}", taskbar::appbar_state());
        if taskbar::appbar_state() != before {
            return Err(Error::new(E_FAIL, "Taskbar state changed"));
        }
        restored?;
        outcome
    }
}

fn main() {
    if taskbar::dispatch_watchdog() {
        return;
    }
    if let Err(error) = run() {
        eprintln!("Dock demo: {error}");
        std::process::exit(1);
    }
}
