//! Native taskbar snapshot, event-driven suppression and recovery.

use std::marker::PhantomData;
use std::process::{Child, Command};
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Console::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Accessibility::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, Error, HSTRING, Result, w};

#[path = "taskbar/state.rs"]
mod state;
#[path = "taskbar/worker.rs"]
pub mod worker;
use state::Rect;
pub use worker::{Outcome, TaskbarWorker, WM_TASKBAR_RESULT};

pub const WM_TASKBAR_SHOW: u32 = WM_APP + 40;
pub const WM_RECOVERY_DONE: u32 = WM_APP + 41;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicUsize = AtomicUsize::new(0);
static RECOVERY: Mutex<Option<Snapshot>> = Mutex::new(None);

pub(crate) struct Handle(pub HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Debug)]
struct Bar {
    hwnd: usize,
    pid: u32,
    class: String,
    visible: bool,
    monitor: Rect,
    rect: Rect,
    /// Explorer's own window region at snapshot time.
    region: Option<Rect>,
}

#[derive(Clone, Debug)]
struct Monitor {
    bounds: Rect,
    work: Rect,
}

#[derive(Clone, Debug)]
struct Snapshot {
    state: u32,
    bars: Vec<Bar>,
    monitors: Vec<Monitor>,
}

impl From<RECT> for Rect {
    fn from(r: RECT) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}
impl From<Rect> for RECT {
    fn from(r: Rect) -> Self {
        Self {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }
}

fn failure(message: &str) -> Error {
    Error::new(E_FAIL, message)
}

fn class_name(hwnd: HWND) -> String {
    let mut name = [0u16; 128];
    let len = unsafe { GetClassNameW(hwnd, &mut name) };
    String::from_utf16_lossy(&name[..len as usize])
}

/// Explorer's taskbars only: the dock's tray proxy shares the class name.
fn is_taskbar(hwnd: HWND) -> bool {
    matches!(class_name(hwnd).as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
        && unsafe { GetPropW(hwnd, crate::tray_host::PROXY_PROP) }.is_invalid()
        && {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            pid != std::process::id()
        }
}

/// A window's region as a rectangle. Explorer gives its taskbars a simple
/// one; `None` for no region, an empty one or a complex one.
fn window_region(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    (unsafe { GetWindowRgnBox(hwnd, &mut rect) } == SIMPLEREGION).then(|| rect.into())
}

fn is_clipped(hwnd: HWND) -> bool {
    let mut rect = RECT::default();
    unsafe { GetWindowRgnBox(hwnd, &mut rect) == NULLREGION }
}

fn set_region(hwnd: HWND, region: Option<Rect>, redraw: bool) {
    unsafe {
        let Some(r) = region else {
            let _ = SetWindowRgn(hwnd, None, redraw);
            return;
        };
        let handle = CreateRectRgn(r.left, r.top, r.right, r.bottom);
        // The system owns the region only once it is set.
        if SetWindowRgn(hwnd, Some(handle), redraw) == 0 {
            let _ = DeleteObject(handle.into());
        }
    }
}

/// Gives a hidden taskbar an empty window region. Explorer shows its taskbars
/// again whenever the work area changes, such as when the dock's space comes
/// or goes, and the show hook can only hide them after they have drawn. An
/// empty region keeps such a show from drawing anything.
fn clip(hwnd: HWND) {
    if !is_clipped(hwnd) {
        set_region(hwnd, Some(Rect::default()), false);
    }
}

/// Gives a clipped taskbar back the region Explorer gave it.
fn unclip(hwnd: HWND, original: Option<&Bar>) {
    if is_clipped(hwnd) {
        let mut region = original.and_then(|bar| bar.region);
        let mut current = RECT::default();
        if let Some(bar) = original
            && unsafe { GetWindowRect(hwnd, &mut current) }.is_ok()
        {
            region = state::restored_region(bar.region, bar.rect, current.into());
        }
        set_region(hwnd, region, true);
    }
}

/// The snapshot's entry for a taskbar: the same window, or Explorer's new one
/// on the same screen.
fn prior<'a>(snapshot: &'a Snapshot, bar: &Bar) -> Option<&'a Bar> {
    snapshot
        .bars
        .iter()
        .find(|old| old.hwnd == bar.hwnd && old.pid == bar.pid && old.class == bar.class)
        .or_else(|| {
            snapshot
                .bars
                .iter()
                .find(|old| old.class == bar.class && old.monitor == bar.monitor)
        })
}

fn monitor_info(handle: HMONITOR) -> Result<MONITORINFO> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe {
        GetMonitorInfoW(handle, &mut info).ok()?;
    }
    Ok(info)
}

/// Every top-level window of a taskbar class, Explorer's or not. Not
/// EnumWindows: while Start or a taskbar flyout is open, Windows moves the
/// taskbar into a z-order band that EnumWindows skips but FindWindowEx walks.
pub fn taskbar_windows() -> Vec<HWND> {
    let mut list = Vec::new();
    for class in [w!("Shell_TrayWnd"), w!("Shell_SecondaryTrayWnd")] {
        let mut after = None;
        while let Ok(hwnd) = unsafe { FindWindowExW(None, after, class, None) } {
            list.push(hwnd);
            after = Some(hwnd);
        }
    }
    list
}

fn bars() -> Result<Vec<Bar>> {
    let mut list = Vec::new();
    for hwnd in taskbar_windows().into_iter().filter(|&hwnd| is_taskbar(hwnd)) {
        let mut pid = 0;
        let mut rect = RECT::default();
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if GetWindowRect(hwnd, &mut rect).is_ok()
                && let Ok(info) = monitor_info(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST))
            {
                list.push(Bar {
                    hwnd: hwnd.0 as usize,
                    pid,
                    class: class_name(hwnd),
                    visible: IsWindowVisible(hwnd).as_bool(),
                    monitor: info.rcMonitor.into(),
                    rect: rect.into(),
                    region: window_region(hwnd),
                });
            }
        }
    }
    Ok(list)
}

fn monitors() -> Result<Vec<Monitor>> {
    unsafe extern "system" fn callback(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        if let Ok(info) = monitor_info(handle) {
            unsafe {
                (&mut *(data.0 as *mut Vec<Monitor>)).push(Monitor {
                    bounds: info.rcMonitor.into(),
                    work: info.rcWork.into(),
                });
            }
            BOOL(1)
        } else {
            BOOL(0)
        }
    }
    let mut list: Vec<Monitor> = Vec::new();
    unsafe {
        EnumDisplayMonitors(None, None, Some(callback), LPARAM(&mut list as *mut _ as isize)).ok()?;
    }
    Ok(list)
}

pub fn appbar_state() -> u32 {
    let mut data = APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        ..Default::default()
    };
    unsafe { SHAppBarMessage(ABM_GETSTATE, &mut data) as u32 }
}

fn set_state(state: u32) -> Result<()> {
    // Not FindWindow, which finds the tray proxy first.
    let Some(hwnd) = bars()?
        .into_iter()
        .find(|bar| bar.class == "Shell_TrayWnd")
        .map(|bar| HWND(bar.hwnd as _))
    else {
        return Ok(());
    };
    let mut data = APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: hwnd,
        lParam: LPARAM(state as isize),
        ..Default::default()
    };
    unsafe {
        SHAppBarMessage(ABM_SETSTATE, &mut data);
    }
    if appbar_state() != state {
        return Err(failure("ABM_SETSTATE did not take effect"));
    }
    Ok(())
}

fn set_work(work: Rect) -> Result<()> {
    let mut rect = RECT::from(work);
    // Do not persist settings or synchronously broadcast into other processes.
    unsafe {
        SystemParametersInfoW(
            SPI_SETWORKAREA,
            0,
            Some(&mut rect as *mut _ as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    }
}

fn primary_work() -> Result<Rect> {
    let mut rect = RECT::default();
    unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }
    Ok(rect.into())
}

fn restore_areas(areas: &[(Rect, Rect)]) -> Result<()> {
    let current = monitors()?;
    let mut error = None;
    for &(bounds, work) in areas {
        if current.iter().any(|m| m.bounds == bounds && m.work != work)
            && let Err(e) = set_work(work)
        {
            error = Some(e);
        }
    }
    if let Some(error) = error {
        return Err(error);
    }
    for &(bounds, work) in areas {
        if monitors()?.iter().any(|m| m.bounds == bounds && m.work != work) {
            return Err(failure("Restored work area did not match its snapshot"));
        }
    }
    Ok(())
}

fn restore_snapshot(snapshot: &Snapshot) -> Result<()> {
    let current_bars = bars()?;
    let mut error = if current_bars.is_empty() {
        None
    } else {
        set_state(snapshot.state).err()
    };
    for bar in &current_bars {
        let prior = prior(snapshot, bar);
        unclip(HWND(bar.hwnd as _), prior);
        let visible = prior.map(|b| b.visible).unwrap_or(true);
        if state::visibility_diff(visible, bar.visible).is_some() {
            unsafe {
                let _ = ShowWindow(HWND(bar.hwnd as _), if visible { SW_SHOW } else { SW_HIDE });
            }
        }
    }
    let areas: Vec<_> = if current_bars.is_empty() && snapshot.bars.is_empty() {
        snapshot.monitors.iter().map(|m| (m.bounds, m.bounds)).collect()
    } else {
        snapshot.monitors.iter().map(|m| (m.bounds, m.work)).collect()
    };
    if let Err(e) = restore_areas(&areas) {
        error = Some(e);
    }
    for bar in bars()? {
        if prior(snapshot, &bar).map(|b| b.visible).unwrap_or(true) != bar.visible {
            error = Some(failure("Restored taskbar visibility did not match its snapshot"));
        }
    }
    if let Some(error) = error { Err(error) } else { Ok(()) }
}

/// Restores taskbar visibility, clipping, auto-hide state, and desktop work areas
/// without altering `HKCU\...\Winlogon\Shell` or spawning `explorer.exe`.
pub fn restore_taskbar_state() {
    ACTIVE.store(false, Ordering::SeqCst);
    let snapshot = RECOVERY.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(snapshot) = snapshot {
        let _ = restore_snapshot(&snapshot);
    }

    // Unclip and show all taskbars across all monitors
    if let Ok(all_bars) = bars() {
        for bar in all_bars {
            let hwnd = HWND(bar.hwnd as _);
            unclip(hwnd, None);
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
        }
    }

    // Clear auto-hide on primary taskbar when present
    let _ = set_state(0);

    // Ensure full work areas are restored when no Explorer taskbar owns a strip
    let has_tray = crate::tray_host::explorer_tray().is_some();
    if !has_tray && let Ok(mons) = monitors() {
        let areas: Vec<_> = mons.iter().map(|m| (m.bounds, m.bounds)).collect();
        let _ = restore_areas(&areas);
    }
}

/// Starts `explorer.exe` desktop/taskbar shell when Explorer is not hosting a taskbar.
/// Temporarily suspends `HKCU\...\Winlogon\Shell` while Explorer boots so Explorer initializes
/// the Windows taskbar instead of re-launching LeanShell, then restores the registration.
pub fn ensure_explorer_running() {
    if crate::tray_host::explorer_tray().is_some() {
        return;
    }
    let suspended = leanshell_platform_win::suspend_registered_shell();
    let _ = Command::new("explorer.exe").spawn();
    if let Some(shell_cmd) = suspended {
        for _ in 0..80 {
            if crate::tray_host::explorer_tray().is_some() {
                std::thread::sleep(Duration::from_millis(150));
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        leanshell_platform_win::resume_registered_shell(&shell_cmd);
    }
}

/// Comprehensive emergency restore invoked on panic, crash watchdog trigger, or console close.
/// Restores taskbars and work areas and ensures Explorer starts, while preserving
/// `HKCU\...\Winlogon\Shell` so the exclusive shell remains configured across reboots.
pub fn emergency_restore() {
    restore_taskbar_state();
    ensure_explorer_running();
}

unsafe extern "system" fn console_handler(event: u32) -> BOOL {
    match event {
        CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT => {
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            restore_taskbar_state();
            let hwnd = HWND(TARGET.load(Ordering::SeqCst) as _);
            if !hwnd.is_invalid() {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_RECOVERY_DONE, WPARAM(0), LPARAM(0));
                }
            }
            BOOL(1)
        }
        CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT => {
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            emergency_restore();
            let hwnd = HWND(TARGET.load(Ordering::SeqCst) as _);
            if !hwnd.is_invalid() {
                unsafe {
                    let _ = PostMessageW(Some(hwnd), WM_RECOVERY_DONE, WPARAM(0), LPARAM(0));
                }
            }
            BOOL(1)
        }
        _ => BOOL(0),
    }
}

/// Install once at host startup, before creating the controller.
pub fn install_recovery_handlers() -> Result<()> {
    unsafe {
        SetConsoleCtrlHandler(Some(console_handler), true)?;
    }
    static INSTALLED: std::sync::Once = std::sync::Once::new();
    INSTALLED.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            emergency_restore();
            previous(info);
        }));
    });
    Ok(())
}

unsafe extern "system" fn show_event(_: HWINEVENTHOOK, _: u32, hwnd: HWND, object: i32, child: i32, _: u32, _: u32) {
    // The global hook observes only SHOW; ignore all HWNDs except actual taskbars.
    if ACTIVE.load(Ordering::SeqCst) && object == OBJID_WINDOW.0 && child == 0 && is_taskbar(hwnd) {
        let target = HWND(TARGET.load(Ordering::SeqCst) as _);
        unsafe {
            let _ = PostMessageW(Some(target), WM_TASKBAR_SHOW, WPARAM(hwnd.0 as usize), LPARAM(0));
        }
    }
}

struct Watchdog {
    disarm: Handle,
    child: Child,
}

impl Watchdog {
    fn start(snapshot: &Snapshot) -> Result<Self> {
        use std::os::windows::process::CommandExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let prefix = format!("Local\\Wisp.TaskbarHide.{}.{nonce}", std::process::id());
        let ready_name = format!("{prefix}.Ready");
        let disarm_name = format!("{prefix}.Disarm");
        let ready = Handle(unsafe { CreateEventW(None, true, false, &HSTRING::from(&ready_name))? });
        let disarm = Handle(unsafe { CreateEventW(None, true, false, &HSTRING::from(&disarm_name))? });
        let exe = std::env::current_exe().map_err(|e| failure(&e.to_string()))?;
        let mut command = Command::new(exe);
        command.args([
            "--taskbar-watchdog",
            &std::process::id().to_string(),
            "--state",
            &snapshot.state.to_string(),
            "--ready-event",
            &ready_name,
            "--disarm-event",
            &disarm_name,
        ]);
        for monitor in &snapshot.monitors {
            let (a, b) = (monitor.bounds, monitor.work);
            command.arg("--area").arg(format!(
                "{},{},{},{},{},{},{},{}",
                a.left, a.top, a.right, a.bottom, b.left, b.top, b.right, b.bottom
            ));
        }
        for bar in &snapshot.bars {
            let region = bar.region.unwrap_or_default();
            command.arg("--bar").arg(format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                bar.hwnd,
                bar.pid,
                u8::from(bar.visible),
                u8::from(bar.class == "Shell_TrayWnd"),
                bar.monitor.left,
                bar.monitor.top,
                bar.monitor.right,
                bar.monitor.bottom,
                u8::from(bar.region.is_some()),
                region.left,
                region.top,
                region.right,
                region.bottom,
                bar.rect.right - bar.rect.left,
                bar.rect.bottom - bar.rect.top
            ));
        }
        let mut child = command
            .creation_flags(CREATE_NO_WINDOW.0)
            .spawn()
            .map_err(|e| failure(&e.to_string()))?;
        if unsafe { WaitForSingleObject(ready.0, 5000) } != WAIT_OBJECT_0 {
            let _ = child.kill();
            let _ = child.wait();
            return Err(failure(
                "Watchdog did not open the host process within 5 seconds; nothing hidden",
            ));
        }
        println!("watchdog ready pid={}", child.id());
        Ok(Self { disarm, child })
    }

    fn disarm(&mut self) -> Result<()> {
        unsafe {
            SetEvent(self.disarm.0)?;
        }
        self.child.wait().map_err(|e| failure(&e.to_string()))?;
        Ok(())
    }
}

fn run_watchdog(
    pid: u32,
    state: u32,
    areas: &[(Rect, Rect)],
    prior_bars: &[Bar],
    ready: Option<&str>,
    disarm: Option<&str>,
) -> Result<()> {
    let process = Handle(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid)? });
    let mut handles = Vec::new();
    let disarm = disarm
        .map(|name| unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)).map(Handle) })
        .transpose()?;
    // Prefer disarm if normal completion and process exit are both signalled.
    if let Some(event) = &disarm {
        handles.push(event.0);
    }
    handles.push(process.0);
    if let Some(name) = ready {
        let event = Handle(unsafe { OpenEventW(EVENT_MODIFY_STATE, false, &HSTRING::from(name))? });
        unsafe {
            SetEvent(event.0)?;
        }
    }
    let result = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
    if result == WAIT_FAILED {
        return Err(Error::from_thread());
    }
    if disarm.is_some() && result == WAIT_OBJECT_0 {
        return Ok(());
    }
    if result.0 != WAIT_OBJECT_0.0 + handles.len() as u32 - 1 {
        return Err(failure("Unexpected watchdog wait result"));
    }
    let _ = restore_snapshot(&Snapshot {
        state,
        bars: prior_bars.to_vec(),
        monitors: areas.iter().map(|&(bounds, work)| Monitor { bounds, work }).collect(),
    });
    emergency_restore();
    Ok(())
}

/// Own on the message-loop thread; one active controller per process/session.
pub struct TaskbarHider {
    snapshot: Snapshot,
    hook: HWINEVENTHOOK,
    watchdog: Option<Watchdog>,
    _single_instance: Handle,
    restored: bool,
    taskbar_created: u32,
    /// Leaves the primary screen's bottom to a dock that keeps its own space,
    /// instead of reclaiming the taskbar's strip there.
    keep_bottom: bool,
    _thread: PhantomData<Rc<()>>,
}

impl TaskbarHider {
    /// Takes the snapshot, starts the watchdog and the show hook. `reapply`
    /// then hides any stray Explorer taskbars.
    /// `notification_window` must be a top-level window pumped by this thread.
    /// The current executable must dispatch the `--taskbar-watchdog` mode.
    fn start(notification_window: HWND, enable_watchdog: bool) -> Result<Self> {
        if STOP_REQUESTED.load(Ordering::SeqCst) {
            return Err(failure("Console shutdown was requested"));
        }
        install_recovery_handlers()?;
        let mutex = Handle(unsafe { CreateMutexW(None, false, w!("Local\\Wisp.TaskbarHide.Controller"))? });
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(failure("A taskbar controller is already running"));
        }
        let current_bars = bars()?;
        let mut mons = monitors()?;
        if current_bars.is_empty() {
            for m in &mut mons {
                m.work = m.bounds;
            }
        }
        let snapshot = Snapshot {
            state: if current_bars.is_empty() { 0 } else { appbar_state() },
            bars: current_bars,
            monitors: mons,
        };
        let watchdog = if enable_watchdog {
            Some(Watchdog::start(&snapshot)?)
        } else {
            None
        };
        *RECOVERY.lock().unwrap_or_else(|e| e.into_inner()) = Some(snapshot.clone());
        TARGET.store(notification_window.0 as usize, Ordering::SeqCst);
        ACTIVE.store(true, Ordering::SeqCst);
        let hook = unsafe {
            SetWinEventHook(
                EVENT_OBJECT_SHOW,
                EVENT_OBJECT_SHOW,
                None,
                Some(show_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        let controller = Self {
            snapshot,
            hook,
            watchdog,
            _single_instance: mutex,
            restored: false,
            taskbar_created: unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) },
            keep_bottom: false,
            _thread: PhantomData,
        };
        if hook.is_invalid() {
            return Err(Error::from_thread());
        }
        Ok(controller)
    }

    /// Forward every owner message; registered shell broadcasts are recognized here.
    pub fn on_message(&mut self, msg: u32) -> Result<bool> {
        if msg == WM_RECOVERY_DONE {
            self.restore()?;
            return Ok(true);
        }
        if matches!(msg, WM_TASKBAR_SHOW | WM_DISPLAYCHANGE | WM_SETTINGCHANGE)
            || (self.taskbar_created != 0 && msg == self.taskbar_created)
        {
            self.reapply()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Reconcile the runtime config; restoration failures keep recovery armed.
    pub fn configure(slot: &mut Option<Self>, hwnd: HWND, enabled: bool, enable_watchdog: bool) -> Result<()> {
        if enabled {
            if slot.is_none() {
                slot.insert(Self::start(hwnd, enable_watchdog)?).reapply()?;
            }
        } else if let Some(hider) = slot {
            hider.restore()?;
            *slot = None;
        }
        Ok(())
    }

    /// With `keep`, leaves the bottom of the primary screen's work area to
    /// the dock's app bar.
    pub fn keep_bottom(&mut self, keep: bool) {
        self.keep_bottom = keep;
    }

    pub fn is_active(&self) -> bool {
        ACTIVE.load(Ordering::SeqCst) && !self.restored
    }

    /// Call for TaskbarCreated, WM_DISPLAYCHANGE, WM_SETTINGCHANGE and WM_TASKBAR_SHOW.
    pub fn reapply(&mut self) -> Result<()> {
        if !self.is_active() {
            return Ok(());
        }
        if STOP_REQUESTED.load(Ordering::SeqCst) {
            self.restore()?;
            return Ok(());
        }
        let current = bars()?;
        if current.is_empty() {
            return Ok(());
        }
        if current.iter().any(|bar| bar.visible) {
            println!("reapply: hiding visible taskbar window(s)");
        }
        // Capture newly attached monitors before touching their work areas.
        let mut added_monitor = false;
        for monitor in monitors()? {
            if !self.snapshot.monitors.iter().any(|old| old.bounds == monitor.bounds) {
                self.snapshot.monitors.push(monitor);
                added_monitor = true;
            }
        }
        for bar in &current {
            if !self
                .snapshot
                .bars
                .iter()
                .any(|old| old.class == bar.class && old.monitor == bar.monitor)
            {
                self.snapshot.bars.push(bar.clone());
            }
        }
        *RECOVERY.lock().unwrap_or_else(|e| e.into_inner()) = Some(self.snapshot.clone());
        if added_monitor {
            // Refresh the external recovery snapshot before mutating a new display.
            let replacement = Watchdog::start(&self.snapshot)?;
            if let Some(watchdog) = &mut self.watchdog {
                watchdog.disarm()?;
            }
            self.watchdog = Some(replacement);
        }
        let desired = state::hidden_state(self.snapshot.state);
        if appbar_state() != desired {
            set_state(desired)?;
        }
        for bar in &current {
            unsafe {
                let _ = ShowWindow(HWND(bar.hwnd as _), SW_HIDE);
            }
        }
        if bars()?.iter().any(|bar| bar.visible) {
            return Err(failure("A taskbar remained visible after SW_HIDE"));
        }
        // Only once hidden: changing a shown taskbar's region makes Explorer
        // show it again.
        for bar in &current {
            clip(HWND(bar.hwnd as _));
        }
        let primary_bounds = Rect::from(
            monitor_info(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) })?.rcMonitor,
        );
        let dock_bounds = crate::dock::fixed_screen().map(Rect::from).unwrap_or(primary_bounds);
        let dock_edge = crate::dock::space::active_edge();
        for monitor in monitors()? {
            let mut target = monitor.work;
            for bar in current.iter().filter(|bar| bar.monitor == monitor.bounds) {
                let reclaimed = state::reclaimed_work(monitor.bounds, target, bar.rect);
                // The dock/panel app bar owns its configured edge; reclaiming that side would erase it.
                if self.keep_bottom && monitor.bounds == dock_bounds {
                    target = match dock_edge {
                        ABE_TOP => Rect {
                            top: target.top,
                            ..reclaimed
                        },
                        ABE_LEFT => Rect {
                            left: target.left,
                            ..reclaimed
                        },
                        ABE_RIGHT => Rect {
                            right: target.right,
                            ..reclaimed
                        },
                        _ => Rect {
                            bottom: target.bottom,
                            ..reclaimed
                        },
                    };
                } else {
                    target = reclaimed;
                }
            }
            if target != monitor.work {
                println!("reclaim work area {:?} -> {:?}", monitor.work, target);
                set_work(target)?;
                if !monitors()?
                    .iter()
                    .any(|m| m.bounds == monitor.bounds && m.work == target)
                {
                    return Err(failure("SPI_SETWORKAREA did not take effect"));
                }
            }
        }
        let primary = monitor_info(unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) })?;
        if primary_work()? != Rect::from(primary.rcWork) {
            return Err(failure("SPI_GETWORKAREA disagrees with primary monitor rcWork"));
        }
        // A console callback may have restored concurrently with the calls above.
        if STOP_REQUESTED.load(Ordering::SeqCst) {
            ACTIVE.store(false, Ordering::SeqCst);
        }
        if !self.is_active() {
            restore_snapshot(&self.snapshot)?;
        }
        Ok(())
    }

    pub fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        ACTIVE.store(false, Ordering::SeqCst);
        let mut error = None;
        if !self.hook.is_invalid() {
            match unsafe { UnhookWinEvent(self.hook).ok() } {
                Ok(()) => self.hook = HWINEVENTHOOK::default(),
                Err(e) => error = Some(e),
            }
        }
        restore_snapshot(&self.snapshot)?;
        if let Some(error) = error {
            return Err(error);
        }
        // Leave the watchdog armed if any recovery step failed.
        if let Some(watchdog) = &mut self.watchdog {
            watchdog.disarm()?;
        }
        self.watchdog = None;
        self.restored = true;
        *RECOVERY.lock().unwrap_or_else(|e| e.into_inner()) = None;
        TARGET.store(0, Ordering::SeqCst);
        println!("restored snapshot: ABM_GETSTATE={}", appbar_state());
        Ok(())
    }

    /// Disarms the crash watchdog and WinEvent hook during an in-place update hot restart
    /// without showing or restoring the stock Explorer taskbar.
    pub fn disarm_for_hot_restart(slot: &mut Option<Self>) {
        let Some(mut hider) = slot.take() else {
            return;
        };
        ACTIVE.store(false, Ordering::SeqCst);
        if !hider.hook.is_invalid() {
            // SAFETY: Unhooks our WinEvent hook on the owning message-loop thread.
            unsafe {
                let _ = UnhookWinEvent(hider.hook);
            }
            hider.hook = HWINEVENTHOOK::default();
        }
        if let Some(watchdog) = &mut hider.watchdog {
            let _ = watchdog.disarm();
        }
        hider.watchdog = None;
        hider.restored = true;
        *RECOVERY.lock().unwrap_or_else(|e| e.into_inner()) = None;
        TARGET.store(0, Ordering::SeqCst);
    }
}

impl Drop for TaskbarHider {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("Taskbar restore failed (watchdog remains armed): {error}");
        }
    }
}

/// Command-line escape hatch to immediately restore the Windows taskbar and Explorer shell.
pub fn restore_shell_cli() -> Result<()> {
    println!("LeanShell: Coordinated recovery starting...");

    // 1. Signal any running LeanShell instance to quit with WPARAM(2) (full recovery)
    //    or tear down its appbars and hooks before we proceed.
    const WM_APP_QUIT: u32 = WM_APP + 4;
    unsafe {
        for class in [w!("LeanShell.Launcher"), w!("LeanShell.Nightly.Launcher")] {
            if let Ok(launcher) = FindWindowW(class, None)
                && !launcher.is_invalid()
            {
                let _ = PostMessageW(Some(launcher), WM_APP_QUIT, WPARAM(2), LPARAM(0));
                std::thread::sleep(Duration::from_millis(500));
            }
        }
        if let Ok(dock) = FindWindowW(w!("Wisp.Dock"), None)
            && !dock.is_invalid()
        {
            let _ = PostMessageW(Some(dock), WM_APP_QUIT, WPARAM(0), LPARAM(0));
        }
    }

    restore_shell_state_and_explorer(true);
    println!("[SUCCESS] Windows taskbar and Explorer shell restored successfully.");
    Ok(())
}

/// Performs in-process full rollback (Winlogon Shell, IFEO hosts, taskbars, DWM borders, and Explorer)
/// during `HOTKEY_RECOVERY` / `"Restore Windows Explorer & Quit"` teardown without re-signaling self.
pub fn full_restore_shell_in_process() {
    restore_shell_state_and_explorer(false);
}

fn restore_shell_state_and_explorer(log: bool) {
    // 1. Transactionally restore user Winlogon Shell override in HKCU and IFEO host overrides.
    if let Err(e) = leanshell_platform_win::unregister_exclusive_shell() {
        if log {
            eprintln!("  [WARN] Failed to unregister exclusive shell: {e}");
        }
    } else if log {
        println!("  [OK] Reverted HKCU Winlogon Shell override to default.");
    }
    match leanshell_platform_win::restore_shell_hosts() {
        Ok(count) if log => println!("  [OK] Restored IFEO shell host overrides ({count} entries checked/reverted)."),
        Ok(_) => {}
        Err(e) if log => {
            eprintln!("  [WARN] Could not restore HKLM IFEO host overrides (re-run elevated if needed): {e}")
        }
        Err(_) => {}
    }

    // 2. Restore taskbar state, work areas, and DWM active border colors.
    restore_taskbar_state();
    leanshell_platform_win::reset_all_window_borders();
    if log {
        println!("  [OK] Restored taskbars, work areas, and DWM borders.");
    }

    // 3. Ensure Explorer shell restarts to own the desktop and taskbar.
    if crate::tray_host::explorer_tray().is_none() {
        if log {
            println!("  [INFO] Explorer shell not detected. Spawning explorer.exe...");
        }
        match Command::new("explorer.exe").spawn() {
            Ok(_) if log => println!("  [OK] Spawned explorer.exe"),
            Ok(_) => {}
            Err(e) if log => eprintln!("  [ERR] Failed to spawn explorer.exe: {e}"),
            Err(_) => {}
        }
    }
}

/// Prints live shell diagnostics (Track T8).
pub fn print_diagnostics() {
    println!("=== LeanShell System Diagnostics (Track T8) ===");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));

    // 1. Session Ownership & Registration
    let shell_owned = leanshell_platform_win::is_shell_window_owned();
    let is_exclusive = leanshell_platform_win::is_session_exclusive();
    println!("Live Session Exclusive (GetShellWindow owned): {shell_owned} (exclusive_mode={is_exclusive})");

    let registered_shell = leanshell_platform_win::get_registered_shell();
    println!("HKCU Winlogon Shell: {:?}", registered_shell);
    let is_registered = leanshell_platform_win::is_exclusive_shell_registered();
    println!("LeanShell Is Registered Shell: {}", is_registered);

    let backup_shell = leanshell_platform_win::get_backup_shell();
    println!("Backup Prior Shell: {:?}", backup_shell);

    let safe_mode = leanshell_platform_win::is_safe_mode_requested();
    println!("Safe Mode Requested (Shift Held): {}", safe_mode);

    // 2. Active Window Handles
    unsafe {
        let shell_wnd = GetShellWindow();
        println!("GetShellWindow(): {:?}", shell_wnd.0);
        let tray = FindWindowW(w!("Shell_TrayWnd"), None);
        println!("Shell_TrayWnd: {:?}", tray.map(|h| h.0));
        // Check both stable and nightly launcher class names
        let launcher = FindWindowW(w!("LeanShell.Launcher"), None)
            .or_else(|_| FindWindowW(w!("LeanShell.Nightly.Launcher"), None));
        println!("LeanShell Launcher HWND: {:?}", launcher.map(|h| h.0));
        let dock = FindWindowW(w!("Wisp.Dock"), None);
        println!("LeanShell Dock HWND: {:?}", dock.map(|h| h.0));
    }

    // 3. Monitor Work Areas
    if let Ok(mon_list) = monitors() {
        for (i, m) in mon_list.iter().enumerate() {
            println!("Monitor #{}: bounds={:?} work={:?}", i, m.bounds, m.work);
        }
    }

    // 4. Phase 7 Shell Host Neutering & On-Demand Explorer State
    println!("IFEO Shell Experience Hosts:");
    for &exe in leanshell_core::DEFAULT_NEUTERED_HOSTS {
        println!(
            "  {exe}: {}",
            if leanshell_platform_win::is_host_neutered(exe) {
                "neutered (systray.exe)"
            } else {
                "stock"
            }
        );
    }
    let (exp_running, exp_folders, exp_dialogs) = leanshell_platform_win::inspect_explorer_windows();
    println!("On-Demand Explorer: running={exp_running}, folders={exp_folders}, file_dialogs={exp_dialogs}");

    // 5. Phase 8 Hybrid Window Management Diagnostics
    let (cfg, _) = crate::config::load();
    let (hmon, _, work, dpi) = leanshell_platform_win::active_monitor_work_rect();
    let tileable = leanshell_platform_win::list_tileable_windows_on_monitor(hmon).len();
    println!(
        "Hybrid WM (Phase 8): enabled={}, default_tiling={}, gap={}dp, borders={}, active_mon_dpi={}, work={:?}, tileable_windows={}",
        cfg.wm_enabled, cfg.wm_tiling, cfg.wm_gap, cfg.wm_borders, dpi, work, tileable
    );
    println!("================================================");
}

/// Dispatches CLI actions such as `--restore-shell` and `--taskbar-watchdog`.
pub fn dispatch_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && !args.get(1).is_some_and(|a| a == "--taskbar-watchdog") {
        unsafe {
            let _ =
                windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
    if args.iter().any(|a| a == "--register-shell") {
        match std::env::current_exe() {
            Ok(exe) => match leanshell_platform_win::register_exclusive_shell(&exe) {
                Ok(()) => println!("[SUCCESS] LeanShell registered as primary user shell in HKCU Winlogon."),
                Err(e) => eprintln!("Failed to register shell: {e}"),
            },
            Err(e) => eprintln!("Failed to determine executable path: {e}"),
        }
        return true;
    }
    if args.iter().any(|a| a == "--unregister-shell") {
        let _ = leanshell_platform_win::restore_shell_hosts();
        match leanshell_platform_win::unregister_exclusive_shell() {
            Ok(()) => println!("[SUCCESS] Reset HKCU Winlogon Shell override back to Windows Explorer."),
            Err(e) => eprintln!("Failed to unregister shell: {e}"),
        }
        return true;
    }
    if args.iter().any(|a| a == "--neuter-hosts") {
        match leanshell_platform_win::neuter_shell_hosts(leanshell_core::DEFAULT_NEUTERED_HOSTS) {
            Ok(n) => println!("[SUCCESS] Neutered {n} Windows Shell Experience hosts via IFEO."),
            Err(e) => eprintln!("Failed to neuter shell hosts (run as Administrator): {e}"),
        }
        return true;
    }
    if args.iter().any(|a| a == "--restore-hosts") {
        match leanshell_platform_win::restore_shell_hosts() {
            Ok(n) => println!("[SUCCESS] Restored {n} IFEO shell host overrides."),
            Err(e) => eprintln!("Failed to restore shell hosts (run as Administrator): {e}"),
        }
        return true;
    }
    if args.iter().any(|a| a == "--restore-shell") {
        if let Err(e) = restore_shell_cli() {
            eprintln!("Failed to restore shell: {e}");
        }
        return true;
    }
    if args.iter().any(|a| a == "--diagnose" || a == "--diagnostics") {
        print_diagnostics();
        return true;
    }
    if args.iter().any(|a| a == "--check-update") {
        match crate::update::check() {
            Ok(Some(release)) => println!("Update available: {} ({})", release.tag, release.page),
            Ok(None) => println!("LeanShell {} is up to date.", env!("CARGO_PKG_VERSION")),
            Err(e) => eprintln!("Update check failed: {e}"),
        }
        return true;
    }
    if args.iter().any(|a| a == "--update") {
        if let Some(manager) = crate::install::current_manager() {
            println!("{}", manager.instruction());
            return true;
        }
        match crate::update::check() {
            Ok(Some(release)) => {
                println!("Installing LeanShell {}...", release.tag);
                if let Err(e) = crate::update::install(&release) {
                    eprintln!("Update failed: {e}");
                    std::process::exit(1);
                }
                println!("LeanShell updated cleanly to {}.", release.tag);
            }
            Ok(None) => println!("LeanShell {} is already up to date.", env!("CARGO_PKG_VERSION")),
            Err(e) => {
                eprintln!("Update check failed: {e}");
                std::process::exit(1);
            }
        }
        return true;
    }
    if let Some(stage_dir) = arg_value(&args, "--apply-staged-update") {
        let target_dir = arg_value(&args, "--target-dir").unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
                .unwrap_or_default()
        });
        if let Err(e) =
            crate::update::apply_staged_dir(std::path::Path::new(&stage_dir), std::path::Path::new(&target_dir))
        {
            eprintln!("Apply staged update failed: {e}");
            std::process::exit(1);
        }
        return true;
    }
    if let Some(stage_dir) = arg_value(&args, "--handoff-update") {
        let target_dir = arg_value(&args, "--target-dir").unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
                .unwrap_or_default()
        });
        let parent_pid = arg_value(&args, "--parent-pid")
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        if let Err(e) = crate::update::execute_handoff_update(
            std::path::Path::new(&stage_dir),
            std::path::Path::new(&target_dir),
            parent_pid,
        ) {
            eprintln!("Handoff update failed: {e}");
            std::process::exit(1);
        }
        return true;
    }
    if args.get(1).map(String::as_str) == Some("--taskbar-watchdog") {
        if let Err(error) = watchdog_args(&args[2..]) {
            eprintln!("Taskbar watchdog: {error}");
        }
        return true;
    }
    false
}

fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone())
}

fn watchdog_args(args: &[String]) -> Result<()> {
    let invalid = || failure("Invalid taskbar watchdog arguments");
    let pid = args.first().and_then(|s| s.parse().ok()).ok_or_else(invalid)?;
    let mut state = None;
    let mut areas = Vec::new();
    let mut bars = Vec::new();
    let mut ready = None;
    let mut disarm = None;
    let (pairs, remainder) = args[1..].as_chunks::<2>();
    for pair in pairs {
        match pair[0].as_str() {
            "--state" => state = Some(pair[1].parse().map_err(|_| invalid())?),
            "--ready-event" => ready = Some(pair[1].as_str()),
            "--disarm-event" => disarm = Some(pair[1].as_str()),
            "--area" => {
                let n: Vec<i32> = pair[1]
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|_| invalid())?;
                if n.len() != 8 {
                    return Err(invalid());
                }
                areas.push((
                    Rect {
                        left: n[0],
                        top: n[1],
                        right: n[2],
                        bottom: n[3],
                    },
                    Rect {
                        left: n[4],
                        top: n[5],
                        right: n[6],
                        bottom: n[7],
                    },
                ));
            }
            "--bar" => {
                let n: Vec<i64> = pair[1]
                    .split(',')
                    .map(str::parse)
                    .collect::<std::result::Result<_, _>>()
                    .map_err(|_| invalid())?;
                if n.len() != 15 {
                    return Err(invalid());
                }
                bars.push(Bar {
                    hwnd: n[0] as usize,
                    pid: n[1] as u32,
                    visible: n[2] != 0,
                    class: if n[3] != 0 {
                        "Shell_TrayWnd"
                    } else {
                        "Shell_SecondaryTrayWnd"
                    }
                    .into(),
                    monitor: Rect {
                        left: n[4] as i32,
                        top: n[5] as i32,
                        right: n[6] as i32,
                        bottom: n[7] as i32,
                    },
                    // Only the snapshot size is needed to recognize a full-window region.
                    rect: Rect {
                        right: n[13] as i32,
                        bottom: n[14] as i32,
                        ..Rect::default()
                    },
                    region: (n[8] != 0).then(|| Rect {
                        left: n[9] as i32,
                        top: n[10] as i32,
                        right: n[11] as i32,
                        bottom: n[12] as i32,
                    }),
                });
            }
            _ => return Err(invalid()),
        }
    }
    if !remainder.is_empty() {
        return Err(invalid());
    }
    run_watchdog(pid, state.ok_or_else(invalid)?, &areas, &bars, ready, disarm)
}
