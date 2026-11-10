//! Release channel. Nightly builds use their own data folder, window class,
//! mutex and startup entry, so they run beside a stable install.

use windows::core::{PCWSTR, w};

pub const NIGHTLY: bool = cfg!(feature = "nightly");
pub const NAME: &str = if NIGHTLY { "LeanShell Nightly" } else { "LeanShell" };
/// Folder under `%APPDATA%`.
pub const DATA_DIR: &str = if NIGHTLY { "leanshell-nightly" } else { "leanshell" };
pub const LAUNCHER_CLASS: PCWSTR = if NIGHTLY {
    w!("LeanShell.Nightly.Launcher")
} else {
    w!("LeanShell.Launcher")
};
pub const INSTANCE_MUTEX: PCWSTR = if NIGHTLY {
    w!(r"Local\LeanShell.Nightly.Launcher.SingleInstance")
} else {
    w!(r"Local\LeanShell.Launcher.SingleInstance")
};
/// Value name under the Run key for Start with Windows.
pub const RUN_VALUE: PCWSTR = if NIGHTLY {
    w!("LeanShell Nightly")
} else {
    w!("LeanShell")
};
