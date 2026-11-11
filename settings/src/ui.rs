use slint::ComponentHandle;
use std::sync::Arc;

slint::include_modules!();

fn apply_setting<T: Clone>(
    store: &Option<crate::config::Store>,
    weak: &slint::Weak<SettingsWindow>,
    key: &str,
    val: toml_edit::Value,
    update: impl FnOnce(&SettingsWindow, T),
    ui_val: T,
) {
    if let Some(s) = store.as_ref() {
        match s.set(key, val) {
            Ok(_) => {
                crate::system::notify_launcher();
                if let Some(w) = weak.upgrade() {
                    update(&w, ui_val);
                }
            }
            Err(e) => {
                eprintln!("Failed to apply setting {key}: {e}");
            }
        }
    }
}

pub fn run(welcome: bool, page: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let window = SettingsWindow::new()?;
    window.set_is_onboarding(welcome);
    if let Some(p) = page {
        let page_idx = match p {
            "appearance" => 1,
            "search" => 2,
            "about" => 3,
            _ => 0,
        };
        window.set_current_page(page_idx);
    }

    let config_path = crate::config::path().ok();
    let store = config_path.clone().map(crate::config::Store::new);
    if let Some(ref s) = store {
        let _ = s.ensure_exists();
    }
    let config = store.as_ref().and_then(|s| s.load().ok()).unwrap_or_default();

    window.set_hotkey(config.hotkey.into());
    window.set_display_mode(config.display_mode.into());
    window.set_panel_edge(config.panel_edge.into());
    window.set_shell_mode(config.shell_mode.into());
    window.set_watchdog(config.watchdog);
    window.set_claim_win_key(config.claim_win_key);
    window.set_hide_taskbar(config.hide_taskbar);
    window.set_dock_enabled(config.dock);
    window.set_dock_autohide(config.dock_autohide);
    window.set_dock_icon_size(config.dock_icon_size as i32);
    window.set_window_list(config.window_list.into());
    window.set_theme_setting(config.theme.into());
    window.set_backdrop_setting(config.backdrop.into());
    window.set_position_setting(config.position.into());
    window.set_backend_setting(config.backend.into());
    window.set_slint_renderer_setting(config.slint_renderer.into());
    window.set_max_results(config.max_results as i32);
    window.set_show_recent(config.show_recent);
    window.set_search_url(config.search_url.into());
    window.set_file_search(config.file_search);

    let autostart = crate::system::autostart_enabled();
    window.set_autostart(autostart);

    let pins_path = crate::config::path().map(|p| p.with_file_name("dock-pins.toml")).ok();
    let pinned_count = pins_path.as_ref().map(|p| crate::pins::load(p).len()).unwrap_or(0);
    window.set_pinned_count(pinned_count as i32);

    window.set_version_text(env!("CARGO_PKG_VERSION").into());
    window.set_config_path(
        config_path
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
            .into(),
    );

    let store = Arc::new(store);

    // Callbacks
    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_display_mode_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "display_mode",
                val.as_str().into(),
                |w, v| w.set_display_mode(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_panel_edge_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "panel_edge",
                val.as_str().into(),
                |w, v| w.set_panel_edge(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_shell_mode_changed(move |val| {
            if val == "exclusive" {
                if let Ok(exe) = std::env::current_exe() {
                    let main_exe = exe.with_file_name("leanshell.exe");
                    let _ = leanshell_platform_win::register_exclusive_shell(&main_exe);
                }
            } else if val == "companion" {
                let _ = leanshell_platform_win::unregister_exclusive_shell();
            }
            apply_setting(
                &store,
                &weak,
                "shell_mode",
                val.as_str().into(),
                |w, v| w.set_shell_mode(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_watchdog_changed(move |val| {
            apply_setting(&store, &weak, "watchdog", val.into(), |w, v| w.set_watchdog(v), val);
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_claim_win_key_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "claim_win_key",
                val.into(),
                |w, v| w.set_claim_win_key(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_hide_taskbar_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "hide_taskbar",
                val.into(),
                |w, v| w.set_hide_taskbar(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_dock_autohide_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "dock_autohide",
                val.into(),
                |w, v| w.set_dock_autohide(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_dock_icon_size_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "dock_icon_size",
                (val as i64).into(),
                |w, v| w.set_dock_icon_size(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_window_list_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "window_list",
                val.as_str().into(),
                |w, v| w.set_window_list(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_hotkey_changed(move |val| {
            let Some(s) = store.as_ref() else {
                return;
            };
            match s.set("hotkey", val.as_str().into()) {
                Ok(_) => {
                    crate::system::notify_launcher();
                    if let Some(w) = weak.upgrade() {
                        w.set_hotkey(val);
                        w.set_status_message("".into());
                    }
                }
                Err(e) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_status_message(format!("Invalid shortcut: {e}").into());
                    }
                }
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_autostart_changed(move |val| match crate::system::set_autostart(val) {
            Ok(_) => {
                if let Some(w) = weak.upgrade() {
                    w.set_autostart(val);
                    w.set_status_message("".into());
                }
            }
            Err(e) => {
                if let Some(w) = weak.upgrade() {
                    w.set_autostart(!val);
                    w.set_status_message(e.into());
                }
            }
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_theme_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "theme",
                val.as_str().into(),
                |w, v| w.set_theme_setting(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_backdrop_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "backdrop",
                val.as_str().into(),
                |w, v| w.set_backdrop_setting(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_position_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "position",
                val.as_str().into(),
                |w, v| w.set_position_setting(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_backend_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "backend",
                val.as_str().into(),
                |w, v| w.set_backend_setting(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_slint_renderer_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "slint_renderer",
                val.as_str().into(),
                |w, v| w.set_slint_renderer_setting(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_max_results_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "max_results",
                (val as i64).into(),
                |w, v| w.set_max_results(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_show_recent_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "show_recent",
                val.into(),
                |w, v| w.set_show_recent(v),
                val,
            );
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_search_url_changed(move |val| {
            let Some(s) = store.as_ref() else {
                return;
            };
            match s.set("search_url", val.as_str().into()) {
                Ok(_) => {
                    crate::system::notify_launcher();
                    if let Some(w) = weak.upgrade() {
                        w.set_search_url(val);
                        w.set_status_message("".into());
                    }
                }
                Err(e) => {
                    if let Some(w) = weak.upgrade() {
                        w.set_status_message(format!("Invalid URL: {e}").into());
                    }
                }
            }
        });
    }

    {
        let store = Arc::clone(&store);
        let weak = window.as_weak();
        window.on_file_search_changed(move |val| {
            apply_setting(
                &store,
                &weak,
                "file_search",
                val.into(),
                |w, v| w.set_file_search(v),
                val,
            );
        });
    }

    {
        let pins_path = pins_path.clone();
        let weak = window.as_weak();
        window.on_unpin_all_clicked(move || {
            if let Some(ref path) = pins_path {
                let _ = crate::pins::save(path, &[]);
                crate::system::notify_launcher();
                if let Some(w) = weak.upgrade() {
                    w.set_pinned_count(0);
                }
            }
        });
    }

    {
        let config_path = config_path.clone();
        window.on_open_config_clicked(move || {
            if let Some(ref path) = config_path {
                let _ = crate::system::open_config(path);
            }
        });
    }

    window.on_open_github_clicked(move || {
        crate::system::open_url("https://github.com/GlitchSlayed/LeanShell");
    });

    {
        let weak = window.as_weak();
        window.on_check_updates_clicked(move || {
            let weak = weak.clone();
            if let Some(w) = weak.upgrade() {
                w.set_update_status("Checking GitHub for updates…".into());
            }
            std::thread::spawn(move || {
                let result = crate::update::check();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        match result {
                            Ok(Some(release)) => {
                                w.set_update_status(format!("LeanShell {} is available!", release.version()).into());
                                w.set_update_available(true);
                            }
                            Ok(None) => {
                                w.set_update_status("LeanShell is up to date.".into());
                                w.set_update_available(false);
                            }
                            Err(e) => {
                                w.set_update_status(format!("Check failed: {e}").into());
                                w.set_update_available(false);
                            }
                        }
                    }
                });
            });
        });
    }

    {
        let weak = window.as_weak();
        window.on_next_onboarding_clicked(move || {
            if let Some(w) = weak.upgrade() {
                let step = w.get_onboarding_step();
                w.set_onboarding_step(step + 1);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_finish_onboarding_clicked(move || {
            if let Some(w) = weak.upgrade() {
                w.set_is_onboarding(false);
                w.set_current_page(0);
            }
        });
    }

    window.run()?;
    Ok(())
}
