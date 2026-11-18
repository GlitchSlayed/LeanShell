//! Runs the taskbar hider and watchdog supervisor on a dedicated background thread.
//!
//! Hiding and restoring Explorer taskbars can block on cross-process broadcasts
//! while the work area changes. Keeping `TaskbarHider` and its `SetWinEventHook`
//! on its own thread keeps the launcher and dock UI responsive and arms the
//! `--taskbar-watchdog` supervisor even when Explorer has already been nuked.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{E_FAIL, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Error, HRESULT, Result};

use super::TaskbarHider;
pub use crate::messages::WM_TASKBAR_RESULT;

/// Wakes the worker's message loop to process queued jobs.
const WM_JOB: u32 = WM_APP + 1;

enum Job {
    /// Start hiding any taskbar / arm watchdog, or restore and exit.
    Configure {
        enabled: bool,
        watchdog: bool,
    },
    /// A shell broadcast or hook message the hider reacts to.
    Message(u32),
    KeepBottom(bool),
    /// Disarm the watchdog without restoring taskbars (used during hot-restart/recovery).
    Disarm(Sender<()>),
    /// Restore taskbar/workarea state synchronously before quitting.
    Restore(Sender<Result<()>>),
}

/// An error that can cross threads safely.
#[derive(Debug)]
pub struct Failure(pub HRESULT, pub String);

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Failure(error.code(), error.message())
    }
}

impl From<&Failure> for Error {
    fn from(failure: &Failure) -> Self {
        Error::new(failure.0, failure.1.as_str())
    }
}

pub enum Outcome {
    /// A configure or reapply finished. `message` is the shell message that triggered
    /// it, if any; `Ok(true)` when a taskbar changed or was suppressed.
    Applied {
        message: Option<u32>,
        result: std::result::Result<bool, Failure>,
    },
}

pub struct TaskbarWorker {
    jobs: Option<Sender<Job>>,
    outcomes: Receiver<Outcome>,
    thread: u32,
}

impl TaskbarWorker {
    fn spawn(owner: HWND) -> Result<Self> {
        let (jobs, inbox) = channel();
        let (send, outcomes) = channel();
        let (started, thread) = channel();
        let started_flag = Arc::new(AtomicBool::new(false));
        let owner = owner.0 as usize;
        std::thread::Builder::new()
            .name("taskbar".into())
            .spawn(move || {
                started_flag.store(true, Ordering::Release);
                run(HWND(owner as _), inbox, send, started);
            })
            .map_err(|e| Error::new(E_FAIL, e.to_string()))?;
        let thread = thread
            .recv()
            .map_err(|_| Error::new(E_FAIL, "The taskbar thread did not start"))?;
        Ok(Self {
            jobs: Some(jobs),
            outcomes,
            thread,
        })
    }

    fn send(&self, job: Job) {
        if let Some(jobs) = &self.jobs
            && jobs.send(job).is_ok()
        {
            // SAFETY: Posting WM_JOB wakes the dedicated worker thread message loop.
            unsafe {
                let _ = PostThreadMessageW(self.thread, WM_JOB, WPARAM(0), LPARAM(0));
            }
        }
    }

    /// Ensures the taskbar suppressor / watchdog worker is configured for `enabled`.
    pub fn configure(slot: &mut Option<Self>, owner: HWND, enabled: bool, watchdog: bool) -> Result<()> {
        if enabled {
            if let Some(worker) = slot.as_ref() {
                worker.send(Job::Configure {
                    enabled: true,
                    watchdog,
                });
            } else {
                let worker = Self::spawn(owner)?;
                worker.send(Job::Configure {
                    enabled: true,
                    watchdog,
                });
                *slot = Some(worker);
            }
        } else if let Some(worker) = slot.take() {
            worker.send(Job::Configure {
                enabled: false,
                watchdog: false,
            });
        }
        Ok(())
    }

    pub fn on_message(&self, msg: u32) {
        self.send(Job::Message(msg));
    }

    pub fn keep_bottom(&self, keep: bool) {
        self.send(Job::KeepBottom(keep));
    }

    /// Whether the taskbar suppressor / watchdog is currently armed.
    pub fn is_active(&self) -> bool {
        super::ACTIVE.load(Ordering::SeqCst)
    }

    /// Drains all completed job outcomes since the previous call.
    pub fn drain(&self) -> Vec<Outcome> {
        self.outcomes.try_iter().collect()
    }

    /// Disarms the crash watchdog during hot-restart or full-recovery teardown
    /// without re-showing a hidden Explorer taskbar first.
    pub fn disarm_for_hot_restart(slot: &mut Option<Self>) {
        let Some(worker) = slot.take() else {
            return;
        };
        let (done, wait) = channel();
        worker.send(Job::Disarm(done));
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            match wait.recv_timeout(Duration::from_millis(10)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // SAFETY: Pumping sent messages prevents deadlocks during teardown.
                    unsafe {
                        let mut msg = MSG::default();
                        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    }
                }
            }
        }
    }

    /// Restores taskbar/workarea state synchronously before quitting, waiting up to 5 seconds.
    pub fn shutdown(self) -> Result<()> {
        let (reply, answer) = channel();
        self.send(Job::Restore(reply));
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match answer.recv_timeout(Duration::from_millis(10)) {
                Ok(result) => return result,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            // Restoring the work area broadcasts WM_SETTINGCHANGE and waits for
            // every window, ours too: answer sent messages while waiting.
            // SAFETY: Peeking messages on the UI thread answers inter-thread SendMessage broadcasts.
            unsafe {
                let mut msg = MSG::default();
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            }
        }
        Err(Error::new(E_FAIL, "The taskbar thread did not answer"))
    }
}

impl Drop for TaskbarWorker {
    fn drop(&mut self) {
        self.jobs = None;
        // SAFETY: Wakes the worker thread so it observes channel disconnection and exits.
        unsafe {
            let _ = PostThreadMessageW(self.thread, WM_JOB, WPARAM(0), LPARAM(0));
        }
    }
}

fn run(owner: HWND, inbox: Receiver<Job>, outcomes: Sender<Outcome>, started: Sender<u32>) {
    // Create the message queue before anyone posts to it.
    let mut msg = MSG::default();
    // SAFETY: PeekMessage forces Win32 thread message queue allocation before reporting thread ID.
    unsafe {
        let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
    }
    let _ = started.send(unsafe { GetCurrentThreadId() });
    let mut hider: Option<TaskbarHider> = None;
    let report = |outcome: Outcome| {
        if outcomes.send(outcome).is_ok() {
            // SAFETY: Notifies owner launcher HWND that outcome items are ready.
            unsafe {
                let _ = PostMessageW(Some(owner), WM_TASKBAR_RESULT, WPARAM(0), LPARAM(0));
            }
        }
    };
    loop {
        loop {
            match inbox.try_recv() {
                Ok(job) => handle(job, &mut hider, owner, &report),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    drop(hider.take());
                    return;
                }
            }
        }
        // The show hook delivers its events through this loop.
        // SAFETY: Standard Win32 message loop for the taskbar worker thread.
        if unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 <= 0 {
            drop(hider.take());
            return;
        }
        if msg.hwnd.is_invalid() && msg.message == WM_JOB {
            continue;
        }
        // SAFETY: Dispatches WinEvent / internal messages on worker thread.
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn handle(job: Job, hider: &mut Option<TaskbarHider>, owner: HWND, report: &impl Fn(Outcome)) {
    match job {
        Job::Configure { enabled, watchdog } => report(Outcome::Applied {
            message: None,
            result: TaskbarHider::configure(hider, owner, enabled, watchdog)
                .map(|()| true)
                .map_err(Failure::from),
        }),
        Job::Message(msg) => {
            let result = hider.as_mut().map_or(Ok(false), |h| h.on_message(msg));
            report(Outcome::Applied {
                message: Some(msg),
                result: result.map_err(Failure::from),
            });
        }
        Job::KeepBottom(keep) => {
            if let Some(hider) = hider {
                hider.keep_bottom(keep);
            }
        }
        Job::Disarm(done) => {
            TaskbarHider::disarm_for_hot_restart(hider);
            let _ = done.send(());
        }
        Job::Restore(reply) => {
            let result = TaskbarHider::configure(hider, owner, false, false);
            let _ = reply.send(result);
        }
    }
}
