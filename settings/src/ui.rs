//! Native Win32 + Direct2D settings and onboarding window for LeanShell.
//!
//! Purely event-driven (zero background timers or idle polling): repaints only
//! on input, resize, DPI changes, system theme changes, or async update replies.

use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::{D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_F1,
    VK_F24, VK_HOME, VK_INSERT, VK_LEFT, VK_LWIN, VK_MENU, VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT,
    VK_SPACE, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use crate::config::{Config, Store};

const WINDOW_CLASS: PCWSTR = if crate::channel::NIGHTLY {
    w!("LeanShell.Nightly.Settings")
} else {
    w!("LeanShell.Settings")
};

const WM_APP_BG_EVENT: u32 = WM_APP + 1;
const SIDEBAR_W: f32 = 212.0;
const CONTENT_PAD: f32 = 28.0;
const MAX_CONTENT_W: f32 = 660.0;
const INIT_W_DIP: f32 = 940.0;
const INIT_H_DIP: f32 = 660.0;
const MIN_W_DIP: f32 = 740.0;
const MIN_H_DIP: f32 = 500.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    General,
    DockPanel,
    WindowManager,
    Appearance,
    Search,
    About,
}

impl Page {
    const ALL: [Self; 6] = [
        Self::General,
        Self::DockPanel,
        Self::WindowManager,
        Self::Appearance,
        Self::Search,
        Self::About,
    ];

    fn from_arg(arg: &str) -> Self {
        match arg.trim().to_ascii_lowercase().as_str() {
            "dock" | "panel" => Self::DockPanel,
            "wm" | "window" | "windows" => Self::WindowManager,
            "appearance" | "theme" => Self::Appearance,
            "search" | "files" => Self::Search,
            "about" | "update" => Self::About,
            _ => Self::General,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::DockPanel => "Dock & Panel",
            Self::WindowManager => "Window Manager",
            Self::Appearance => "Appearance",
            Self::Search => "Search & Files",
            Self::About => "About & Updates",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::General => "\u{E713}",       // Settings gear
            Self::DockPanel => "\u{E7F4}",     // Dock / taskbar layout
            Self::WindowManager => "\u{E737}", // Grid / tiles
            Self::Appearance => "\u{E790}",    // Palette / color
            Self::Search => "\u{E721}",        // Search
            Self::About => "\u{E946}",         // Info
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextField {
    DockColor,
    WmBorderColor,
    SearchUrl,
    ExcludeFolder,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ActiveInput {
    None,
    HotkeyCapture,
    Text {
        field: TextField,
        buffer: String,
        caret: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum HitAction {
    SelectPage(Page),
    ToggleAutostart,
    RegisterExclusiveShell,
    UnregisterExclusiveShell,
    ToggleBool {
        key: &'static str,
    },
    SetChoice {
        key: &'static str,
        value: &'static str,
    },
    StepInt {
        key: &'static str,
        delta: i64,
        min: i64,
        max: i64,
    },
    StartHotkeyCapture,
    ActivateText {
        field: TextField,
    },
    ApplyColorSwatch {
        key: &'static str,
        value: &'static str,
    },
    ApplySearchPreset(&'static str),
    AddExcludeFolder,
    RemoveExcludeFolder(usize),
    UnpinAll,
    OpenConfig,
    OpenGithub,
    OpenBugReport,
    CheckUpdates,
    InstallUpdate,
    RestartAfterUpdate,
    OnboardingNext,
    OnboardingFinish,
}

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl Rect {
    fn contains(self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.x + self.w,
            bottom: self.y + self.h,
        }
    }

    fn rounded(self, radius: f32) -> D2D1_ROUNDED_RECT {
        D2D1_ROUNDED_RECT {
            rect: self.d2d(),
            radiusX: radius,
            radiusY: radius,
        }
    }
}

#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    window_rgb: u32,
    text_rgb: u32,
    bg: D2D1_COLOR_F,
    sidebar: D2D1_COLOR_F,
    card: D2D1_COLOR_F,
    card_border: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    subtext: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    accent_text: D2D1_COLOR_F,
    pill_bg: D2D1_COLOR_F,
    danger: D2D1_COLOR_F,
    warning_bg: D2D1_COLOR_F,
    warning_text: D2D1_COLOR_F,
    scrollbar: D2D1_COLOR_F,
}

fn rgba(hex: u32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((hex >> 16) & 0xFF) as f32 / 255.0,
        g: ((hex >> 8) & 0xFF) as f32 / 255.0,
        b: (hex & 0xFF) as f32 / 255.0,
        a,
    }
}

impl Palette {
    fn for_theme(theme: &str) -> Self {
        let dark = match theme {
            "dark" => true,
            "light" => false,
            _ => crate::system::is_system_dark(),
        };
        if dark {
            Self {
                dark: true,
                window_rgb: 0x12151C,
                text_rgb: 0xF3F5F8,
                bg: rgba(0x12151C, 1.0),
                sidebar: rgba(0x181C25, 1.0),
                card: rgba(0x1E232E, 1.0),
                card_border: rgba(0x2E3545, 1.0),
                hover: rgba(0x282F3E, 1.0),
                text: rgba(0xF3F5F8, 1.0),
                subtext: rgba(0x9BA3B4, 1.0),
                accent: rgba(0x0891B2, 1.0),
                accent_text: rgba(0xFFFFFF, 1.0),
                pill_bg: rgba(0x141820, 1.0),
                danger: rgba(0xDC2626, 1.0),
                warning_bg: rgba(0x3B2A14, 1.0),
                warning_text: rgba(0xFBBF24, 1.0),
                scrollbar: rgba(0x475569, 0.7),
            }
        } else {
            Self {
                dark: false,
                window_rgb: 0xF3F5F9,
                text_rgb: 0x111827,
                bg: rgba(0xF3F5F9, 1.0),
                sidebar: rgba(0xE9EDF4, 1.0),
                card: rgba(0xFFFFFF, 1.0),
                card_border: rgba(0xD8DFEB, 1.0),
                hover: rgba(0xE2E7F0, 1.0),
                text: rgba(0x111827, 1.0),
                subtext: rgba(0x4B5563, 1.0),
                accent: rgba(0x0891B2, 1.0),
                accent_text: rgba(0xFFFFFF, 1.0),
                pill_bg: rgba(0xEEF2F7, 1.0),
                danger: rgba(0xDC2626, 1.0),
                warning_bg: rgba(0xFEF3C7, 1.0),
                warning_text: rgba(0x92400E, 1.0),
                scrollbar: rgba(0x94A3B8, 0.7),
            }
        }
    }
}

enum BgEvent {
    CheckDone(Result<Option<crate::update::Release>, String>),
    InstallDone(Result<(), String>),
}

#[derive(Clone)]
struct Formats {
    h1: IDWriteTextFormat,
    h2: IDWriteTextFormat,
    body: IDWriteTextFormat,
    body_semi: IDWriteTextFormat,
    small: IDWriteTextFormat,
    center_small: IDWriteTextFormat,
    glyph: IDWriteTextFormat,
    mono: IDWriteTextFormat,
}

struct Target {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
}

struct Renderer {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    formats: Formats,
    target: Option<Target>,
    dpi: f32,
}

impl Renderer {
    fn new(dpi: f32) -> Result<Self, String> {
        // SAFETY: Standard Direct2D and DirectWrite factory creation with no external pointers.
        unsafe {
            let d2d: ID2D1Factory =
                D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).map_err(|e| e.to_string())?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).map_err(|e| e.to_string())?;

            let pick_font = |preferred: PCWSTR, fallback: PCWSTR| -> PCWSTR {
                let mut collection = None;
                if dwrite.GetSystemFontCollection(&mut collection, false).is_ok()
                    && let Some(coll) = collection
                {
                    let (mut idx, mut exists) = (0, false.into());
                    if coll.FindFamilyName(preferred, &mut idx, &mut exists).is_ok() && exists.as_bool() {
                        return preferred;
                    }
                }
                fallback
            };

            let ui = pick_font(w!("Segoe UI Variable Text"), w!("Segoe UI"));
            let display = pick_font(w!("Segoe UI Variable Display"), w!("Segoe UI"));
            let icons = pick_font(w!("Segoe Fluent Icons"), w!("Segoe MDL2 Assets"));

            let make_fmt = |family: PCWSTR,
                            size: f32,
                            weight: DWRITE_FONT_WEIGHT,
                            wrap: DWRITE_WORD_WRAPPING,
                            align: DWRITE_TEXT_ALIGNMENT,
                            palign: DWRITE_PARAGRAPH_ALIGNMENT|
             -> Result<IDWriteTextFormat, String> {
                let fmt = dwrite
                    .CreateTextFormat(
                        family,
                        None,
                        weight,
                        DWRITE_FONT_STYLE_NORMAL,
                        DWRITE_FONT_STRETCH_NORMAL,
                        size,
                        w!("en-us"),
                    )
                    .map_err(|e| e.to_string())?;
                fmt.SetWordWrapping(wrap).map_err(|e| e.to_string())?;
                fmt.SetTextAlignment(align).map_err(|e| e.to_string())?;
                fmt.SetParagraphAlignment(palign).map_err(|e| e.to_string())?;
                Ok(fmt)
            };

            let formats = Formats {
                h1: make_fmt(
                    display,
                    22.0,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                h2: make_fmt(
                    display,
                    15.5,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                body: make_fmt(
                    ui,
                    13.5,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_WORD_WRAPPING_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                body_semi: make_fmt(
                    ui,
                    13.5,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                small: make_fmt(
                    ui,
                    11.5,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_WORD_WRAPPING_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                )?,
                center_small: make_fmt(
                    ui,
                    12.5,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                glyph: make_fmt(
                    icons,
                    15.0,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
                mono: make_fmt(
                    w!("Consolas"),
                    12.5,
                    DWRITE_FONT_WEIGHT_NORMAL,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                    DWRITE_TEXT_ALIGNMENT_LEADING,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                )?,
            };

            Ok(Self {
                d2d,
                dwrite,
                formats,
                target: None,
                dpi,
            })
        }
    }

    fn set_dpi(&mut self, dpi: f32) {
        self.dpi = dpi;
        if let Some(t) = &self.target {
            // SAFETY: Valid render target on the UI thread.
            unsafe { t.rt.SetDpi(dpi, dpi) };
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if let Some(t) = &self.target {
            // SAFETY: Valid render target and non-zero pixel dimensions.
            unsafe {
                let sz = t.rt.GetPixelSize();
                if sz.width != width || sz.height != height {
                    let _ = t.rt.Resize(&D2D_SIZE_U { width, height });
                }
            }
        }
    }

    fn ensure_target(&mut self, hwnd: HWND) -> Result<&mut Target, String> {
        if self.target.is_none() {
            // SAFETY: Creates HWND render target sized to current client rectangle.
            unsafe {
                let mut rc = RECT::default();
                GetClientRect(hwnd, &mut rc).map_err(|e| e.to_string())?;
                let props = D2D1_RENDER_TARGET_PROPERTIES {
                    r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_IGNORE,
                    },
                    ..Default::default()
                };
                let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd,
                    pixelSize: D2D_SIZE_U {
                        width: rc.right.max(1) as u32,
                        height: rc.bottom.max(1) as u32,
                    },
                    presentOptions: D2D1_PRESENT_OPTIONS_NONE,
                };
                let rt = self
                    .d2d
                    .CreateHwndRenderTarget(&props, &hwnd_props)
                    .map_err(|e| e.to_string())?;
                rt.SetDpi(self.dpi, self.dpi);
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
                let brush = rt
                    .CreateSolidColorBrush(&rgba(0xFFFFFF, 1.0), None)
                    .map_err(|e| e.to_string())?;
                self.target = Some(Target { rt, brush });
            }
        }
        self.target
            .as_mut()
            .ok_or_else(|| "Direct2D target unavailable".to_string())
    }
}

struct SettingsApp {
    hwnd: HWND,
    renderer: Renderer,
    store: Option<Store>,
    config_path_display: String,
    pins_path: Option<std::path::PathBuf>,
    config: Config,
    autostart: bool,
    exclusive_registered: bool,
    powertoys_enabled: bool,
    pinned_count: usize,
    page: Page,
    onboarding: bool,
    onboarding_step: usize,
    scroll_y: f32,
    max_scroll: f32,
    active_input: ActiveInput,
    dock_color_buf: String,
    wm_border_color_buf: String,
    search_url_buf: String,
    exclude_input_buf: String,
    status_message: String,
    update_status: String,
    latest_release: Option<crate::update::Release>,
    update_installed_restart: bool,
    update_busy: bool,
    bg_queue: Arc<Mutex<Vec<BgEvent>>>,
    hits: Vec<(Rect, HitAction)>,
    hovered: Option<usize>,
    tracking_mouse: bool,
}

impl SettingsApp {
    fn new(hwnd: HWND, dpi: f32, welcome: bool, page: Option<&str>) -> Result<Self, String> {
        let config_path = crate::config::path().ok();
        let store = config_path.clone().map(Store::new);
        if let Some(ref s) = store {
            let _ = s.ensure_exists();
        }
        let config = store.as_ref().and_then(|s| s.load().ok()).unwrap_or_default();
        let pins_path = config_path.as_ref().map(|p| p.with_file_name("dock-pins.toml"));
        let pinned_count = pins_path.as_ref().map(|p| crate::pins::load(p).len()).unwrap_or(0);
        let dock_color_buf = config.dock_color.clone();
        let wm_border_color_buf = config.wm_border_color.clone();
        let search_url_buf = config.search_url.clone();

        let renderer = Renderer::new(dpi)?;
        let initial_page = page.map(Page::from_arg).unwrap_or(Page::General);
        let exclusive_registered = leanshell_platform_win::is_exclusive_shell_registered();
        let powertoys_enabled = crate::system::powertoys_run_enabled().unwrap_or(false);
        let autostart = crate::system::autostart_enabled();

        let app = Self {
            hwnd,
            renderer,
            store,
            config_path_display: config_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            pins_path,
            config,
            autostart,
            exclusive_registered,
            powertoys_enabled,
            pinned_count,
            page: initial_page,
            onboarding: welcome,
            onboarding_step: 0,
            scroll_y: 0.0,
            max_scroll: 0.0,
            active_input: ActiveInput::None,
            dock_color_buf,
            wm_border_color_buf,
            search_url_buf,
            exclude_input_buf: String::new(),
            status_message: String::new(),
            update_status: if crate::channel::NIGHTLY {
                "Nightly channel updates are installed manually.".into()
            } else {
                format!("LeanShell {} (100% Native Direct2D)", env!("CARGO_PKG_VERSION"))
            },
            latest_release: None,
            update_installed_restart: false,
            update_busy: false,
            bg_queue: Arc::new(Mutex::new(Vec::new())),
            hits: Vec::new(),
            hovered: None,
            tracking_mouse: false,
        };
        app.sync_title_bar();
        Ok(app)
    }

    fn palette(&self) -> Palette {
        Palette::for_theme(&self.config.theme)
    }

    fn sync_title_bar(&self) {
        let p = self.palette();
        crate::system::style_hwnd_title_bar(self.hwnd, p.dark, p.window_rgb, p.text_rgb);
    }

    fn invalidate(&self) {
        // SAFETY: Valid window HWND on UI thread.
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn set_key(&mut self, key: &str, value: toml_edit::Value) {
        let Some(ref store) = self.store else {
            self.status_message = "Config file path is unavailable.".into();
            return;
        };
        match store.set(key, value) {
            Ok(updated) => {
                self.config = updated;
                self.dock_color_buf = self.config.dock_color.clone();
                self.wm_border_color_buf = self.config.wm_border_color.clone();
                self.search_url_buf = self.config.search_url.clone();
                self.status_message.clear();
                crate::system::notify_launcher();
                if key == "theme" {
                    self.sync_title_bar();
                }
            }
            Err(e) => {
                self.status_message = e;
            }
        }
    }

    fn paint(&mut self) {
        let p = self.palette();
        let prev_hits = std::mem::take(&mut self.hits);
        let hovered_action = self.hovered.and_then(|idx| prev_hits.get(idx)).map(|(_, a)| a.clone());
        let mut new_hits = Vec::with_capacity(prev_hits.len());

        let (res, new_max_scroll) = {
            let target = match self.renderer.ensure_target(self.hwnd) {
                Ok(t) => t,
                Err(_) => return,
            };
            let rt = target.rt.clone();
            let brush = target.brush.clone();
            let formats = self.renderer.formats.clone();
            let mut updated_max_scroll = self.max_scroll;

            // SAFETY: Paired BeginPaint/EndDraw on valid HWND render target.
            let end_res = unsafe {
                rt.BeginDraw();
                rt.Clear(Some(&p.bg));
                let size = rt.GetSize();

                if self.onboarding {
                    draw_onboarding(
                        &rt,
                        &brush,
                        &formats,
                        &p,
                        size.width,
                        size.height,
                        self.onboarding_step,
                        self,
                        &mut new_hits,
                    );
                } else {
                    draw_sidebar(
                        &rt,
                        &brush,
                        &formats,
                        &p,
                        size.height,
                        self.page,
                        &mut new_hits,
                        &hovered_action,
                    );
                    let content_x = SIDEBAR_W + CONTENT_PAD;
                    let content_w = (size.width - content_x - CONTENT_PAD).clamp(320.0, MAX_CONTENT_W);
                    rt.PushAxisAlignedClip(
                        &D2D_RECT_F {
                            left: SIDEBAR_W,
                            top: 0.0,
                            right: size.width,
                            bottom: size.height,
                        },
                        D2D1_ANTIALIAS_MODE_ALIASED,
                    );
                    let content_h = draw_page_content(
                        &rt,
                        &brush,
                        &formats,
                        &p,
                        content_x,
                        content_w,
                        size.height,
                        self,
                        &mut new_hits,
                    );
                    rt.PopAxisAlignedClip();

                    let view_h = (size.height - CONTENT_PAD * 2.0).max(100.0);
                    updated_max_scroll = (content_h - view_h).max(0.0);
                    let effective_scroll = self.scroll_y.min(updated_max_scroll);
                    if updated_max_scroll > 0.0 {
                        let ratio = view_h / content_h.max(view_h);
                        let thumb_h = (size.height * ratio).max(28.0);
                        let thumb_y = (effective_scroll / updated_max_scroll) * (size.height - thumb_h);
                        brush.SetColor(&p.scrollbar);
                        rt.FillRoundedRectangle(
                            &Rect {
                                x: size.width - 7.0,
                                y: thumb_y + 2.0,
                                w: 4.0,
                                h: thumb_h - 4.0,
                            }
                            .rounded(2.0),
                            &brush,
                        );
                    }
                }

                rt.EndDraw(None, None)
            };
            (end_res, updated_max_scroll)
        };

        self.hits = new_hits;
        self.max_scroll = new_max_scroll;
        if self.scroll_y > self.max_scroll {
            self.scroll_y = self.max_scroll;
        }

        if let Err(e) = res
            && e.code() == D2DERR_RECREATE_TARGET
        {
            self.renderer.target = None;
            self.invalidate();
        }
    }

    fn handle_click(&mut self, dip_x: f32, dip_y: f32) {
        let Some(action) = self
            .hits
            .iter()
            .rev()
            .find(|(r, _)| r.contains(dip_x, dip_y))
            .map(|(_, a)| a.clone())
        else {
            // Clicking empty space commits active text edits if any.
            self.commit_active_text();
            self.active_input = ActiveInput::None;
            self.invalidate();
            return;
        };

        if !matches!(action, HitAction::ActivateText { .. } | HitAction::StartHotkeyCapture) {
            self.commit_active_text();
            self.active_input = ActiveInput::None;
        }

        match action {
            HitAction::SelectPage(page) => {
                self.page = page;
                self.scroll_y = 0.0;
                self.hovered = None;
                self.status_message.clear();
            }
            HitAction::ToggleAutostart => {
                let next = !self.autostart;
                match crate::system::set_autostart(next) {
                    Ok(()) => {
                        self.autostart = next;
                        self.status_message.clear();
                    }
                    Err(e) => {
                        self.status_message = e;
                    }
                }
            }
            HitAction::RegisterExclusiveShell => {
                let main_exe = std::env::current_exe()
                    .ok()
                    .map(|p| p.with_file_name("leanshell.exe"))
                    .filter(|p| p.is_file());
                if let Some(exe) = main_exe {
                    match leanshell_platform_win::register_exclusive_shell(&exe) {
                        Ok(()) => {
                            self.exclusive_registered = true;
                            self.set_key("shell_mode", "exclusive".into());
                            self.status_message = "Exclusive shell registered (effective next logon).".into();
                        }
                        Err(e) => self.status_message = format!("Shell registration failed: {e}"),
                    }
                } else {
                    self.status_message = "leanshell.exe not found beside leanshell-settings.exe.".into();
                }
            }
            HitAction::UnregisterExclusiveShell => match leanshell_platform_win::unregister_exclusive_shell() {
                Ok(()) => {
                    self.exclusive_registered = false;
                    self.status_message = "Restored stock Windows shell in Winlogon registry.".into();
                }
                Err(e) => self.status_message = format!("Restore failed: {e}"),
            },
            HitAction::ToggleBool { key } => {
                let current = match key {
                    "watchdog" => self.config.watchdog,
                    "claim_win_key" => self.config.claim_win_key,
                    "neuter_shell_hosts" => self.config.neuter_shell_hosts,
                    "auto_reap_explorer" => self.config.auto_reap_explorer,
                    "claim_shell_chords" => self.config.claim_shell_chords,
                    "show_recent" => self.config.show_recent,
                    "dock" => self.config.dock,
                    "dock_autohide" => self.config.dock_autohide,
                    "hide_taskbar" => self.config.hide_taskbar,
                    "dock_search_button" => self.config.dock_search_button,
                    "wm_enabled" => self.config.wm_enabled,
                    "wm_tiling" => self.config.wm_tiling,
                    "wm_borders" => self.config.wm_borders,
                    "file_search" => self.config.file_search,
                    "file_search_everything" => self.config.file_search_everything,
                    _ => false,
                };
                self.set_key(key, (!current).into());
            }
            HitAction::SetChoice { key, value } => {
                self.set_key(key, value.into());
            }
            HitAction::StepInt { key, delta, min, max } => {
                let current = match key {
                    "max_results" => self.config.max_results,
                    "dock_icon_size" => self.config.dock_icon_size,
                    "wm_gap" => self.config.wm_gap,
                    "update_check_hours" => self.config.update_check_hours,
                    _ => min,
                };
                let next = (current + delta).clamp(min, max);
                self.set_key(key, next.into());
            }
            HitAction::StartHotkeyCapture => {
                self.active_input = ActiveInput::HotkeyCapture;
                self.status_message = "Press a new shortcut (e.g. Alt+Space), or Esc to cancel.".into();
            }
            HitAction::ActivateText { field } => {
                let text = match field {
                    TextField::DockColor => self.dock_color_buf.clone(),
                    TextField::WmBorderColor => self.wm_border_color_buf.clone(),
                    TextField::SearchUrl => self.search_url_buf.clone(),
                    TextField::ExcludeFolder => self.exclude_input_buf.clone(),
                };
                let caret = text.len();
                self.active_input = ActiveInput::Text {
                    field,
                    buffer: text,
                    caret,
                };
            }
            HitAction::ApplyColorSwatch { key, value } => {
                if key == "dock_color" {
                    self.dock_color_buf = value.into();
                } else if key == "wm_border_color" {
                    self.wm_border_color_buf = value.into();
                }
                self.set_key(key, value.into());
            }
            HitAction::ApplySearchPreset(url) => {
                self.search_url_buf = url.into();
                self.set_key("search_url", url.into());
            }
            HitAction::AddExcludeFolder => {
                self.commit_exclude_add();
            }
            HitAction::RemoveExcludeFolder(idx) => {
                let mut list = self.config.file_search_exclude.clone();
                if idx < list.len() {
                    list.remove(idx);
                    let arr: toml_edit::Array = list.into_iter().collect();
                    self.set_key("file_search_exclude", toml_edit::Value::Array(arr));
                }
            }
            HitAction::UnpinAll => {
                if let Some(ref p) = self.pins_path {
                    if let Err(e) = crate::pins::save(p, &[]) {
                        self.status_message = format!("Cannot unpin apps: {e}");
                    } else {
                        self.pinned_count = 0;
                        crate::system::notify_launcher();
                        self.status_message = "Cleared all pinned apps.".into();
                    }
                }
            }
            HitAction::OpenConfig => {
                if let Ok(path) = crate::config::path()
                    && let Err(e) = crate::system::open_config(&path)
                {
                    self.status_message = e;
                }
            }
            HitAction::OpenGithub => {
                crate::system::open_url("https://github.com/GlitchSlayed/LeanShell");
            }
            HitAction::OpenBugReport => {
                crate::system::open_url("https://github.com/GlitchSlayed/LeanShell/issues/new");
            }
            HitAction::CheckUpdates => {
                if !self.update_busy {
                    self.update_busy = true;
                    self.update_status = "Checking GitHub releases...".into();
                    let hwnd_raw = self.hwnd.0 as isize;
                    let q = Arc::clone(&self.bg_queue);
                    std::thread::spawn(move || {
                        let result = crate::update::check();
                        if let Ok(mut guard) = q.lock() {
                            guard.push(BgEvent::CheckDone(result));
                        }
                        // SAFETY: Posting WM_APP notification back to UI thread's valid HWND.
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(hwnd_raw as *mut core::ffi::c_void)),
                                WM_APP_BG_EVENT,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    });
                }
            }
            HitAction::InstallUpdate => {
                if !self.update_busy
                    && let Some(release) = self.latest_release.clone()
                {
                    self.update_busy = true;
                    self.update_status = format!("Downloading and verifying {}...", release.tag);
                    let hwnd_raw = self.hwnd.0 as isize;
                    let q = Arc::clone(&self.bg_queue);
                    std::thread::spawn(move || {
                        let result = crate::update::install(&release);
                        if let Ok(mut guard) = q.lock() {
                            guard.push(BgEvent::InstallDone(result));
                        }
                        // SAFETY: Posting WM_APP notification back to UI thread's valid HWND.
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(hwnd_raw as *mut core::ffi::c_void)),
                                WM_APP_BG_EVENT,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    });
                }
            }
            HitAction::RestartAfterUpdate => {
                if let Err(e) = crate::update::restart_settings() {
                    self.status_message = e;
                } else {
                    // SAFETY: Destroy this settings window so the updated process replaces it.
                    unsafe {
                        let _ = DestroyWindow(self.hwnd);
                    }
                }
            }
            HitAction::OnboardingNext => {
                if self.onboarding_step < 2 {
                    self.onboarding_step += 1;
                } else {
                    self.onboarding = false;
                }
            }
            HitAction::OnboardingFinish => {
                self.onboarding = false;
                self.page = Page::General;
            }
        }
        self.invalidate();
    }

    fn commit_exclude_add(&mut self) {
        let trimmed = self.exclude_input_buf.trim().to_string();
        if trimmed.is_empty() {
            return;
        }
        let mut list = self.config.file_search_exclude.clone();
        if !list.iter().any(|x| x.eq_ignore_ascii_case(&trimmed)) {
            list.push(trimmed);
            let arr: toml_edit::Array = list.into_iter().collect();
            self.set_key("file_search_exclude", toml_edit::Value::Array(arr));
        }
        self.exclude_input_buf.clear();
        if let ActiveInput::Text {
            field: TextField::ExcludeFolder,
            buffer,
            caret,
        } = &mut self.active_input
        {
            buffer.clear();
            *caret = 0;
        }
    }

    fn commit_active_text(&mut self) {
        let active = std::mem::replace(&mut self.active_input, ActiveInput::None);
        if let ActiveInput::Text { field, buffer, .. } = active {
            match field {
                TextField::DockColor => {
                    self.dock_color_buf = buffer.trim().to_string();
                    self.set_key("dock_color", self.dock_color_buf.as_str().into());
                }
                TextField::WmBorderColor => {
                    self.wm_border_color_buf = buffer.trim().to_string();
                    self.set_key("wm_border_color", self.wm_border_color_buf.as_str().into());
                }
                TextField::SearchUrl => {
                    self.search_url_buf = buffer.trim().to_string();
                    self.set_key("search_url", self.search_url_buf.as_str().into());
                }
                TextField::ExcludeFolder => {
                    self.exclude_input_buf = buffer;
                }
            }
        }
    }

    fn handle_key_down(&mut self, vk: u16) -> bool {
        if matches!(self.active_input, ActiveInput::HotkeyCapture) {
            if vk == VK_ESCAPE.0 {
                self.active_input = ActiveInput::None;
                self.status_message.clear();
                self.invalidate();
                return true;
            }
            if let Some(key_name) = vk_to_hotkey_name(vk) {
                // SAFETY: Reading modifier key state for current window event.
                let down = |k: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe {
                    GetKeyState(k.0 as i32) < 0
                };
                let ctrl = down(VK_CONTROL);
                let alt = down(VK_MENU);
                let shift = down(VK_SHIFT);
                let win = down(VK_LWIN) || down(VK_RWIN);
                match crate::hotkey::recorded(&key_name, ctrl, alt, shift, win) {
                    Ok(spec) => {
                        self.set_key("hotkey", spec.as_str().into());
                        self.active_input = ActiveInput::None;
                    }
                    Err(e) => {
                        self.status_message = e;
                    }
                }
                self.invalidate();
            }
            return true;
        }

        if let ActiveInput::Text { field, buffer, caret } = &mut self.active_input {
            let field_copy = *field;
            match vk {
                x if x == VK_LEFT.0 => {
                    *caret = prev_char_boundary(buffer, *caret);
                    self.sync_live_buffer(field_copy);
                }
                x if x == VK_RIGHT.0 => {
                    *caret = next_char_boundary(buffer, *caret);
                    self.sync_live_buffer(field_copy);
                }
                x if x == VK_HOME.0 => {
                    *caret = 0;
                    self.sync_live_buffer(field_copy);
                }
                x if x == VK_END.0 => {
                    *caret = buffer.len();
                    self.sync_live_buffer(field_copy);
                }
                x if x == VK_DELETE.0 => {
                    if *caret < buffer.len() {
                        let next = next_char_boundary(buffer, *caret);
                        buffer.drain(*caret..next);
                        self.sync_live_buffer(field_copy);
                    }
                }
                x if x == VK_RETURN.0 => {
                    if field_copy == TextField::ExcludeFolder {
                        self.exclude_input_buf = buffer.clone();
                        self.commit_exclude_add();
                    } else {
                        self.commit_active_text();
                    }
                }
                x if x == VK_ESCAPE.0 => {
                    self.active_input = ActiveInput::None;
                    self.dock_color_buf = self.config.dock_color.clone();
                    self.wm_border_color_buf = self.config.wm_border_color.clone();
                    self.search_url_buf = self.config.search_url.clone();
                    self.status_message.clear();
                }
                _ => return false,
            }
            self.invalidate();
            return true;
        }
        false
    }

    fn sync_live_buffer(&mut self, field: TextField) {
        if let ActiveInput::Text { buffer, .. } = &self.active_input {
            match field {
                TextField::DockColor => self.dock_color_buf = buffer.clone(),
                TextField::WmBorderColor => self.wm_border_color_buf = buffer.clone(),
                TextField::SearchUrl => self.search_url_buf = buffer.clone(),
                TextField::ExcludeFolder => self.exclude_input_buf = buffer.clone(),
            }
        }
    }

    fn handle_char(&mut self, ch_code: u16) -> bool {
        let Some(ch) = char::from_u32(u32::from(ch_code)) else {
            return false;
        };
        let ActiveInput::Text { field, buffer, caret } = &mut self.active_input else {
            return false;
        };
        let field_copy = *field;
        if ch_code == u16::from(b'\x08') {
            if *caret > 0 {
                let prev = prev_char_boundary(buffer, *caret);
                buffer.drain(prev..*caret);
                *caret = prev;
                self.sync_live_buffer(field_copy);
                self.invalidate();
            }
            return true;
        }
        if ch.is_control() {
            return false;
        }
        buffer.insert(*caret, ch);
        *caret += ch.len_utf8();
        self.sync_live_buffer(field_copy);
        self.invalidate();
        true
    }

    fn drain_bg_events(&mut self) {
        let events: Vec<BgEvent> = {
            let Ok(mut guard) = self.bg_queue.lock() else {
                return;
            };
            std::mem::take(&mut *guard)
        };
        for ev in events {
            match ev {
                BgEvent::CheckDone(res) => {
                    self.update_busy = false;
                    match res {
                        Ok(Some(release)) => {
                            self.update_status = format!("LeanShell {} is available!", release.version());
                            self.latest_release = Some(release);
                        }
                        Ok(None) => {
                            self.update_status = "LeanShell is up to date.".into();
                            self.latest_release = None;
                        }
                        Err(e) => {
                            self.update_status = e;
                            self.latest_release = None;
                        }
                    }
                }
                BgEvent::InstallDone(res) => {
                    self.update_busy = false;
                    match res {
                        Ok(()) => {
                            self.update_status = "Update installed! Restart Settings to load the new build.".into();
                            self.update_installed_restart = true;
                            self.latest_release = None;
                        }
                        Err(e) => {
                            self.update_status = e;
                        }
                    }
                }
            }
        }
        self.invalidate();
    }
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 {
        i -= 1;
        if s.is_char_boundary(i) {
            return i;
        }
    }
    0
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    if i >= s.len() {
        return s.len();
    }
    i += 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn vk_to_hotkey_name(vk: u16) -> Option<String> {
    if vk == VK_SPACE.0 {
        return Some("space".into());
    }
    if vk == VK_RETURN.0 {
        return Some("return".into());
    }
    if vk == VK_TAB.0 {
        return Some("tab".into());
    }
    if vk == VK_INSERT.0 {
        return Some("insert".into());
    }
    if vk == VK_DELETE.0 {
        return Some("delete".into());
    }
    if vk == VK_HOME.0 {
        return Some("home".into());
    }
    if vk == VK_END.0 {
        return Some("end".into());
    }
    if vk == VK_PRIOR.0 {
        return Some("pageup".into());
    }
    if vk == VK_NEXT.0 {
        return Some("pagedown".into());
    }
    if vk == VK_BACK.0 {
        return None;
    }
    if (VK_F1.0..=VK_F24.0).contains(&vk) {
        return Some(format!("f{}", vk - VK_F1.0 + 1));
    }
    if (b'A' as u16..=b'Z' as u16).contains(&vk) || (b'0' as u16..=b'9' as u16).contains(&vk) {
        return Some((vk as u8 as char).to_string());
    }
    match vk {
        0xC0 => Some("`".into()),
        0xBD => Some("-".into()),
        0xBB => Some("=".into()),
        0xDB => Some("[".into()),
        0xDD => Some("]".into()),
        0xDC => Some("\\".into()),
        0xBA => Some(";".into()),
        0xDE => Some("'".into()),
        0xBC => Some(",".into()),
        0xBE => Some(".".into()),
        0xBF => Some("/".into()),
        _ => None,
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

unsafe fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    text: &str,
    fmt: &IDWriteTextFormat,
    rect: D2D_RECT_F,
    brush: &ID2D1SolidColorBrush,
) {
    // SAFETY: Draws UTF-16 text into an active Direct2D BeginDraw/EndDraw block.
    unsafe {
        rt.DrawText(
            &wide(text),
            fmt,
            &rect,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_sidebar(
    rt: &ID2D1HwndRenderTarget,
    brush: &ID2D1SolidColorBrush,
    fm: &Formats,
    p: &Palette,
    height: f32,
    active: Page,
    hits: &mut Vec<(Rect, HitAction)>,
    hovered: &Option<HitAction>,
) {
    // SAFETY: Caller has active BeginDraw on rt.
    unsafe {
        brush.SetColor(&p.sidebar);
        rt.FillRectangle(
            &D2D_RECT_F {
                left: 0.0,
                top: 0.0,
                right: SIDEBAR_W,
                bottom: height,
            },
            brush,
        );
        brush.SetColor(&p.card_border);
        rt.FillRectangle(
            &D2D_RECT_F {
                left: SIDEBAR_W - 1.0,
                top: 0.0,
                right: SIDEBAR_W,
                bottom: height,
            },
            brush,
        );

        brush.SetColor(&p.accent);
        rt.FillRoundedRectangle(
            &Rect {
                x: 16.0,
                y: 22.0,
                w: 28.0,
                h: 28.0,
            }
            .rounded(7.0),
            brush,
        );
        brush.SetColor(&p.accent_text);
        draw_text(
            rt,
            "\u{E770}",
            &fm.glyph,
            Rect {
                x: 16.0,
                y: 22.0,
                w: 28.0,
                h: 28.0,
            }
            .d2d(),
            brush,
        );
        brush.SetColor(&p.text);
        draw_text(
            rt,
            crate::channel::NAME,
            &fm.h2,
            Rect {
                x: 52.0,
                y: 18.0,
                w: SIDEBAR_W - 60.0,
                h: 36.0,
            }
            .d2d(),
            brush,
        );

        let mut y = 74.0;
        for item in Page::ALL {
            let r = Rect {
                x: 12.0,
                y,
                w: SIDEBAR_W - 24.0,
                h: 38.0,
            };
            let action = HitAction::SelectPage(item);
            let selected = item == active;
            let is_hover = hovered.as_ref() == Some(&action);

            if selected {
                brush.SetColor(&p.accent);
                rt.FillRoundedRectangle(
                    &Rect {
                        x: r.x,
                        y: r.y + 8.0,
                        w: 3.5,
                        h: r.h - 16.0,
                    }
                    .rounded(1.75),
                    brush,
                );
                brush.SetColor(&p.hover);
                rt.FillRoundedRectangle(&r.rounded(8.0), brush);
            } else if is_hover {
                brush.SetColor(&p.hover);
                rt.FillRoundedRectangle(&r.rounded(8.0), brush);
            }

            brush.SetColor(if selected { &p.accent } else { &p.subtext });
            draw_text(
                rt,
                item.icon(),
                &fm.glyph,
                Rect {
                    x: r.x + 10.0,
                    y: r.y,
                    w: 22.0,
                    h: r.h,
                }
                .d2d(),
                brush,
            );
            brush.SetColor(&p.text);
            draw_text(
                rt,
                item.label(),
                if selected { &fm.body_semi } else { &fm.body },
                Rect {
                    x: r.x + 38.0,
                    y: r.y,
                    w: r.w - 44.0,
                    h: r.h,
                }
                .d2d(),
                brush,
            );
            hits.push((r, action));
            y += 44.0;
        }
    }
}

struct Layout<'a> {
    rt: &'a ID2D1HwndRenderTarget,
    brush: &'a ID2D1SolidColorBrush,
    fm: &'a Formats,
    p: &'a Palette,
    x: f32,
    w: f32,
    y: f32,
    hits: &'a mut Vec<(Rect, HitAction)>,
    hovered_idx: Option<usize>,
}

impl<'a> Layout<'a> {
    unsafe fn heading(&mut self, title: &str, subtitle: &str) {
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            self.brush.SetColor(&self.p.text);
            draw_text(
                self.rt,
                title,
                &self.fm.h1,
                Rect {
                    x: self.x,
                    y: self.y,
                    w: self.w,
                    h: 30.0,
                }
                .d2d(),
                self.brush,
            );
            self.y += 32.0;
            if !subtitle.is_empty() {
                self.brush.SetColor(&self.p.subtext);
                draw_text(
                    self.rt,
                    subtitle,
                    &self.fm.small,
                    Rect {
                        x: self.x,
                        y: self.y,
                        w: self.w,
                        h: 22.0,
                    }
                    .d2d(),
                    self.brush,
                );
                self.y += 26.0;
            }
            self.y += 6.0;
        }
    }

    unsafe fn section_label(&mut self, label: &str) {
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            self.brush.SetColor(&self.p.subtext);
            draw_text(
                self.rt,
                label,
                &self.fm.body_semi,
                Rect {
                    x: self.x,
                    y: self.y,
                    w: self.w,
                    h: 22.0,
                }
                .d2d(),
                self.brush,
            );
            self.y += 28.0;
        }
    }

    unsafe fn banner(&mut self, msg: &str, warn: bool) {
        if msg.is_empty() {
            return;
        }
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let r = Rect {
                x: self.x,
                y: self.y,
                w: self.w,
                h: 38.0,
            };
            self.brush
                .SetColor(if warn { &self.p.warning_bg } else { &self.p.card });
            self.rt.FillRoundedRectangle(&r.rounded(8.0), self.brush);
            self.brush.SetColor(&self.p.card_border);
            self.rt.DrawRoundedRectangle(&r.rounded(8.0), self.brush, 1.0, None);
            self.brush
                .SetColor(if warn { &self.p.warning_text } else { &self.p.text });
            draw_text(
                self.rt,
                msg,
                &self.fm.body,
                Rect {
                    x: r.x + 12.0,
                    y: r.y,
                    w: r.w - 24.0,
                    h: r.h,
                }
                .d2d(),
                self.brush,
            );
            self.y += 46.0;
        }
    }

    unsafe fn draw_card_frame(&mut self, h: f32, title: &str, desc: &str) -> Rect {
        let r = Rect {
            x: self.x,
            y: self.y,
            w: self.w,
            h,
        };
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            self.brush.SetColor(&self.p.card);
            self.rt.FillRoundedRectangle(&r.rounded(10.0), self.brush);
            self.brush.SetColor(&self.p.card_border);
            self.rt.DrawRoundedRectangle(&r.rounded(10.0), self.brush, 1.0, None);

            self.brush.SetColor(&self.p.text);
            let title_y = if desc.is_empty() {
                r.y + (h - 22.0) * 0.5
            } else {
                r.y + 10.0
            };
            draw_text(
                self.rt,
                title,
                &self.fm.body_semi,
                Rect {
                    x: r.x + 14.0,
                    y: title_y,
                    w: (r.w - 210.0).max(160.0),
                    h: 22.0,
                }
                .d2d(),
                self.brush,
            );
            if !desc.is_empty() {
                self.brush.SetColor(&self.p.subtext);
                draw_text(
                    self.rt,
                    desc,
                    &self.fm.small,
                    Rect {
                        x: r.x + 14.0,
                        y: r.y + 32.0,
                        w: (r.w - 210.0).max(160.0),
                        h: h - 38.0,
                    }
                    .d2d(),
                    self.brush,
                );
            }
        }
        self.y += h + 10.0;
        r
    }

    unsafe fn toggle(&mut self, title: &str, desc: &str, on: bool, action: HitAction) {
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let h = if desc.is_empty() { 48.0 } else { 62.0 };
            let r = self.draw_card_frame(h, title, desc);
            let sw = Rect {
                x: r.x + r.w - 58.0,
                y: r.y + (r.h - 24.0) * 0.5,
                w: 44.0,
                h: 24.0,
            };
            self.brush.SetColor(if on { &self.p.accent } else { &self.p.pill_bg });
            self.rt.FillRoundedRectangle(&sw.rounded(12.0), self.brush);
            if !on {
                self.brush.SetColor(&self.p.card_border);
                self.rt.DrawRoundedRectangle(&sw.rounded(12.0), self.brush, 1.0, None);
            }
            let knob_x = if on { sw.x + 23.0 } else { sw.x + 3.0 };
            self.brush
                .SetColor(if on { &self.p.accent_text } else { &self.p.subtext });
            self.rt.FillRoundedRectangle(
                &Rect {
                    x: knob_x,
                    y: sw.y + 3.0,
                    w: 18.0,
                    h: 18.0,
                }
                .rounded(9.0),
                self.brush,
            );
            self.hits.push((r, action));
        }
    }

    unsafe fn choices(
        &mut self,
        title: &str,
        desc: &str,
        current: &str,
        key: &'static str,
        opts: &[(&'static str, &'static str)],
    ) {
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let r = self.draw_card_frame(78.0, title, desc);
            let pill_w = ((r.w - 28.0) / opts.len() as f32).min(148.0);
            let row_y = r.y + 42.0;
            let mut px = r.x + 14.0;
            for (val, label) in opts {
                let pr = Rect {
                    x: px,
                    y: row_y,
                    w: pill_w - 6.0,
                    h: 26.0,
                };
                let selected = *val == current;
                self.brush
                    .SetColor(if selected { &self.p.accent } else { &self.p.pill_bg });
                self.rt.FillRoundedRectangle(&pr.rounded(6.0), self.brush);
                if !selected {
                    self.brush.SetColor(&self.p.card_border);
                    self.rt.DrawRoundedRectangle(&pr.rounded(6.0), self.brush, 1.0, None);
                }
                self.brush
                    .SetColor(if selected { &self.p.accent_text } else { &self.p.text });
                draw_text(self.rt, label, &self.fm.center_small, pr.d2d(), self.brush);
                self.hits.push((pr, HitAction::SetChoice { key, value: val }));
                px += pill_w;
            }
        }
    }

    unsafe fn stepper(
        &mut self,
        title: &str,
        desc: &str,
        value: i64,
        suffix: &str,
        key: &'static str,
        range: std::ops::RangeInclusive<i64>,
    ) {
        let (min, max) = (*range.start(), *range.end());
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let r = self.draw_card_frame(62.0, title, desc);
            let minus = Rect {
                x: r.x + r.w - 134.0,
                y: r.y + 17.0,
                w: 28.0,
                h: 28.0,
            };
            let plus = Rect {
                x: r.x + r.w - 42.0,
                y: r.y + 17.0,
                w: 28.0,
                h: 28.0,
            };
            let val_r = Rect {
                x: r.x + r.w - 104.0,
                y: r.y + 17.0,
                w: 60.0,
                h: 28.0,
            };
            for (btn, label) in [(minus, "-"), (plus, "+")] {
                self.brush.SetColor(&self.p.pill_bg);
                self.rt.FillRoundedRectangle(&btn.rounded(6.0), self.brush);
                self.brush.SetColor(&self.p.card_border);
                self.rt.DrawRoundedRectangle(&btn.rounded(6.0), self.brush, 1.0, None);
                self.brush.SetColor(&self.p.text);
                draw_text(self.rt, label, &self.fm.center_small, btn.d2d(), self.brush);
            }
            self.brush.SetColor(&self.p.text);
            draw_text(
                self.rt,
                &format!("{value}{suffix}"),
                &self.fm.center_small,
                val_r.d2d(),
                self.brush,
            );
            self.hits.push((
                minus,
                HitAction::StepInt {
                    key,
                    delta: -1,
                    min,
                    max,
                },
            ));
            self.hits.push((
                plus,
                HitAction::StepInt {
                    key,
                    delta: 1,
                    min,
                    max,
                },
            ));
        }
    }

    unsafe fn button_row(&mut self, title: &str, desc: &str, btn_label: &str, danger: bool, action: HitAction) {
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let r = self.draw_card_frame(62.0, title, desc);
            let br = Rect {
                x: r.x + r.w - 160.0,
                y: r.y + 16.0,
                w: 146.0,
                h: 30.0,
            };
            self.brush
                .SetColor(if danger { &self.p.danger } else { &self.p.accent });
            self.rt.FillRoundedRectangle(&br.rounded(6.0), self.brush);
            self.brush.SetColor(&self.p.accent_text);
            draw_text(self.rt, btn_label, &self.fm.center_small, br.d2d(), self.brush);
            self.hits.push((br, action));
        }
    }

    unsafe fn text_box_card(
        &mut self,
        title: &str,
        desc: &str,
        text: &str,
        placeholder: &str,
        field: TextField,
        active_input: &ActiveInput,
    ) {
        let caret = match active_input {
            ActiveInput::Text { field: f, caret, .. } if *f == field => Some(*caret),
            _ => None,
        };
        let active = caret.is_some();
        // SAFETY: Active BeginDraw on self.rt.
        unsafe {
            let r = self.draw_card_frame(82.0, title, desc);
            let box_r = Rect {
                x: r.x + 14.0,
                y: r.y + 44.0,
                w: r.w - 28.0,
                h: 28.0,
            };
            self.brush.SetColor(&self.p.pill_bg);
            self.rt.FillRoundedRectangle(&box_r.rounded(6.0), self.brush);
            self.brush
                .SetColor(if active { &self.p.accent } else { &self.p.card_border });
            self.rt
                .DrawRoundedRectangle(&box_r.rounded(6.0), self.brush, if active { 1.5 } else { 1.0 }, None);

            let shown = if text.is_empty() && !active {
                placeholder.to_string()
            } else if let Some(pos) = caret {
                let c = pos.min(text.len());
                format!("{}|{}", &text[..c], &text[c..])
            } else {
                text.to_string()
            };
            self.brush.SetColor(if text.is_empty() && !active {
                &self.p.subtext
            } else {
                &self.p.text
            });
            draw_text(
                self.rt,
                &shown,
                &self.fm.mono,
                Rect {
                    x: box_r.x + 8.0,
                    y: box_r.y,
                    w: box_r.w - 16.0,
                    h: box_r.h,
                }
                .d2d(),
                self.brush,
            );
            self.hits.push((box_r, HitAction::ActivateText { field }));
        }
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_onboarding(
    rt: &ID2D1HwndRenderTarget,
    brush: &ID2D1SolidColorBrush,
    fm: &Formats,
    p: &Palette,
    w: f32,
    _h: f32,
    step: usize,
    app: &SettingsApp,
    hits: &mut Vec<(Rect, HitAction)>,
) {
    let cw = (w - 80.0).clamp(400.0, 620.0);
    let cx = (w - cw) * 0.5;
    let mut layout = Layout {
        rt,
        brush,
        fm,
        p,
        x: cx,
        w: cw,
        y: 44.0,
        hits,
        hovered_idx: app.hovered,
    };

    // SAFETY: Active BeginDraw on rt.
    unsafe {
        match step {
            0 => {
                layout.heading(
                    &format!("Welcome to {}", crate::channel::NAME),
                    "100% native Win32 + Direct2D Windows 11 shell replacement with ~0% idle overhead.",
                );
                layout.toggle(
                    "Start with Windows",
                    "Launch automatically at user logon.",
                    app.autostart,
                    HitAction::ToggleAutostart,
                );
                layout.toggle(
                    "Neuter Stock Shell Hosts",
                    "Block StartMenuExperienceHost, SearchHost, Widgets & ShellExperienceHost.",
                    app.config.neuter_shell_hosts,
                    HitAction::ToggleBool {
                        key: "neuter_shell_hosts",
                    },
                );
                layout.toggle(
                    "Auto-Reap Background Explorer",
                    "Terminate idle explorer.exe once the last folder window closes.",
                    app.config.auto_reap_explorer,
                    HitAction::ToggleBool {
                        key: "auto_reap_explorer",
                    },
                );
            }
            1 => {
                layout.heading("Dock & Panel Layout", "Choose how your workspace bar behaves.");
                layout.choices(
                    "Display Mode",
                    "Floating dock, edge-docked panel (AppBar), or launcher only.",
                    &app.config.display_mode,
                    "display_mode",
                    &[("dock", "Dock"), ("panel", "Panel"), ("center", "Launcher Only")],
                );
                layout.toggle(
                    "Hide Windows Taskbar",
                    "Keep the stock taskbar hidden while LeanShell runs.",
                    app.config.hide_taskbar,
                    HitAction::ToggleBool { key: "hide_taskbar" },
                );
                layout.choices(
                    "Theme",
                    "Follow Windows dark/light mode or pin one explicitly.",
                    &app.config.theme,
                    "theme",
                    &[("system", "System"), ("dark", "Dark"), ("light", "Light")],
                );
            }
            _ => {
                layout.heading(
                    "Hybrid Window Management & Ready",
                    "Keyboard + mouse tiling and native switchers.",
                );
                layout.toggle(
                    "Hybrid Window Manager",
                    "Direct2D Alt+Tab & Win+Tab, Win+Arrow snap, drag-to-snap, Win+T tiling.",
                    app.config.wm_enabled,
                    HitAction::ToggleBool { key: "wm_enabled" },
                );
                layout.toggle(
                    "Focused Window Accent Border",
                    "Highlight the foreground window border via DWM.",
                    app.config.wm_borders,
                    HitAction::ToggleBool { key: "wm_borders" },
                );
                layout.banner(&app.status_message, false);
            }
        }

        let next_label = if step < 2 { "Next Step" } else { "Open Full Settings" };
        let action = if step < 2 {
            HitAction::OnboardingNext
        } else {
            HitAction::OnboardingFinish
        };
        layout.button_row(
            &format!("Step {} of 3", step + 1),
            "You can change any setting later in LeanShell Settings.",
            next_label,
            false,
            action,
        );
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn draw_page_content(
    rt: &ID2D1HwndRenderTarget,
    brush: &ID2D1SolidColorBrush,
    fm: &Formats,
    p: &Palette,
    x: f32,
    w: f32,
    _view_h: f32,
    app: &SettingsApp,
    hits: &mut Vec<(Rect, HitAction)>,
) -> f32 {
    let start_y = CONTENT_PAD - app.scroll_y;
    let mut l = Layout {
        rt,
        brush,
        fm,
        p,
        x,
        w,
        y: start_y,
        hits,
        hovered_idx: app.hovered,
    };

    // SAFETY: Active BeginDraw on rt; all hit rects are in window DIPs matching WM_LBUTTONDOWN.
    unsafe {
        if !app.status_message.is_empty() {
            l.banner(
                &app.status_message,
                app.status_message.contains("error")
                    || app.status_message.contains("Invalid")
                    || app.status_message.contains("Cannot"),
            );
        }

        match app.page {
            Page::General => {
                l.heading(
                    "General & System Shell",
                    "Configure startup, exclusive shell takeover, and launcher shortcut.",
                );
                l.toggle(
                    "Start with Windows",
                    "Run LeanShell automatically when you sign in.",
                    app.autostart,
                    HitAction::ToggleAutostart,
                );
                if app.exclusive_registered {
                    l.button_row(
                        "Exclusive Windows Shell",
                        "Registered in HKCU Winlogon\\Shell. Hold Shift at logon to bypass.",
                        "Restore Explorer",
                        true,
                        HitAction::UnregisterExclusiveShell,
                    );
                } else {
                    l.button_row(
                        "Exclusive Windows Shell",
                        "Boot straight into LeanShell instead of Explorer on logon.",
                        "Register Shell",
                        false,
                        HitAction::RegisterExclusiveShell,
                    );
                }
                l.toggle(
                    "Crash Watchdog",
                    "Automatically restore the taskbar if LeanShell exits unexpectedly.",
                    app.config.watchdog,
                    HitAction::ToggleBool { key: "watchdog" },
                );
                l.toggle(
                    "Claim Bare Windows Key",
                    "Open launcher on Win tap; chords like Win+L still pass through.",
                    app.config.claim_win_key,
                    HitAction::ToggleBool { key: "claim_win_key" },
                );
                l.toggle(
                    "Neuter Stock Shell Hosts",
                    "Block StartMenu, SearchHost, Widgets & ShellExperienceHost via IFEO.",
                    app.config.neuter_shell_hosts,
                    HitAction::ToggleBool {
                        key: "neuter_shell_hosts",
                    },
                );
                l.toggle(
                    "Auto-Reap Explorer",
                    "Quit idle explorer.exe once its last file browser window closes.",
                    app.config.auto_reap_explorer,
                    HitAction::ToggleBool {
                        key: "auto_reap_explorer",
                    },
                );
                l.toggle(
                    "Native Shell Chords & Volume",
                    "Handle Win+E, Win+R, Win+D, Win+I, Win+X and Volume OSD natively.",
                    app.config.claim_shell_chords,
                    HitAction::ToggleBool {
                        key: "claim_shell_chords",
                    },
                );

                l.section_label("Launcher");
                let capturing = matches!(app.active_input, ActiveInput::HotkeyCapture);
                let hk_text = if capturing {
                    "Press shortcut..."
                } else {
                    "Change Shortcut"
                };
                l.button_row(
                    "Launcher Shortcut",
                    &format!("Current: {}", app.config.hotkey),
                    hk_text,
                    false,
                    HitAction::StartHotkeyCapture,
                );
                if crate::system::has_shortcut_conflict(&app.config.hotkey, app.powertoys_enabled) {
                    l.banner("Conflict: PowerToys Run is enabled and also uses Alt+Space.", true);
                }
                l.choices(
                    "Launcher Position",
                    "Where the search window opens on screen.",
                    &app.config.position,
                    "position",
                    &[("bottom", "Bottom"), ("center", "Center")],
                );
                l.stepper(
                    "Maximum Results",
                    "Visible rows in the search results list (1 to 20).",
                    app.config.max_results,
                    "",
                    "max_results",
                    1..=20,
                );
                l.toggle(
                    "Show Recent Apps",
                    "List recently launched apps when the search query is empty.",
                    app.config.show_recent,
                    HitAction::ToggleBool { key: "show_recent" },
                );
            }
            Page::DockPanel => {
                l.heading(
                    "Dock & Panel",
                    "Control the floating dock, docked AppBar panel, taskbar hiding, and icons.",
                );
                l.choices(
                    "Display Mode",
                    "Floating auto-hide dock, edge panel, or center launcher only.",
                    &app.config.display_mode,
                    "display_mode",
                    &[("dock", "Dock"), ("panel", "Panel"), ("center", "Center")],
                );
                l.choices(
                    "Panel Edge",
                    "Screen edge used in Panel mode.",
                    &app.config.panel_edge,
                    "panel_edge",
                    &[
                        ("bottom", "Bottom"),
                        ("top", "Top"),
                        ("left", "Left"),
                        ("right", "Right"),
                    ],
                );
                l.choices(
                    "Panel Alignment",
                    "Icon grouping alignment in Panel mode.",
                    &app.config.panel_align,
                    "panel_align",
                    &[("center", "Center"), ("left", "Left")],
                );
                l.toggle(
                    "Show Dock",
                    "Display running apps, system tray, volume, and clock.",
                    app.config.dock,
                    HitAction::ToggleBool { key: "dock" },
                );
                l.toggle(
                    "Auto-Hide Dock",
                    "Reveal when pointer touches the bottom edge (dock mode).",
                    app.config.dock_autohide,
                    HitAction::ToggleBool { key: "dock_autohide" },
                );
                l.toggle(
                    "Hide Windows Taskbar",
                    "Hide the stock taskbar window whenever LeanShell runs.",
                    app.config.hide_taskbar,
                    HitAction::ToggleBool { key: "hide_taskbar" },
                );
                l.toggle(
                    "Show Search Button",
                    "Include the launcher button on the dock.",
                    app.config.dock_search_button,
                    HitAction::ToggleBool {
                        key: "dock_search_button",
                    },
                );
                l.choices(
                    "Dock Monitor",
                    "Which monitor shows the dock.",
                    &app.config.dock_screen,
                    "dock_screen",
                    &[("pointer", "Follow Pointer"), ("main", "Main Display")],
                );
                l.choices(
                    "Window Hover List",
                    "Show per-window thumbnails/titles on dock icon hover.",
                    &app.config.window_list,
                    "window_list",
                    &[("multiple", "Multiple Only"), ("always", "Always"), ("off", "Off")],
                );
                l.stepper(
                    "Dock Icon Size",
                    "App icon size in DIPs (24 to 40).",
                    app.config.dock_icon_size,
                    " DIP",
                    "dock_icon_size",
                    24..=40,
                );
                l.button_row(
                    "Pinned Dock Apps",
                    &format!("Currently pinned: {} app(s)", app.pinned_count),
                    "Unpin All",
                    true,
                    HitAction::UnpinAll,
                );
            }
            Page::WindowManager => {
                l.heading(
                    "Hybrid Window Management",
                    "Direct2D Alt+Tab / Win+Tab switchers, snap zones, Master-Stack tiling, and DWM borders.",
                );
                l.toggle(
                    "Enable Hybrid Window Manager",
                    "Direct2D Alt+Tab & Win+Tab, Win+Arrow snap, drag-to-snap, and Win+T.",
                    app.config.wm_enabled,
                    HitAction::ToggleBool { key: "wm_enabled" },
                );
                l.toggle(
                    "Dynamic Master-Stack Tiling",
                    "Automatically arrange windows in tiled layouts (toggle anytime via Win+T).",
                    app.config.wm_tiling,
                    HitAction::ToggleBool { key: "wm_tiling" },
                );
                l.stepper(
                    "Tile & Snap Gap",
                    "Spacing between snapped or tiled windows (0 to 32 DIP).",
                    app.config.wm_gap,
                    " DIP",
                    "wm_gap",
                    0..=32,
                );
                l.toggle(
                    "Focused Window DWM Border",
                    "Color the active window's Windows 11 DWM border.",
                    app.config.wm_borders,
                    HitAction::ToggleBool { key: "wm_borders" },
                );
                l.text_box_card(
                    "Focused Border Color (#RRGGBB)",
                    "Press Enter after editing hex color, or pick a swatch below.",
                    &app.wm_border_color_buf,
                    "#0891B2",
                    TextField::WmBorderColor,
                    &app.active_input,
                );

                // Quick border swatches
                let mut sx = l.x;
                for (hex, name) in [
                    ("#0891B2", "Cyan"),
                    ("#2563EB", "Blue"),
                    ("#10B981", "Emerald"),
                    ("#F59E0B", "Amber"),
                    ("#EF4444", "Red"),
                    ("", "Default"),
                ] {
                    let sr = Rect {
                        x: sx,
                        y: l.y,
                        w: 72.0,
                        h: 26.0,
                    };
                    l.brush.SetColor(&l.p.pill_bg);
                    l.rt.FillRoundedRectangle(&sr.rounded(6.0), l.brush);
                    l.brush.SetColor(&l.p.card_border);
                    l.rt.DrawRoundedRectangle(&sr.rounded(6.0), l.brush, 1.0, None);
                    l.brush.SetColor(&l.p.text);
                    draw_text(l.rt, name, &l.fm.center_small, sr.d2d(), l.brush);
                    l.hits.push((
                        sr,
                        HitAction::ApplyColorSwatch {
                            key: "wm_border_color",
                            value: hex,
                        },
                    ));
                    sx += 78.0;
                }
                l.y += 38.0;
            }
            Page::Appearance => {
                l.heading(
                    "Appearance & Backdrop",
                    "Customize colors and DWM surface materials across the native Direct2D UI.",
                );
                l.choices(
                    "Theme",
                    "System follows Windows Personalization settings.",
                    &app.config.theme,
                    "theme",
                    &[("system", "System"), ("dark", "Dark"), ("light", "Light")],
                );
                l.choices(
                    "Surface Backdrop",
                    "DWM material behind launcher, dock, switchers, and OSDs.",
                    &app.config.backdrop,
                    "backdrop",
                    &[("acrylic", "Acrylic"), ("mica", "Mica"), ("solid", "Solid")],
                );
                l.text_box_card(
                    "Custom Surface Tint (#RRGGBB or #RRGGBBAA)",
                    "Leave empty to use default theme backdrop tint. Press Enter to save.",
                    &app.dock_color_buf,
                    "Empty (Theme default)",
                    TextField::DockColor,
                    &app.active_input,
                );

                let mut sx = l.x;
                for (hex, name) in [
                    ("", "Default"),
                    ("#0F172ACC", "Slate"),
                    ("#1E3A8AC0", "Cobalt"),
                    ("#134E4AC0", "Teal"),
                    ("#312E81C0", "Indigo"),
                ] {
                    let sr = Rect {
                        x: sx,
                        y: l.y,
                        w: 80.0,
                        h: 26.0,
                    };
                    l.brush.SetColor(&l.p.pill_bg);
                    l.rt.FillRoundedRectangle(&sr.rounded(6.0), l.brush);
                    l.brush.SetColor(&l.p.card_border);
                    l.rt.DrawRoundedRectangle(&sr.rounded(6.0), l.brush, 1.0, None);
                    l.brush.SetColor(&l.p.text);
                    draw_text(l.rt, name, &l.fm.center_small, sr.d2d(), l.brush);
                    l.hits.push((
                        sr,
                        HitAction::ApplyColorSwatch {
                            key: "dock_color",
                            value: hex,
                        },
                    ));
                    sx += 86.0;
                }
                l.y += 40.0;
                l.banner(
                    "Renderer: 100% Native Win32 + Direct2D (no external UI toolkit runtime).",
                    false,
                );
            }
            Page::Search => {
                l.heading(
                    "Search & File Indexing",
                    "Configure web fallback engine and local file search filters.",
                );
                l.text_box_card(
                    "Web Search URL",
                    "Use {query} as placeholder. Press Enter to apply.",
                    &app.search_url_buf,
                    "https://www.google.com/search?q={query}",
                    TextField::SearchUrl,
                    &app.active_input,
                );

                let mut sx = l.x;
                for (url, name) in [
                    ("https://www.google.com/search?q={query}", "Google"),
                    ("https://duckduckgo.com/?q={query}", "DuckDuckGo"),
                    ("https://www.bing.com/search?q={query}", "Bing"),
                    ("https://kagi.com/search?q={query}", "Kagi"),
                ] {
                    let pr = Rect {
                        x: sx,
                        y: l.y,
                        w: 96.0,
                        h: 26.0,
                    };
                    l.brush.SetColor(&l.p.pill_bg);
                    l.rt.FillRoundedRectangle(&pr.rounded(6.0), l.brush);
                    l.brush.SetColor(&l.p.card_border);
                    l.rt.DrawRoundedRectangle(&pr.rounded(6.0), l.brush, 1.0, None);
                    l.brush.SetColor(&l.p.text);
                    draw_text(l.rt, name, &l.fm.center_small, pr.d2d(), l.brush);
                    l.hits.push((pr, HitAction::ApplySearchPreset(url)));
                    sx += 102.0;
                }
                l.y += 38.0;

                l.toggle(
                    "File Search (? prefix)",
                    "Allow searching indexed local files directly in the launcher.",
                    app.config.file_search,
                    HitAction::ToggleBool { key: "file_search" },
                );
                l.toggle(
                    "Prefer Everything Index",
                    "Query voidtools Everything IPC when running; fallback to Windows Search.",
                    app.config.file_search_everything,
                    HitAction::ToggleBool {
                        key: "file_search_everything",
                    },
                );

                l.section_label("Excluded Folder Names (click chip to remove)");
                l.text_box_card(
                    "Add Excluded Folder Name",
                    "Type folder name (e.g. target) and press Enter or click Add.",
                    &app.exclude_input_buf,
                    "Folder name...",
                    TextField::ExcludeFolder,
                    &app.active_input,
                );
                let add_btn = Rect {
                    x: l.x + l.w - 110.0,
                    y: l.y - 4.0,
                    w: 110.0,
                    h: 28.0,
                };
                l.brush.SetColor(&l.p.accent);
                l.rt.FillRoundedRectangle(&add_btn.rounded(6.0), l.brush);
                l.brush.SetColor(&l.p.accent_text);
                draw_text(l.rt, "Add Exclusion", &l.fm.center_small, add_btn.d2d(), l.brush);
                l.hits.push((add_btn, HitAction::AddExcludeFolder));
                l.y += 34.0;

                let mut cx = l.x;
                for (idx, folder) in app.config.file_search_exclude.iter().enumerate() {
                    let chip_w = (folder.len() as f32 * 7.5 + 32.0).clamp(64.0, 200.0);
                    if cx + chip_w > l.x + l.w {
                        cx = l.x;
                        l.y += 30.0;
                    }
                    let cr = Rect {
                        x: cx,
                        y: l.y,
                        w: chip_w,
                        h: 24.0,
                    };
                    l.brush.SetColor(&l.p.card);
                    l.rt.FillRoundedRectangle(&cr.rounded(12.0), l.brush);
                    l.brush.SetColor(&l.p.card_border);
                    l.rt.DrawRoundedRectangle(&cr.rounded(12.0), l.brush, 1.0, None);
                    l.brush.SetColor(&l.p.text);
                    draw_text(
                        l.rt,
                        &format!("{folder}  \u{00D7}"),
                        &l.fm.center_small,
                        cr.d2d(),
                        l.brush,
                    );
                    l.hits.push((cr, HitAction::RemoveExcludeFolder(idx)));
                    cx += chip_w + 8.0;
                }
                l.y += 36.0;
            }
            Page::About => {
                l.heading(&format!("About {}", crate::channel::NAME), &app.update_status);
                l.choices(
                    "Automatic Updates",
                    "Download and hot-restart when idle (auto), notify in launcher/settings, or turn off.",
                    &app.config.auto_update,
                    "auto_update",
                    &[("auto", "Auto"), ("notify", "Notify"), ("off", "Off")],
                );
                l.stepper(
                    "Update Check Interval",
                    "Hours between background GitHub release checks (1 to 168).",
                    app.config.update_check_hours,
                    "h",
                    "update_check_hours",
                    1..=168,
                );
                if app.update_installed_restart {
                    l.button_row(
                        "Update Ready",
                        "Restart Settings to switch to the newly installed version.",
                        "Restart Now",
                        false,
                        HitAction::RestartAfterUpdate,
                    );
                } else if app.latest_release.is_some() {
                    l.button_row(
                        "New Release Found",
                        &app.update_status,
                        "Download & Install",
                        false,
                        HitAction::InstallUpdate,
                    );
                } else {
                    l.button_row(
                        "Software Updates",
                        "Verify SHA-256 checksums against GitHub Releases.",
                        if app.update_busy {
                            "Checking..."
                        } else {
                            "Check for Updates"
                        },
                        false,
                        HitAction::CheckUpdates,
                    );
                }
                l.button_row(
                    "Configuration File",
                    &app.config_path_display,
                    "Open config.toml",
                    false,
                    HitAction::OpenConfig,
                );
                l.button_row(
                    "Source Repository",
                    "MIT licensed. Built with native Rust, Win32 & Direct2D.",
                    "Open GitHub",
                    false,
                    HitAction::OpenGithub,
                );
                l.button_row(
                    "Issue Tracker",
                    "Report bugs or share performance traces.",
                    "Report Issue",
                    false,
                    HitAction::OpenBugReport,
                );
            }
        }
    }

    (l.y - start_y).max(0.0)
}

pub fn run(welcome: bool, page: Option<&str>) -> Result<(), String> {
    // SAFETY: GetModuleHandleW(None) retrieves current executable HINSTANCE.
    let instance = unsafe { GetModuleHandleW(None).map_err(|e| e.to_string())? };
    // SAFETY: Standard Win32 icon/cursor loading and WNDCLASSEXW registration for Settings UI.
    unsafe {
        let icon_handle = LoadImageW(
            Some(instance.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            32,
            32,
            LR_DEFAULTCOLOR,
        );
        let small_icon_handle = LoadImageW(
            Some(instance.into()),
            PCWSTR(1 as _),
            IMAGE_ICON,
            16,
            16,
            LR_DEFAULTCOLOR,
        );
        let fallback = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let h_icon = icon_handle
            .ok()
            .filter(|h| !h.is_invalid())
            .map(|h| HICON(h.0))
            .unwrap_or(fallback);
        let h_icon_sm = small_icon_handle
            .ok()
            .filter(|h| !h.is_invalid())
            .map(|h| HICON(h.0))
            .unwrap_or(fallback);

        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).map_err(|e| e.to_string())?,
            hIcon: h_icon,
            hIconSm: h_icon_sm,
            lpszClassName: WINDOW_CLASS,
            ..Default::default()
        };
        if RegisterClassExW(&wc) == 0 {
            return Err(format!(
                "RegisterClassExW failed: {}",
                windows::core::Error::from_thread()
            ));
        }

        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&raw mut work).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        let dpi_guess = 96.0f32;
        let w = (INIT_W_DIP * (dpi_guess / 96.0)).round() as i32;
        let h = (INIT_H_DIP * (dpi_guess / 96.0)).round() as i32;
        let x = work.left + ((work.right - work.left - w) / 2).max(0);
        let y = work.top + ((work.bottom - work.top - h) / 2).max(0);

        let title_str = if welcome {
            format!("Welcome to {}", crate::channel::NAME)
        } else {
            format!("{} Settings", crate::channel::NAME)
        };
        let title_wide = wide(&title_str);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            WINDOW_CLASS,
            PCWSTR(title_wide.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

        let dpi = GetDpiForWindow(hwnd).max(96) as f32;
        let scale = dpi / 96.0;
        let sw = (INIT_W_DIP * scale).round() as i32;
        let sh = (INIT_H_DIP * scale).round() as i32;
        let sx = work.left + ((work.right - work.left - sw) / 2).max(0);
        let sy = work.top + ((work.bottom - work.top - sh) / 2).max(0);
        let _ = SetWindowPos(hwnd, None, sx, sy, sw, sh, SWP_NOZORDER | SWP_NOACTIVATE);

        let app = Box::new(SettingsApp::new(hwnd, dpi, welcome, page)?);
        let raw_ptr = Box::into_raw(app);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, raw_ptr as isize);

        let _ = ShowWindow(hwnd, SW_SHOW);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCDESTROY {
        // SAFETY: Reclaims the SettingsApp Box pointer stored in GWLP_USERDATA exactly once on destroy.
        unsafe {
            let ptr = SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) as *mut SettingsApp;
            if !ptr.is_null() {
                drop(Box::from_raw(ptr));
            }
            PostQuitMessage(0);
        }
        return LRESULT(0);
    }

    // SAFETY: GWLP_USERDATA holds either 0 (before setup) or the heap-allocated SettingsApp owned by this HWND.
    let app_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsApp };
    let Some(app) = (unsafe { app_ptr.as_mut() }) else {
        // SAFETY: Default window handling prior to SettingsApp attachment.
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };

    match msg {
        WM_GETMINMAXINFO => {
            // SAFETY: lparam points to MINMAXINFO per WM_GETMINMAXINFO contract.
            unsafe {
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let scale = app.renderer.dpi / 96.0;
                let _ = GetSystemMetricsForDpi;
                info.ptMinTrackSize.x = (MIN_W_DIP * scale).round() as i32;
                info.ptMinTrackSize.y = (MIN_H_DIP * scale).round() as i32;
            }
            LRESULT(0)
        }
        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as u32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
            app.renderer.resize(width, height);
            app.invalidate();
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let dpi = (wparam.0 & 0xFFFF) as f32;
            app.renderer.set_dpi(dpi.max(96.0));
            // SAFETY: lparam points to suggested RECT for the new DPI monitor.
            unsafe {
                let suggested = &*(lparam.0 as *const RECT);
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            app.invalidate();
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            app.sync_title_bar();
            app.invalidate();
            LRESULT(0)
        }
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            // SAFETY: Paired BeginPaint/EndPaint around Direct2D render cycle.
            unsafe {
                let _ = BeginPaint(hwnd, &mut ps);
            }
            app.paint();
            // SAFETY: Matches BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &ps);
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let px = (lparam.0 as i16) as f32;
            let py = ((lparam.0 >> 16) as i16) as f32;
            let scale = 96.0 / app.renderer.dpi;
            app.handle_click(px * scale, py * scale);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !app.tracking_mouse {
                app.tracking_mouse = true;
                // SAFETY: Registers one-shot TME_LEAVE notification on this HWND.
                unsafe {
                    let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    });
                }
            }
            let px = (lparam.0 as i16) as f32;
            let py = ((lparam.0 >> 16) as i16) as f32;
            let scale = 96.0 / app.renderer.dpi;
            let (dx, dy) = (px * scale, py * scale);
            let next = app.hits.iter().rposition(|(r, _)| r.contains(dx, dy));
            if next != app.hovered {
                app.hovered = next;
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            app.tracking_mouse = false;
            if app.hovered.is_some() {
                app.hovered = None;
                app.invalidate();
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as i16) as f32;
            app.scroll_y = (app.scroll_y - delta * 0.45).clamp(0.0, app.max_scroll);
            app.invalidate();
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if app.handle_key_down(wparam.0 as u16) {
                LRESULT(0)
            } else {
                // SAFETY: Unhandled keys go to DefWindowProcW.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_CHAR => {
            if app.handle_char(wparam.0 as u16) {
                LRESULT(0)
            } else {
                // SAFETY: Unhandled characters go to DefWindowProcW.
                unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
            }
        }
        WM_APP_BG_EVENT => {
            app.drain_bg_events();
            LRESULT(0)
        }
        _ => {
            // SAFETY: Default handling for standard non-custom Win32 messages.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cli_page_names() {
        assert_eq!(Page::from_arg("general"), Page::General);
        assert_eq!(Page::from_arg("dock"), Page::DockPanel);
        assert_eq!(Page::from_arg("wm"), Page::WindowManager);
        assert_eq!(Page::from_arg("appearance"), Page::Appearance);
        assert_eq!(Page::from_arg("search"), Page::Search);
        assert_eq!(Page::from_arg("about"), Page::About);
    }

    #[test]
    fn maps_virtual_keys_to_hotkey_names() {
        assert_eq!(vk_to_hotkey_name(VK_SPACE.0).as_deref(), Some("space"));
        assert_eq!(vk_to_hotkey_name(b'K' as u16).as_deref(), Some("K"));
        assert_eq!(vk_to_hotkey_name(VK_F1.0).as_deref(), Some("f1"));
        assert_eq!(vk_to_hotkey_name(VK_BACK.0), None);
    }
}
