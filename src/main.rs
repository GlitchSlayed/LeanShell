// Keep a console for debug logs. Release builds run without a console.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod calc;
mod channel;
mod commands;
mod config;
#[cfg(test)]
#[allow(dead_code)]
mod config_write;
#[allow(dead_code)]
mod deferred;
mod desktop;
mod dock;
mod drag;
mod ease;
mod editor;
mod files;
mod history;
mod hotkey;
mod indexer;
// Settings also uses package-manager detection and update instructions.
#[allow(dead_code)]
mod install;
mod launch;
mod messages;
mod morph;
mod panic_log;
mod render;
mod screens;
mod search;
mod startup;
mod surface;
mod system;
mod taskbar;
mod theme;
mod tray;
mod tray_host;
mod util;

fn main() {
    panic_log::install();
    if taskbar::dispatch_cli() {
        return;
    }
    if let Err(e) = app::run() {
        eprintln!("LeanShell startup error: {:?}", e);
        util::error_box(&format!("LeanShell could not start:\n{}", e.message()));
    }
}
