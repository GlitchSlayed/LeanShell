//! Adds a test notification icon and logs what reaches it, to check the
//! dock's tray proxy without touching other apps' icons.
//!
//! `tray_probe [--host] [--secs N] [--late-add S] [--drop-host S] [--tip TEXT]`
//!
//! `--host` also runs the tray proxy in this process, then clicks and
//! right-clicks the probe's own icon through it after three seconds.
//! `--drop-host S` stops that proxy after S seconds; `--late-add S` adds a
//! second icon after S seconds, to check Explorer still accepts icons.
#![allow(dead_code, unused_imports)]

#[path = "../src/tray_host.rs"]
mod tray_host;

use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, Result, w};

const WM_PROBE: u32 = WM_APP + 1;
const WM_HOST: u32 = WM_APP + 2;
const QUIT_TIMER: usize = 1;
const MODIFY_TIMER: usize = 2;
const INVOKE_TIMER: usize = 3;
const LATE_TIMER: usize = 4;
const DROP_TIMER: usize = 5;

struct Probe {
    start: Instant,
    tip: String,
    host: Option<tray_host::TrayHost>,
    taskbar_created: u32,
}

thread_local! { static PROBE: std::cell::RefCell<Option<Probe>> = const { std::cell::RefCell::new(None) }; }

fn log(text: &str) {
    let elapsed = PROBE.with(|p| p.borrow().as_ref().map_or(0.0, |p| p.start.elapsed().as_secs_f32()));
    println!("[{elapsed:6.2}] {text}");
}

fn data(hwnd: HWND, uid: u32) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: uid,
        ..Default::default()
    }
}

fn add(hwnd: HWND, uid: u32, tip: &str, icon: HICON) {
    let mut d = data(hwnd, uid);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_PROBE;
    d.hIcon = icon;
    for (dst, src) in d.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
        *dst = src;
    }
    let added = unsafe { Shell_NotifyIconW(NIM_ADD, &d) }.as_bool();
    d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
    let versioned = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &d) }.as_bool();
    log(&format!(
        "NIM_ADD uid={uid} -> {added}, NIM_SETVERSION 4 -> {versioned}"
    ));
}

fn icon(id: windows::core::PCWSTR) -> HICON {
    unsafe { LoadIconW(None, id).unwrap_or_default() }
}

fn print_host(host: &tray_host::TrayHost) {
    let icons = host.icons();
    log(&format!("host model: {} icons", icons.len()));
    for icon in icons {
        println!(
            "         {:<44} v{} cb={:#x} visible={} accepted={} promoted={:?} exe={}",
            format!("{:?}", icon.tip.replace('\n', " / ")),
            icon.version,
            icon.callback,
            icon.visible(),
            icon.accepted,
            icon.promoted,
            icon.exes.first().map_or("", String::as_str)
        );
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PROBE => {
                // Version 4: event and ID in lParam, anchor in wParam.
                let (event, uid) = (lp.0 & 0xFFFF, (lp.0 >> 16) & 0xFFFF);
                let (x, y) = ((wp.0 & 0xFFFF) as u16 as i16, ((wp.0 >> 16) & 0xFFFF) as u16 as i16);
                let name = match event as u32 {
                    WM_MOUSEMOVE => "WM_MOUSEMOVE",
                    WM_LBUTTONDOWN => "WM_LBUTTONDOWN",
                    WM_LBUTTONUP => "WM_LBUTTONUP",
                    WM_LBUTTONDBLCLK => "WM_LBUTTONDBLCLK",
                    WM_RBUTTONDOWN => "WM_RBUTTONDOWN",
                    WM_RBUTTONUP => "WM_RBUTTONUP",
                    WM_CONTEXTMENU => "WM_CONTEXTMENU",
                    NIN_SELECT => "NIN_SELECT",
                    0x401 => "NIN_KEYSELECT",
                    0x406 => "NIN_POPUPOPEN",
                    0x407 => "NIN_POPUPCLOSE",
                    _ => "other",
                };
                log(&format!(
                    "callback uid={uid} event={name} ({event:#x}) anchor=({x},{y})"
                ));
                return LRESULT(0);
            }
            WM_HOST => {
                PROBE.with(|p| {
                    if let Some(host) = p.borrow().as_ref().and_then(|p| p.host.as_ref()) {
                        print_host(host);
                    }
                });
                return LRESULT(0);
            }
            WM_TIMER => {
                match wp.0 {
                    QUIT_TIMER => PostQuitMessage(0),
                    MODIFY_TIMER => {
                        let _ = KillTimer(Some(hwnd), MODIFY_TIMER);
                        let tip = PROBE
                            .with(|p| p.borrow().as_ref().map(|p| p.tip.clone()))
                            .unwrap_or_default();
                        let mut d = data(hwnd, 1);
                        d.uFlags = NIF_ICON | NIF_TIP;
                        d.hIcon = icon(IDI_WARNING);
                        let text = format!("{tip}\nmodified");
                        for (dst, src) in d.szTip.iter_mut().zip(text.encode_utf16().take(127)) {
                            *dst = src;
                        }
                        let modified = Shell_NotifyIconW(NIM_MODIFY, &d).as_bool();
                        log(&format!("NIM_MODIFY -> {modified}"));
                        let id = NOTIFYICONIDENTIFIER {
                            cbSize: size_of::<NOTIFYICONIDENTIFIER>() as u32,
                            hWnd: hwnd,
                            uID: 1,
                            ..Default::default()
                        };
                        log(&format!(
                            "Shell_NotifyIconGetRect -> {:?}",
                            Shell_NotifyIconGetRect(&id)
                        ));
                    }
                    INVOKE_TIMER => {
                        let _ = KillTimer(Some(hwnd), INVOKE_TIMER);
                        PROBE.with(|p| {
                            let probe = p.borrow();
                            let Some(host) = probe.as_ref().and_then(|p| p.host.as_ref()) else {
                                return;
                            };
                            let own = host.icons().into_iter().find(|i| i.hwnd as usize == hwnd.0 as usize);
                            match own {
                                Some(own) => {
                                    log("invoking Click then Menu through the host");
                                    for action in [tray_host::Action::Click, tray_host::Action::Menu] {
                                        if let Err(e) = tray_host::invoke(&own, action, 1234, 1100) {
                                            log(&format!("invoke {action:?}: {e}"));
                                        }
                                    }
                                }
                                None => log("probe icon is not in the host model"),
                            }
                        });
                    }
                    LATE_TIMER => {
                        let _ = KillTimer(Some(hwnd), LATE_TIMER);
                        add(hwnd, 2, "Wisp tray probe (late)", icon(IDI_QUESTION));
                    }
                    DROP_TIMER => {
                        let _ = KillTimer(Some(hwnd), DROP_TIMER);
                        let host = PROBE.with(|p| p.borrow_mut().as_mut().and_then(|p| p.host.take()));
                        drop(host);
                        log(&format!(
                            "host dropped; FindWindow(Shell_TrayWnd) = {:?}",
                            FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or_default().0
                        ));
                    }
                    _ => {}
                }
                return LRESULT(0);
            }
            _ => {}
        }
        let created = PROBE.with(|p| p.borrow().as_ref().map_or(0, |p| p.taskbar_created));
        if created != 0 && msg == created {
            log("TaskbarCreated: adding the icon again");
            let tip = PROBE
                .with(|p| p.borrow().as_ref().map(|p| p.tip.clone()))
                .unwrap_or_default();
            add(hwnd, 1, &tip, icon(IDI_INFORMATION));
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let secs: u32 = value("--secs").and_then(|s| s.parse().ok()).unwrap_or(20);
    let tip = value("--tip").unwrap_or_else(|| "Wisp tray probe".into());
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let instance = GetModuleHandleW(None)?;
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("Wisp.TrayProbe"),
            ..Default::default()
        };
        if RegisterClassExW(&class) == 0 {
            return Err(Error::from_thread());
        }
        // Top-level and hidden, so TaskbarCreated reaches it.
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.lpszClassName,
            w!("Wisp tray probe"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )?;
        PROBE.with(|p| {
            *p.borrow_mut() = Some(Probe {
                start: Instant::now(),
                tip: tip.clone(),
                host: None,
                taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            })
        });
        log(&format!(
            "probe hwnd={:?} pid={}; FindWindow(Shell_TrayWnd) = {:?}",
            hwnd.0,
            std::process::id(),
            FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or_default().0
        ));
        if args.iter().any(|a| a == "--host") {
            match tray_host::TrayHost::start(hwnd, WM_HOST) {
                Ok(host) => {
                    log(&format!(
                        "host started; FindWindow(Shell_TrayWnd) = {:?}",
                        FindWindowW(w!("Shell_TrayWnd"), None).unwrap_or_default().0
                    ));
                    PROBE.with(|p| p.borrow_mut().as_mut().unwrap().host = Some(host));
                    SetTimer(Some(hwnd), INVOKE_TIMER, 3000, None);
                }
                Err(e) => log(&format!("host failed: {e}")),
            }
        }
        add(hwnd, 1, &tip, icon(IDI_INFORMATION));
        SetTimer(Some(hwnd), MODIFY_TIMER, 1000, None);
        SetTimer(Some(hwnd), QUIT_TIMER, secs * 1000, None);
        if let Some(s) = value("--late-add").and_then(|s| s.parse::<u32>().ok()) {
            SetTimer(Some(hwnd), LATE_TIMER, s * 1000, None);
        }
        if let Some(s) = value("--drop-host").and_then(|s| s.parse::<u32>().ok()) {
            SetTimer(Some(hwnd), DROP_TIMER, s * 1000, None);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        for uid in [1, 2] {
            let deleted = Shell_NotifyIconW(NIM_DELETE, &data(hwnd, uid)).as_bool();
            log(&format!("NIM_DELETE uid={uid} -> {deleted}"));
        }
        PROBE.with(|p| p.borrow_mut().take());
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Tray probe: {error}");
        std::process::exit(1);
    }
}
