//! Slint UI toolkit renderer backend for LeanShell.
//!
//! Provides an alternative renderer behind the `leanshell_render::Renderer` trait
//! using Slint with configurable Skia (default), Vello (experimental GPU compute),
//! and software rendering backends.

use std::rc::Rc;

use leanshell_core::config::{Backdrop, SlintRendererConfig};
use leanshell_render::{Frame, Renderer};
use slint::{ModelRc, VecModel};

slint::include_modules!();

/// Subsystem renderer type inside the Slint backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SlintBackendType {
    #[default]
    Skia,
    Vello,
    Software,
}

impl From<SlintRendererConfig> for SlintBackendType {
    fn from(cfg: SlintRendererConfig) -> Self {
        match cfg {
            SlintRendererConfig::Skia => Self::Skia,
            SlintRendererConfig::Vello => Self::Vello,
            SlintRendererConfig::Software => Self::Software,
        }
    }
}

impl SlintBackendType {
    pub fn as_env_str(&self) -> &'static str {
        match self {
            Self::Skia => "renderer-skia",
            Self::Vello => "renderer-vello",
            Self::Software => "renderer-software",
        }
    }
}

/// Sets the `SLINT_BACKEND` environment variable if not already set.
pub fn configure_slint_backend(backend: SlintBackendType) {
    if std::env::var("SLINT_BACKEND").is_err() {
        // SAFETY: configuring process environment variable during early init
        unsafe {
            std::env::set_var("SLINT_BACKEND", backend.as_env_str());
        }
    }
}

/// Applies Acrylic / Mica backdrop to a Slint window given its raw HWND.
#[cfg(target_os = "windows")]
pub fn apply_slint_window_backdrop(hwnd: isize, dark: bool, backdrop: Backdrop, persistent: bool) -> bool {
    use windows::Win32::Foundation::HWND;
    leanshell_platform_win::apply_backdrop(HWND(hwnd as *mut _), dark, backdrop, persistent)
}

/// Slint-based launcher renderer implementing `leanshell_render::Renderer`.
pub struct SlintRenderer {
    backend: SlintBackendType,
    dpi: f32,
    window: Option<LauncherWindow>,
}

impl Default for SlintRenderer {
    fn default() -> Self {
        Self::new(SlintBackendType::default())
    }
}

impl SlintRenderer {
    pub fn new(backend: SlintBackendType) -> Self {
        configure_slint_backend(backend);
        Self {
            backend,
            dpi: 96.0,
            window: None,
        }
    }

    pub fn backend(&self) -> SlintBackendType {
        self.backend
    }

    fn ensure_window(&mut self) -> Result<&LauncherWindow, String> {
        if self.window.is_none() {
            let win = LauncherWindow::new().map_err(|e| format!("Failed to create Slint launcher window: {e}"))?;
            self.window = Some(win);
        }
        Ok(self.window.as_ref().unwrap())
    }
}

impl Renderer for SlintRenderer {
    type Error = String;

    fn draw(&mut self, _hwnd: isize, frame: &Frame<'_>) -> Result<(), Self::Error> {
        let window = self.ensure_window()?;

        window.set_search_text(frame.text.into());
        window.set_placeholder_text(frame.placeholder.into());

        if let Some(chip) = frame.chip {
            window.set_has_chip(true);
            window.set_chip_text(chip.into());
        } else {
            window.set_has_chip(false);
            window.set_chip_text("".into());
        }

        let mut rows = Vec::new();
        for (i, res) in frame.results.iter().enumerate() {
            rows.push(ResultRowData {
                title: res.title.clone().into(),
                subtitle: res.subtitle.clone().into(),
                badge: res.verb.unwrap_or_default().into(),
                selected: i == frame.selected,
            });
        }

        let model: Rc<VecModel<ResultRowData>> = Rc::new(VecModel::from(rows));
        window.set_results(ModelRc::from(model));
        window.set_selected_index(frame.selected as i32);

        Ok(())
    }

    fn resize(&mut self, _hwnd: isize, dpi: f32) -> Result<(), Self::Error> {
        self.dpi = dpi;
        Ok(())
    }
}
