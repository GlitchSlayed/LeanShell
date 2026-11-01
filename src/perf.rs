//! Optional buffered QPC tracing. No clocks, buffers or environment reads without perf-trace.
#[cfg(feature = "perf-trace")]
mod enabled {
    use std::cell::RefCell;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    fn ticks() -> i64 {
        let mut value = 0;
        unsafe {
            let _ = QueryPerformanceCounter(&mut value);
        }
        value
    }
    #[derive(Default)]
    struct Trace {
        rows: Vec<(&'static str, i64, i64)>,
        pending: Option<(&'static str, i64)>,
    }
    thread_local! { static TRACE: RefCell<Trace> = RefCell::new(Trace::default()); }
    pub struct Session(i64);
    impl Session {
        pub fn new() -> Self {
            TRACE.with(|t| t.borrow_mut().rows.reserve(8192));
            Self(ticks())
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            let Some(path) = std::env::var_os("WISP_PERF_TRACE") else {
                return;
            };
            let mut frequency = 0;
            unsafe {
                let _ = QueryPerformanceFrequency(&mut frequency);
            }
            let scale = 1000.0 / frequency as f64;
            let mut output = String::from("metric,start_ms,duration_ms\n");
            TRACE.with(|t| {
                for (name, start, end) in &t.borrow().rows {
                    use std::fmt::Write;
                    let _ = writeln!(
                        output,
                        "{name},{:.6},{:.6}",
                        (start - self.0) as f64 * scale,
                        (end - start) as f64 * scale
                    );
                }
            });
            let _ = std::fs::write(path, output);
        }
    }
    pub struct Span(&'static str, i64);
    impl Span {
        pub fn new(name: &'static str) -> Self {
            Self(name, ticks())
        }
    }
    impl Drop for Span {
        fn drop(&mut self) {
            TRACE.with(|t| t.borrow_mut().rows.push((self.0, self.1, ticks())));
        }
    }
    pub fn event(name: &'static str) {
        let now = ticks();
        TRACE.with(|t| t.borrow_mut().rows.push((name, now, now)));
    }
    pub fn input(name: &'static str) {
        TRACE.with(|t| {
            let mut t = t.borrow_mut();
            if t.pending.is_none() {
                t.pending = Some((name, ticks()));
            }
        });
    }
    pub fn present() {
        let now = ticks();
        TRACE.with(|t| {
            let mut t = t.borrow_mut();
            t.rows.push(("content_frame", now, now));
            if let Some((name, start)) = t.pending.take() {
                t.rows.push((name, start, now));
            }
        });
    }
}
#[cfg(feature = "perf-trace")]
pub use enabled::*;
#[cfg(not(feature = "perf-trace"))]
mod disabled {
    pub struct Session;
    impl Session {
        #[inline(always)]
        pub fn new() -> Self {
            Self
        }
    }
    pub struct Span;
    impl Span {
        #[inline(always)]
        pub fn new(_: &'static str) -> Self {
            Self
        }
    }
    #[inline(always)]
    pub fn event(_: &'static str) {}
    #[inline(always)]
    pub fn input(_: &'static str) {}
    #[inline(always)]
    pub fn present() {}
}
#[cfg(not(feature = "perf-trace"))]
pub use disabled::*;
