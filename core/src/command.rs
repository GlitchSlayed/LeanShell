//! System commands, media control, and power actions models.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaControl {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PowerAction {
    Lock,
    Sleep,
    Restart,
    ShutDown,
    SignOut,
    EmptyRecycleBin,
}

impl PowerAction {
    pub const ALL: [PowerAction; 6] = [
        PowerAction::Lock,
        PowerAction::Sleep,
        PowerAction::Restart,
        PowerAction::ShutDown,
        PowerAction::SignOut,
        PowerAction::EmptyRecycleBin,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum SystemCommand {
    WifiPower(bool),
    WifiConnect(String),
    BluetoothPower(bool),
    Volume(u32),
    Mute(bool),
    AppVolume { pid: u32, percent: u32 },
    Output(String),
    Media(MediaControl),
    FocusWindow(isize),
    CloseWindow(isize),
    Power(PowerAction),
    Quit,
}

impl SystemCommand {
    pub fn stays_open(&self) -> bool {
        !matches!(
            self,
            SystemCommand::Power(_)
                | SystemCommand::FocusWindow(_)
                | SystemCommand::CloseWindow(_)
                | SystemCommand::WifiConnect(_)
                | SystemCommand::Quit
        )
    }
}
