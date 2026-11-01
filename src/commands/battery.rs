//! Battery charge, for surfaces.

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct State {
    /// 0 to 100.
    pub percent: u32,
    pub plugged_in: bool,
    pub saver: bool,
}

/// `None` if the PC has no battery.
pub fn state() -> Option<State> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status).ok()? };
    // 128 means no system battery; 255 means the charge is unknown.
    if status.BatteryFlag & 128 != 0 || status.BatteryLifePercent > 100 {
        return None;
    }
    Some(State {
        percent: status.BatteryLifePercent as u32,
        plugged_in: status.ACLineStatus == 1,
        saver: status.SystemStatusFlag == 1,
    })
}
