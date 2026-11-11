#![windows_subsystem = "windows"]
#![allow(dead_code)]

#[path = "../../src/dock/pins.rs"]
mod pins;

#[allow(dead_code)]
#[path = "../../src/channel.rs"]
mod channel;
mod config;
mod hotkey;
#[path = "../../src/install.rs"]
mod install;
mod system;
mod ui;
mod update;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let welcome = args.iter().any(|a| a == "--welcome");
    let page = args
        .iter()
        .position(|a| a == "--page")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str());
    update::remove_old_files();
    ui::run(welcome, page)
}
