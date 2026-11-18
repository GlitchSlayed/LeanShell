//! Hybrid Window Manager (Phase 8 / Track T4):
//! - Direct2D `Alt+Tab` & `Win+Tab` Task View switcher (mouse + keyboard)
//! - Mouse drag-to-snap & tile slot swap/resize preview overlay
//! - `Win+Arrow` keyboard snap state machine
//! - Dynamic Master-Stack tiling per monitor (`Win+T` / menu / launcher)
//! - DWM active window border styling with clean restoration

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use leanshell_core::config::{Backdrop, Config};
use leanshell_core::wm::{
    DipRect, SnapAction, SnapDirection, SnapZone, SwitcherHit, SwitcherLayout, WmRect, compute_tile_rects,
    detect_mouse_snap_zone, hit_tile_slot, infer_master_ratio, next_keyboard_snap,
};
use leanshell_platform_win::{
    MoveSizeHook, ShellChord, active_monitor_work_rect, apply_snap_zone, cursor_monitor_work_rect,
    get_visible_window_rect, is_manageable_window, list_tileable_windows_on_monitor, monitor_work_rect_for_window,
    parse_colorref, reset_all_window_borders, set_window_border_color, set_window_rect_exact,
};
use windows::Win32::Foundation::{COLORREF, D2DERR_RECREATE_TARGET, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_LEFT,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, Result, w};

use crate::commands::{self, open_windows};
use crate::dock::desktops;
use crate::indexer::IconPixels;
use crate::messages::{WM_WM_ACTION, WM_WM_MOVESIZE};
use crate::render::Fonts;
use crate::theme::{self, Palette, Tint};

const SWITCHER_CLASS: PCWSTR = w!("LeanShell.Switcher");
const OVERLAY_CLASS: PCWSTR = w!("LeanShell.SnapOverlay");
pub const WM_DRAG_TIMER: usize = 6;
const DRAG_POLL_MS: u32 = 16;
const EDGE_THRESHOLD_DIP: f32 = 18.0;
const CORNER_SPAN_DIP: f32 = 96.0;

thread_local! {
    static SWITCHER_STATE: RefCell<Option<SwitcherState>> = const { RefCell::new(None) };
    static OVERLAY_STATE: RefCell<Option<OverlayState>> = const { RefCell::new(None) };
}

fn d2d_rect(left: f32, top: f32, right: f32, bottom: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    }
}

fn dip_to_d2d(r: DipRect) -> D2D_RECT_F {
    d2d_rect(r.left, r.top, r.right, r.bottom)
}

fn rounded_rect(rect: D2D_RECT_F, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT {
        rect,
        radiusX: radius,
        radiusY: radius,
    }
}

fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    brush: &ID2D1SolidColorBrush,
    format: &IDWriteTextFormat,
    value: &str,
    area: D2D_RECT_F,
) {
    let wide: Vec<u16> = value.encode_utf16().collect();
    // SAFETY: `wide` holds valid UTF-16 text for `DrawText`.
    unsafe {
        rt.DrawText(
            &wide,
            format,
            &area,
            brush,
            D2D1_DRAW_TEXT_OPTIONS_CLIP,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

fn make_ellipsized(dw: &IDWriteFactory, format: IDWriteTextFormat) -> Result<IDWriteTextFormat> {
    // SAFETY: Creating ellipsis trimming sign and applying to a valid DirectWrite format.
    unsafe {
        let sign = dw.CreateEllipsisTrimmingSign(&format)?;
        let trimming = DWRITE_TRIMMING {
            granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
            delimiter: 0,
            delimiterCount: 0,
        };
        format.SetTrimming(&trimming, &sign)?;
        Ok(format)
    }
}

struct SwitcherFormats {
    title: IDWriteTextFormat,
    subtitle: IDWriteTextFormat,
    pill: IDWriteTextFormat,
    letter: IDWriteTextFormat,
    close: IDWriteTextFormat,
}

impl SwitcherFormats {
    fn new(dw: &IDWriteFactory) -> Result<Self> {
        let fonts = Fonts::installed(dw);
        // SAFETY: Creating DirectWrite formats from installed system fonts.
        unsafe {
            let make = |family, weight, size, align, para, wrap| -> Result<IDWriteTextFormat> {
                let fmt = dw.CreateTextFormat(
                    family,
                    None,
                    weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    w!("en-us"),
                )?;
                fmt.SetTextAlignment(align)?;
                fmt.SetParagraphAlignment(para)?;
                fmt.SetWordWrapping(wrap)?;
                Ok(fmt)
            };
            Ok(Self {
                title: make_ellipsized(
                    dw,
                    make(
                        fonts.text,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        13.0,
                        DWRITE_TEXT_ALIGNMENT_LEADING,
                        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                        DWRITE_WORD_WRAPPING_NO_WRAP,
                    )?,
                )?,
                subtitle: make_ellipsized(
                    dw,
                    make(
                        fonts.text,
                        DWRITE_FONT_WEIGHT_NORMAL,
                        11.5,
                        DWRITE_TEXT_ALIGNMENT_LEADING,
                        DWRITE_PARAGRAPH_ALIGNMENT_NEAR,
                        DWRITE_WORD_WRAPPING_NO_WRAP,
                    )?,
                )?,
                pill: make_ellipsized(
                    dw,
                    make(
                        fonts.text,
                        DWRITE_FONT_WEIGHT_SEMI_BOLD,
                        11.5,
                        DWRITE_TEXT_ALIGNMENT_CENTER,
                        DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                        DWRITE_WORD_WRAPPING_NO_WRAP,
                    )?,
                )?,
                letter: make(
                    fonts.display,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    15.0,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                )?,
                close: make(
                    fonts.text,
                    DWRITE_FONT_WEIGHT_SEMI_BOLD,
                    12.0,
                    DWRITE_TEXT_ALIGNMENT_CENTER,
                    DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
                    DWRITE_WORD_WRAPPING_NO_WRAP,
                )?,
            })
        }
    }
}

struct D2dTarget {
    rt: ID2D1HwndRenderTarget,
    brush: ID2D1SolidColorBrush,
    bitmaps: HashMap<String, ID2D1Bitmap>,
    size: D2D_SIZE_U,
}

struct SwitcherState {
    hwnd: HWND,
    launcher_hwnd: HWND,
    factory: ID2D1Factory,
    formats: SwitcherFormats,
    target: Option<D2dTarget>,
    visible: bool,
    task_view: bool,
    hiding: bool,
    windows: Vec<open_windows::Window>,
    desktops: desktops::Desktops,
    tiling_enabled: bool,
    selected: usize,
    hover_hit: SwitcherHit,
    layout: SwitcherLayout,
    dpi: u32,
    palette: Palette,
    backdrop: Backdrop,
    dark: bool,
    tint: Option<Tint>,
    icons: HashMap<String, Arc<IconPixels>>,
}

impl SwitcherState {
    fn create(
        launcher_hwnd: HWND,
        palette: Palette,
        dark: bool,
        backdrop: Backdrop,
        tint: Option<Tint>,
    ) -> Result<Self> {
        // SAFETY: Initializing Direct2D factory, DirectWrite formats, and hidden switcher HWND.
        unsafe {
            let factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dw: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let formats = SwitcherFormats::new(&dw)?;
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(switcher_wndproc),
                hInstance: instance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                lpszClassName: SWITCHER_CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0
                && windows::Win32::Foundation::GetLastError() != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
            {
                return Err(windows::core::Error::from_thread());
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                SWITCHER_CLASS,
                w!("LeanShell Switcher"),
                WS_POPUP | WS_CAPTION,
                0,
                0,
                200,
                120,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let applied = theme::apply_tinted_backdrop(hwnd, dark, backdrop, true, tint);
            let _ = palette;
            Ok(Self {
                hwnd,
                launcher_hwnd,
                factory,
                formats,
                target: None,
                visible: false,
                task_view: false,
                hiding: false,
                windows: Vec::new(),
                desktops: desktops::Desktops {
                    current: 0,
                    names: vec!["Desktop 1".into()],
                },
                tiling_enabled: false,
                selected: 0,
                hover_hit: SwitcherHit::None,
                layout: SwitcherLayout::compute(0, 1, false, 4),
                dpi: 96,
                palette: applied,
                backdrop,
                dark,
                tint,
                icons: HashMap::new(),
            })
        }
    }

    fn apply_theme(&mut self, dark: bool, backdrop: Backdrop, tint: Option<Tint>) {
        self.dark = dark;
        self.backdrop = backdrop;
        self.tint = tint;
        self.palette = theme::apply_tinted_backdrop(self.hwnd, dark, backdrop, true, tint);
        if self.visible {
            // SAFETY: Requesting repaint of visible switcher window after theme change.
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    fn open(&mut self, task_view: bool, step: isize, tiling_enabled: bool, icons: &HashMap<String, Arc<IconPixels>>) {
        let was_visible = self.visible && self.task_view == task_view;
        self.task_view = task_view;
        self.tiling_enabled = tiling_enabled;
        if !was_visible {
            self.windows = open_windows::list();
            self.desktops = desktops::read();
            self.icons.clear();
            let icon_px = ((32.0 * self.dpi as f32 / 96.0).round() as i32).max(24);
            for w in &self.windows {
                if let Some(key) = w.icon_key() {
                    if let Some(cached) = icons.get(&key) {
                        self.icons.insert(key, cached.clone());
                    } else if let Some(loaded) = commands::exe_icon(&w.exe, icon_px) {
                        self.icons.insert(key, Arc::new(loaded));
                    }
                }
            }
            if self.windows.is_empty() {
                self.selected = 0;
            } else if !task_view && self.windows.len() > 1 && step > 0 {
                self.selected = 1;
            } else if !task_view && self.windows.len() > 1 && step < 0 {
                self.selected = self.windows.len() - 1;
            } else {
                self.selected = 0;
            }
            self.hover_hit = SwitcherHit::None;
        } else if !self.windows.is_empty() {
            let len = self.windows.len() as isize;
            self.selected = (self.selected as isize + step).rem_euclid(len) as usize;
        }

        self.relayout_and_show();
    }

    fn relayout_and_show(&mut self) {
        let (_, _, work, dpi) = cursor_monitor_work_rect();
        self.dpi = dpi;
        let scale = dpi as f32 / 96.0;
        let max_cols = (((work.width() as f32 / scale) - 64.0) / (SwitcherLayout::CARD_W + SwitcherLayout::GAP))
            .floor()
            .clamp(1.0, 5.0) as usize;
        self.layout = SwitcherLayout::compute(self.windows.len(), self.desktops.names.len(), self.task_view, max_cols);

        let w = (self.layout.width * scale).ceil() as i32;
        let h = (self.layout.height * scale).ceil() as i32;
        let x = work.left + (work.width() - w) / 2;
        let y = work.top + (work.height() - h) / 2;
        theme::shape(self.hwnd, w, h, 10.0, dpi);
        self.visible = true;
        // SAFETY: Positioning, showing, and focusing the switcher overlay window.
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_SHOWWINDOW);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = SetFocus(Some(self.hwnd));
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.hiding = true;
        self.visible = false;
        self.hover_hit = SwitcherHit::None;
        // SAFETY: Hiding switcher overlay window cleanly.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.hiding = false;
    }

    fn activate_selected(&mut self) {
        let target = self.windows.get(self.selected).map(|w| w.hwnd);
        self.hide();
        if let Some(hwnd) = target {
            let _ = open_windows::focus(hwnd);
        }
    }

    fn close_card(&mut self, index: usize) {
        let Some(win) = self.windows.get(index) else {
            return;
        };
        let target = win.hwnd;
        let _ = open_windows::close(target);
        self.windows.remove(index);
        if self.selected >= self.windows.len() {
            self.selected = self.windows.len().saturating_sub(1);
        }
        if !self.task_view && self.windows.is_empty() {
            self.hide();
        } else {
            self.relayout_and_show();
        }
    }

    fn click_hit(&mut self, hit: SwitcherHit) {
        match hit {
            SwitcherHit::Card(idx) => {
                if idx < self.windows.len() {
                    self.selected = idx;
                    self.activate_selected();
                }
            }
            SwitcherHit::CloseCard(idx) => {
                self.close_card(idx);
            }
            SwitcherHit::DesktopPill(idx) => {
                let current = self.desktops.current;
                if idx != current && idx < self.desktops.names.len() {
                    let right = idx > current;
                    let steps = idx.abs_diff(current);
                    for _ in 0..steps {
                        desktops::switch(right);
                    }
                    self.desktops.current = idx;
                    // SAFETY: Requesting repaint after switching virtual desktop.
                    unsafe {
                        let _ = InvalidateRect(Some(self.hwnd), None, false);
                    }
                }
            }
            SwitcherHit::ToggleTiling => {
                self.tiling_enabled = !self.tiling_enabled;
                // SAFETY: Notifying launcher on UI thread to toggle monitor tiling.
                unsafe {
                    let _ = PostMessageW(Some(self.launcher_hwnd), WM_WM_ACTION, WPARAM(2), LPARAM(0));
                    let _ = InvalidateRect(Some(self.hwnd), None, false);
                }
            }
            SwitcherHit::None => {}
        }
    }

    fn paint(&mut self) -> Result<()> {
        // SAFETY: Reading client bounds and painting Direct2D switcher frame.
        unsafe {
            let mut client = RECT::default();
            GetClientRect(self.hwnd, &mut client)?;
            let size = D2D_SIZE_U {
                width: client.right.max(1) as u32,
                height: client.bottom.max(1) as u32,
            };
            if self.target.is_none() {
                let rt = self.factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                    },
                )?;
                rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
                let brush = rt.CreateSolidColorBrush(&self.palette.text, None)?;
                self.target = Some(D2dTarget {
                    rt,
                    brush,
                    bitmaps: HashMap::new(),
                    size,
                });
            }
            let Some(target) = self.target.as_mut() else {
                return Ok(());
            };
            if target.size.width != size.width || target.size.height != size.height {
                target.rt.Resize(&size)?;
                target.size = size;
            }
            target.rt.SetDpi(self.dpi as f32, self.dpi as f32);
            target.rt.BeginDraw();
            theme::paint_background(&target.rt, &target.brush, &self.palette, 10.0);

            if self.task_view {
                for (i, &pill) in self.layout.desktop_pills.iter().enumerate() {
                    let active = i == self.desktops.current;
                    let hovered = self.hover_hit == SwitcherHit::DesktopPill(i);
                    let fill = if active {
                        self.palette.accent
                    } else if hovered {
                        self.palette.row_selected
                    } else {
                        self.palette.tile
                    };
                    target.brush.SetColor(&fill);
                    target
                        .rt
                        .FillRoundedRectangle(&rounded_rect(dip_to_d2d(pill), 6.0), &target.brush);
                    let text_col = if active {
                        self.palette.background_solid
                    } else {
                        self.palette.text
                    };
                    target.brush.SetColor(&text_col);
                    let name = self.desktops.names.get(i).map(String::as_str).unwrap_or("Desktop");
                    draw_text(&target.rt, &target.brush, &self.formats.pill, name, dip_to_d2d(pill));
                }

                if let Some(btn) = self.layout.tiling_button {
                    let hovered = self.hover_hit == SwitcherHit::ToggleTiling;
                    let fill = if self.tiling_enabled {
                        self.palette.accent
                    } else if hovered {
                        self.palette.row_selected
                    } else {
                        self.palette.tile
                    };
                    target.brush.SetColor(&fill);
                    target
                        .rt
                        .FillRoundedRectangle(&rounded_rect(dip_to_d2d(btn), 6.0), &target.brush);
                    let text_col = if self.tiling_enabled {
                        self.palette.background_solid
                    } else {
                        self.palette.text
                    };
                    target.brush.SetColor(&text_col);
                    let label = if self.tiling_enabled {
                        "Tiling: On"
                    } else {
                        "Tiling: Off"
                    };
                    draw_text(&target.rt, &target.brush, &self.formats.pill, label, dip_to_d2d(btn));
                }
            }

            if self.windows.is_empty() {
                target.brush.SetColor(&self.palette.subtext);
                draw_text(
                    &target.rt,
                    &target.brush,
                    &self.formats.pill,
                    "No open windows",
                    d2d_rect(0.0, 42.0, self.layout.width, self.layout.height - 12.0),
                );
            } else {
                let raw_rt: ID2D1RenderTarget = windows::core::Interface::cast(&target.rt)?;
                for (i, win) in self.windows.iter().enumerate() {
                    let Some(&card) = self.layout.cards.get(i) else {
                        continue;
                    };
                    let selected = i == self.selected;
                    let hovered =
                        matches!(self.hover_hit, SwitcherHit::Card(idx) | SwitcherHit::CloseCard(idx) if idx == i);
                    let fill = if selected || hovered {
                        self.palette.row_selected
                    } else {
                        self.palette.tile
                    };
                    target.brush.SetColor(&fill);
                    target
                        .rt
                        .FillRoundedRectangle(&rounded_rect(dip_to_d2d(card), 8.0), &target.brush);
                    if selected {
                        target.brush.SetColor(&self.palette.accent);
                        target
                            .rt
                            .DrawRoundedRectangle(&rounded_rect(dip_to_d2d(card), 8.0), &target.brush, 1.5, None);
                    }

                    // Icon or letter badge
                    let icon_rect = d2d_rect(
                        card.left + 14.0,
                        card.top + (SwitcherLayout::CARD_H - 32.0) / 2.0,
                        card.left + 46.0,
                        card.top + (SwitcherLayout::CARD_H + 32.0) / 2.0,
                    );
                    let mut drew_icon = false;
                    if let Some(key) = win.icon_key()
                        && let Some(px) = self.icons.get(&key)
                        && let Some(bmp) = crate::render::bitmap(&mut target.bitmaps, &raw_rt, &key, px)
                    {
                        target
                            .rt
                            .DrawBitmap(&bmp, Some(&icon_rect), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                        drew_icon = true;
                    }
                    if !drew_icon {
                        target.brush.SetColor(&self.palette.row_selected);
                        target
                            .rt
                            .FillRoundedRectangle(&rounded_rect(icon_rect, 6.0), &target.brush);
                        let initial: String = open_windows::program_name(&win.exe)
                            .chars()
                            .chain(win.title.chars())
                            .find(|c| !c.is_whitespace())
                            .map(|c| c.to_uppercase().collect())
                            .unwrap_or_else(|| "W".into());
                        target.brush.SetColor(&self.palette.text);
                        draw_text(&target.rt, &target.brush, &self.formats.letter, &initial, icon_rect);
                    }

                    // Title & app name
                    let text_left = card.left + 56.0;
                    let text_right = card.right - 28.0;
                    target.brush.SetColor(&self.palette.text);
                    draw_text(
                        &target.rt,
                        &target.brush,
                        &self.formats.title,
                        &win.title,
                        d2d_rect(text_left, card.top + 20.0, text_right, card.top + 42.0),
                    );
                    target.brush.SetColor(&self.palette.subtext);
                    let app = open_windows::program_name(&win.exe);
                    draw_text(
                        &target.rt,
                        &target.brush,
                        &self.formats.subtitle,
                        if app.is_empty() { "Window" } else { app },
                        d2d_rect(text_left, card.top + 44.0, text_right, card.top + 64.0),
                    );

                    // Close [×] button
                    if let Some(&close) = self.layout.close_buttons.get(i) {
                        if self.hover_hit == SwitcherHit::CloseCard(i) {
                            target.brush.SetColor(&self.palette.row_selected);
                            target
                                .rt
                                .FillRoundedRectangle(&rounded_rect(dip_to_d2d(close), 4.0), &target.brush);
                            target.brush.SetColor(&self.palette.text);
                        } else {
                            target.brush.SetColor(&self.palette.subtext);
                        }
                        draw_text(&target.rt, &target.brush, &self.formats.close, "×", dip_to_d2d(close));
                    }
                }
            }

            let ended = target.rt.EndDraw(None, None);
            if ended.as_ref().is_err_and(|e| e.code() == D2DERR_RECREATE_TARGET) {
                self.target = None;
                return Ok(());
            }
            ended
        }
    }
}

struct OverlayState {
    hwnd: HWND,
    factory: ID2D1Factory,
    rt: Option<ID2D1HwndRenderTarget>,
    brush: Option<ID2D1SolidColorBrush>,
    size: D2D_SIZE_U,
    palette: Palette,
    visible: bool,
    current_rect: WmRect,
}

impl OverlayState {
    fn create(palette: Palette) -> Result<Self> {
        // SAFETY: Creating click-through layered translucent preview window for drag snap/tile preview.
        unsafe {
            let factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let instance = GetModuleHandleW(None)?;
            let class = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_wndproc),
                hInstance: instance.into(),
                lpszClassName: OVERLAY_CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&class) == 0
                && windows::Win32::Foundation::GetLastError() != windows::Win32::Foundation::ERROR_CLASS_ALREADY_EXISTS
            {
                return Err(windows::core::Error::from_thread());
            }
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
                OVERLAY_CLASS,
                w!("LeanShell Snap Preview"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(instance.into()),
                None,
            )?;
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 185, LWA_ALPHA);
            Ok(Self {
                hwnd,
                factory,
                rt: None,
                brush: None,
                size: D2D_SIZE_U::default(),
                palette,
                visible: false,
                current_rect: WmRect::default(),
            })
        }
    }

    fn show(&mut self, rect: WmRect) {
        if self.visible && self.current_rect == rect {
            return;
        }
        self.visible = true;
        self.current_rect = rect;
        // SAFETY: Showing click-through snap preview rectangle above windows without taking focus.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                rect.left,
                rect.top,
                rect.width().max(1),
                rect.height().max(1),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    fn hide(&mut self) {
        if !self.visible {
            return;
        }
        self.visible = false;
        // SAFETY: Hiding snap preview overlay.
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn paint(&mut self) -> Result<()> {
        // SAFETY: Painting translucent snap preview fill and accent outline with Direct2D.
        unsafe {
            let mut client = RECT::default();
            GetClientRect(self.hwnd, &mut client)?;
            let size = D2D_SIZE_U {
                width: client.right.max(1) as u32,
                height: client.bottom.max(1) as u32,
            };
            if self.rt.is_none() {
                let rt = self.factory.CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        ..Default::default()
                    },
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd: self.hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                    },
                )?;
                let brush = rt.CreateSolidColorBrush(&self.palette.accent, None)?;
                self.rt = Some(rt);
                self.brush = Some(brush);
                self.size = size;
            }
            let (Some(rt), Some(brush)) = (self.rt.as_ref(), self.brush.as_ref()) else {
                return Ok(());
            };
            if self.size.width != size.width || self.size.height != size.height {
                rt.Resize(&size)?;
                self.size = size;
            }
            rt.BeginDraw();
            let sz = rt.GetSize();
            rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
            let area = d2d_rect(2.0, 2.0, (sz.width - 2.0).max(3.0), (sz.height - 2.0).max(3.0));
            let fill = D2D1_COLOR_F {
                a: 0.25,
                ..self.palette.accent
            };
            brush.SetColor(&fill);
            rt.FillRoundedRectangle(&rounded_rect(area, 8.0), brush);
            brush.SetColor(&self.palette.accent);
            rt.DrawRoundedRectangle(&rounded_rect(area, 8.0), brush, 2.0, None);
            let ended = rt.EndDraw(None, None);
            if ended.as_ref().is_err_and(|e| e.code() == D2DERR_RECREATE_TARGET) {
                self.rt = None;
                self.brush = None;
                return Ok(());
            }
            ended
        }
    }
}

unsafe extern "system" fn switcher_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
        WM_NCACTIVATE => unsafe { DefWindowProcW(hwnd, msg, wparam, LPARAM(-1)) },
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: Standard BeginPaint/EndPaint cycle around thread-local SwitcherState paint.
            unsafe {
                BeginPaint(hwnd, &mut paint);
            }
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    let _ = state.paint();
                }
            });
            // SAFETY: Paired EndPaint for BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    let scale = state.dpi as f32 / 96.0;
                    let x = (lparam.0 & 0xFFFF) as i16 as f32 / scale;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / scale;
                    let hit = state.layout.hit_test(x, y);
                    if hit != state.hover_hit {
                        state.hover_hit = hit;
                        // SAFETY: Requesting repaint and WM_MOUSELEAVE on hover change.
                        unsafe {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                    // SAFETY: Registering one-shot TME_LEAVE on the switcher HWND.
                    unsafe {
                        let _ = TrackMouseEvent(&mut TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        });
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                    && state.hover_hit != SwitcherHit::None
                {
                    state.hover_hit = SwitcherHit::None;
                    // SAFETY: Repainting switcher after pointer exit.
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    let scale = state.dpi as f32 / 96.0;
                    let x = (lparam.0 & 0xFFFF) as i16 as f32 / scale;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / scale;
                    let hit = state.layout.hit_test(x, y);
                    state.click_hit(hit);
                }
            });
            LRESULT(0)
        }
        WM_MBUTTONUP => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    let scale = state.dpi as f32 / 96.0;
                    let x = (lparam.0 & 0xFFFF) as i16 as f32 / scale;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32 / scale;
                    if let SwitcherHit::Card(idx) | SwitcherHit::CloseCard(idx) = state.layout.hit_test(x, y) {
                        state.close_card(idx);
                    }
                }
            });
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wparam.0 >> 16) as u16 as i16) as i32;
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                    && !state.windows.is_empty()
                {
                    let step = if delta > 0 { -1 } else { 1 };
                    let len = state.windows.len() as isize;
                    state.selected = (state.selected as isize + step).rem_euclid(len) as usize;
                    // SAFETY: Repainting switcher selection after wheel scroll.
                    unsafe {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wparam.0 as u16;
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    // SAFETY: Reading physical Shift state for Shift+Tab backward cycle.
                    let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) < 0 };
                    let len = state.windows.len() as isize;
                    match vk {
                        x if x == VK_ESCAPE.0 => state.hide(),
                        x if x == VK_RETURN.0 => state.activate_selected(),
                        x if x == VK_DELETE.0 => {
                            if !state.windows.is_empty() {
                                state.close_card(state.selected);
                            }
                        }
                        x if x == b'T' as u16 && state.task_view => {
                            state.click_hit(SwitcherHit::ToggleTiling);
                        }
                        x if ((x == VK_TAB.0 && shift) || x == VK_LEFT.0 || x == VK_UP.0) && len > 0 => {
                            state.selected = (state.selected as isize - 1).rem_euclid(len) as usize;
                            // SAFETY: Repainting updated card selection.
                            unsafe {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        }
                        x if (x == VK_TAB.0 || x == VK_RIGHT.0 || x == VK_DOWN.0) && len > 0 => {
                            state.selected = (state.selected as isize + 1).rem_euclid(len) as usize;
                            // SAFETY: Repainting updated card selection.
                            unsafe {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        }
                        _ => {}
                    }
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                    && state.task_view
                    && !state.hiding
                {
                    state.hide();
                }
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            SWITCHER_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    state.hide();
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn overlay_wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: Standard BeginPaint/EndPaint cycle for snap preview overlay.
            unsafe {
                BeginPaint(hwnd, &mut paint);
            }
            OVERLAY_STATE.with(|cell| {
                if let Ok(mut slot) = cell.try_borrow_mut()
                    && let Some(state) = slot.as_mut()
                {
                    let _ = state.paint();
                }
            });
            // SAFETY: Paired EndPaint for BeginPaint above.
            unsafe {
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[derive(Default)]
struct MonitorTileState {
    enabled: bool,
    order: Vec<isize>,
    master_ratio: f32,
}

impl MonitorTileState {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            order: Vec::new(),
            master_ratio: 0.55,
        }
    }
}

struct DragSession {
    hwnd: isize,
    start_rect: WmRect,
    preview_snap: Option<SnapZone>,
    preview_swap_slot: Option<usize>,
    preview_monitor: isize,
}

/// Coordinates Direct2D switchers, mouse/keyboard snapping, per-monitor tiling,
/// and DWM active window borders.
pub struct WindowManager {
    launcher_hwnd: HWND,
    enabled: bool,
    default_tiling: bool,
    gap_dip: u32,
    borders: bool,
    border_color: Option<u32>,
    monitors: HashMap<isize, MonitorTileState>,
    snapped: HashMap<isize, SnapZone>,
    styled_foreground: Option<isize>,
    movesize_hook: Option<MoveSizeHook>,
    drag: Option<DragSession>,
    retiling: bool,
}

impl WindowManager {
    pub fn new(launcher_hwnd: HWND, config: &Config, palette: Palette, dark: bool, tint: Option<Tint>) -> Self {
        SWITCHER_STATE.with(|cell| {
            if cell.borrow().is_none()
                && let Ok(state) = SwitcherState::create(launcher_hwnd, palette, dark, config.backdrop, tint)
            {
                *cell.borrow_mut() = Some(state);
            }
        });
        OVERLAY_STATE.with(|cell| {
            if cell.borrow().is_none()
                && let Ok(state) = OverlayState::create(Palette::new(dark, true))
            {
                *cell.borrow_mut() = Some(state);
            }
        });

        let mut mgr = Self {
            launcher_hwnd,
            enabled: config.wm_enabled,
            default_tiling: config.wm_tiling,
            gap_dip: config.wm_gap.min(32),
            borders: config.wm_borders,
            border_color: Self::resolve_border_color(config),
            monitors: HashMap::new(),
            snapped: HashMap::new(),
            styled_foreground: None,
            movesize_hook: None,
            drag: None,
            retiling: false,
        };
        mgr.sync_hooks();
        if mgr.enabled && mgr.default_tiling {
            let (hmon, _, _, _) = active_monitor_work_rect();
            let entry = mgr.monitors.entry(hmon).or_insert_with(|| MonitorTileState::new(true));
            entry.enabled = true;
            mgr.retile_monitor(hmon);
        }
        mgr
    }

    pub fn switcher_visible(&self) -> bool {
        SWITCHER_STATE.with(|cell| cell.borrow().as_ref().is_some_and(|s| s.visible))
    }

    fn resolve_border_color(config: &Config) -> Option<u32> {
        if config.wm_border_color.trim().is_empty() {
            None
        } else {
            parse_colorref(&config.wm_border_color).or(Some(0x00B29108))
        }
    }

    fn sync_hooks(&mut self) {
        if self.enabled {
            if self.movesize_hook.is_none()
                && let Ok(hook) = MoveSizeHook::install(self.launcher_hwnd, WM_WM_MOVESIZE)
            {
                self.movesize_hook = Some(hook);
            }
        } else {
            self.movesize_hook = None;
            self.stop_drag();
        }
    }

    pub fn sync_config(&mut self, config: &Config, palette: Palette, dark: bool, tint: Option<Tint>) {
        self.enabled = config.wm_enabled;
        self.default_tiling = config.wm_tiling;
        self.gap_dip = config.wm_gap.min(32);
        let borders_changed =
            self.borders != config.wm_borders || self.border_color != Self::resolve_border_color(config);
        self.borders = config.wm_borders;
        self.border_color = Self::resolve_border_color(config);
        self.sync_hooks();

        SWITCHER_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.apply_theme(dark, config.backdrop, tint);
            }
        });
        OVERLAY_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.palette = palette;
            }
        });

        if !self.enabled || !self.borders {
            if let Some(prev) = self.styled_foreground.take() {
                set_window_border_color(HWND(prev as _), None);
            }
            if borders_changed {
                reset_all_window_borders();
            }
        } else if borders_changed {
            // SAFETY: Re-applying updated DWM border color to current foreground window.
            let fg = unsafe { GetForegroundWindow() };
            self.on_foreground_changed(fg);
        }

        if self.enabled {
            self.retile_all_enabled();
        }
    }

    pub fn is_tiling_active_monitor(&self) -> bool {
        let (hmon, _, _, _) = active_monitor_work_rect();
        self.monitors
            .get(&hmon)
            .map_or(self.default_tiling, |state| state.enabled)
    }

    pub fn on_chord(&mut self, chord: ShellChord, previous_fg: HWND, icons: &HashMap<String, Arc<IconPixels>>) {
        if !self.enabled {
            return;
        }
        match chord {
            ShellChord::AltTabNext => self.open_switcher(false, 1, icons),
            ShellChord::AltTabPrev => self.open_switcher(false, -1, icons),
            ShellChord::AltReleased => {
                SWITCHER_STATE.with(|cell| {
                    if let Ok(mut slot) = cell.try_borrow_mut()
                        && let Some(state) = slot.as_mut()
                        && state.visible
                        && !state.task_view
                    {
                        state.activate_selected();
                    }
                });
            }
            ShellChord::TaskView => {
                let already_task_view = SWITCHER_STATE.with(|cell| {
                    cell.try_borrow()
                        .ok()
                        .and_then(|s| s.as_ref().map(|st| st.visible && st.task_view))
                        .unwrap_or(false)
                });
                if already_task_view {
                    self.hide_switcher();
                } else {
                    self.open_switcher(true, 0, icons);
                }
            }
            ShellChord::SnapLeft => self.snap_active(previous_fg, SnapDirection::Left),
            ShellChord::SnapRight => self.snap_active(previous_fg, SnapDirection::Right),
            ShellChord::SnapUp => self.snap_active(previous_fg, SnapDirection::Up),
            ShellChord::SnapDown => self.snap_active(previous_fg, SnapDirection::Down),
            ShellChord::ToggleTiling => self.toggle_tiling_active_monitor(),
            _ => {}
        }
    }

    pub fn on_action(&mut self, action: usize, icons: &HashMap<String, Arc<IconPixels>>) {
        if !self.enabled {
            return;
        }
        match action {
            1 => self.open_switcher(true, 0, icons),
            2 => self.toggle_tiling_active_monitor(),
            3 => self.retile_all_enabled(),
            _ => {}
        }
    }

    fn open_switcher(&self, task_view: bool, step: isize, icons: &HashMap<String, Arc<IconPixels>>) {
        let tiling = self.is_tiling_active_monitor();
        SWITCHER_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.open(task_view, step, tiling, icons);
            }
        });
    }

    pub fn hide_switcher(&self) {
        SWITCHER_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.hide();
            }
        });
    }

    pub fn snap_active(&mut self, fallback_fg: HWND, dir: SnapDirection) {
        if !self.enabled {
            return;
        }
        // SAFETY: Resolving foreground target window (preferring non-launcher window).
        let fg = unsafe { GetForegroundWindow() };
        let target = if is_manageable_window(fg) {
            fg
        } else if is_manageable_window(fallback_fg) {
            fallback_fg
        } else {
            return;
        };
        let raw = target.0 as isize;
        let current = self.snapped.get(&raw).copied();
        // SAFETY: Querying maximized state on validated target window.
        let is_zoomed = unsafe { IsZoomed(target).as_bool() };
        match next_keyboard_snap(current, is_zoomed, dir) {
            SnapAction::Snap(zone) => {
                apply_snap_zone(target, zone, self.gap_dip);
                self.snapped.insert(raw, zone);
            }
            SnapAction::Restore => {
                self.snapped.remove(&raw);
                // SAFETY: Restoring snapped/maximized window asynchronously.
                unsafe {
                    let _ = ShowWindowAsync(target, SW_RESTORE);
                }
            }
            SnapAction::Minimize => {
                self.snapped.remove(&raw);
                // SAFETY: Minimizing window asynchronously.
                unsafe {
                    let _ = ShowWindowAsync(target, SW_MINIMIZE);
                }
            }
        }
    }

    pub fn toggle_tiling_active_monitor(&mut self) {
        if !self.enabled {
            return;
        }
        let (hmon, _, _, _) = active_monitor_work_rect();
        let default_tiling = self.default_tiling;
        let state = self
            .monitors
            .entry(hmon)
            .or_insert_with(|| MonitorTileState::new(default_tiling));
        state.enabled = !state.enabled;
        let enabled = state.enabled;
        if enabled {
            state.order = list_tileable_windows_on_monitor(hmon);
            self.retile_monitor(hmon);
        }
        SWITCHER_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(st) = slot.as_mut()
                && st.visible
                && st.task_view
            {
                st.tiling_enabled = enabled;
                // SAFETY: Refreshing Task View header tiling pill state.
                unsafe {
                    let _ = InvalidateRect(Some(st.hwnd), None, false);
                }
            }
        });
    }

    pub fn retile_all_enabled(&mut self) {
        if !self.enabled || self.retiling {
            return;
        }
        let mons: Vec<isize> = self
            .monitors
            .iter()
            .filter_map(|(&m, st)| st.enabled.then_some(m))
            .collect();
        for hmon in mons {
            self.retile_monitor(hmon);
        }
    }

    fn retile_monitor(&mut self, hmon: isize) {
        if !self.enabled || self.retiling {
            return;
        }
        let Some(state) = self.monitors.get_mut(&hmon) else {
            return;
        };
        if !state.enabled {
            return;
        }
        let live = list_tileable_windows_on_monitor(hmon);
        state.order.retain(|hwnd| live.contains(hwnd));
        for hwnd in live {
            if !state.order.contains(&hwnd) {
                state.order.push(hwnd);
            }
        }
        if state.order.is_empty() {
            return;
        }

        let sample_hwnd = HWND(state.order[0] as _);
        let (_, _, work, dpi) = monitor_work_rect_for_window(sample_hwnd);
        let gap_px = ((self.gap_dip as f32) * (dpi as f32 / 96.0)).round() as i32;
        let slots = compute_tile_rects(&state.order, work, gap_px, state.master_ratio);
        self.retiling = true;
        for (raw, rect) in slots {
            let hwnd = HWND(raw as _);
            let needs_move = get_visible_window_rect(hwnd).is_none_or(|cur| {
                (cur.left - rect.left).abs() > 2
                    || (cur.top - rect.top).abs() > 2
                    || (cur.width() - rect.width()).abs() > 2
                    || (cur.height() - rect.height()).abs() > 2
            });
            if needs_move {
                set_window_rect_exact(hwnd, rect);
            }
        }
        self.retiling = false;
    }

    pub fn on_foreground_changed(&mut self, hwnd: HWND) {
        if !self.enabled || !self.borders {
            return;
        }
        let next = is_manageable_window(hwnd).then_some(hwnd.0 as isize);
        if self.styled_foreground == next {
            return;
        }
        if let Some(prev) = self.styled_foreground.take() {
            set_window_border_color(HWND(prev as _), None);
        }
        if let Some(raw) = next {
            set_window_border_color(HWND(raw as _), self.border_color);
            self.styled_foreground = Some(raw);
        }
    }

    pub fn on_movesize_event(&mut self, event: u32, hwnd: HWND) {
        if !self.enabled {
            return;
        }
        match event {
            EVENT_SYSTEM_MOVESIZESTART => {
                if !is_manageable_window(hwnd) {
                    return;
                }
                let Some(start_rect) = get_visible_window_rect(hwnd) else {
                    return;
                };
                let (hmon, _, _, _) = monitor_work_rect_for_window(hwnd);
                self.drag = Some(DragSession {
                    hwnd: hwnd.0 as isize,
                    start_rect,
                    preview_snap: None,
                    preview_swap_slot: None,
                    preview_monitor: hmon,
                });
                // SAFETY: Polling drag cursor at 16 ms only while user is dragging a window.
                unsafe {
                    SetTimer(Some(self.launcher_hwnd), WM_DRAG_TIMER, DRAG_POLL_MS, None);
                }
            }
            EVENT_SYSTEM_MOVESIZEEND => {
                self.finish_drag(hwnd);
            }
            _ => {}
        }
    }

    pub fn on_drag_tick(&mut self) {
        let Some(drag) = self.drag.as_mut() else {
            self.stop_drag();
            return;
        };
        let hwnd = HWND(drag.hwnd as _);
        let Some(cur_rect) = get_visible_window_rect(hwnd) else {
            self.stop_drag();
            return;
        };
        let mut cursor = POINT::default();
        // SAFETY: Querying live cursor screen coordinates during active drag.
        if unsafe { GetCursorPos(&mut cursor) }.is_err() {
            return;
        }
        let (hmon, mon_bounds, work, dpi) = cursor_monitor_work_rect();
        drag.preview_monitor = hmon;
        let scale = dpi as f32 / 96.0;
        let gap_px = ((self.gap_dip as f32) * scale).round() as i32;
        let is_move = (cur_rect.width() - drag.start_rect.width()).abs() <= 4
            && (cur_rect.height() - drag.start_rect.height()).abs() <= 4;

        let tiling_on = self.monitors.get(&hmon).map_or(self.default_tiling, |st| st.enabled);

        if tiling_on {
            drag.preview_snap = None;
            if is_move
                && let Some(state) = self.monitors.get(&hmon)
                && state.order.len() > 1
            {
                let slots = compute_tile_rects(&state.order, work, gap_px, state.master_ratio);
                if let Some(slot_idx) = hit_tile_slot(&slots, cursor.x, cursor.y)
                    && slots[slot_idx].0 != drag.hwnd
                {
                    drag.preview_swap_slot = Some(slot_idx);
                    Self::show_overlay(slots[slot_idx].1);
                    return;
                }
            }
            drag.preview_swap_slot = None;
            Self::hide_overlay();
        } else if is_move {
            drag.preview_swap_slot = None;
            let edge_px = (EDGE_THRESHOLD_DIP * scale).round() as i32;
            let corner_px = (CORNER_SPAN_DIP * scale).round() as i32;
            let zone = detect_mouse_snap_zone(cursor.x, cursor.y, mon_bounds, edge_px, corner_px);
            drag.preview_snap = zone;
            if let Some(z) = zone {
                Self::show_overlay(z.rect(work, gap_px));
            } else {
                Self::hide_overlay();
            }
        } else {
            drag.preview_snap = None;
            drag.preview_swap_slot = None;
            Self::hide_overlay();
        }
    }

    fn finish_drag(&mut self, ended_hwnd: HWND) {
        let Some(drag) = self.drag.take() else {
            self.stop_drag();
            return;
        };
        // SAFETY: Stopping temporary drag poll timer immediately on mouse release.
        unsafe {
            let _ = KillTimer(Some(self.launcher_hwnd), WM_DRAG_TIMER);
        }
        Self::hide_overlay();

        if ended_hwnd.0 as isize != drag.hwnd {
            return;
        }
        let hwnd = HWND(drag.hwnd as _);
        let hmon = drag.preview_monitor;
        let tiling_on = self.monitors.get(&hmon).map_or(self.default_tiling, |st| st.enabled);

        if tiling_on {
            let (_, _, work, dpi) = monitor_work_rect_for_window(hwnd);
            let gap_px = ((self.gap_dip as f32) * (dpi as f32 / 96.0)).round() as i32;
            let cur_rect = get_visible_window_rect(hwnd);
            let is_resize = cur_rect.is_some_and(|r| {
                (r.width() - drag.start_rect.width()).abs() > 4 || (r.height() - drag.start_rect.height()).abs() > 4
            });
            let default_tiling = self.default_tiling;
            let state = self
                .monitors
                .entry(hmon)
                .or_insert_with(|| MonitorTileState::new(default_tiling));
            if let Some(slot_idx) = drag.preview_swap_slot {
                if let Some(from_idx) = state.order.iter().position(|&h| h == drag.hwnd) {
                    if slot_idx < state.order.len() {
                        state.order.swap(from_idx, slot_idx);
                    }
                } else {
                    state.order.insert(slot_idx.min(state.order.len()), drag.hwnd);
                }
            } else if is_resize
                && state.order.first().copied() == Some(drag.hwnd)
                && let Some(r) = cur_rect
            {
                state.master_ratio = infer_master_ratio(work, gap_px, r.right);
            }
            self.retile_monitor(hmon);
        } else if let Some(zone) = drag.preview_snap {
            apply_snap_zone(hwnd, zone, self.gap_dip);
            self.snapped.insert(drag.hwnd, zone);
        } else {
            self.snapped.remove(&drag.hwnd);
        }
    }

    fn stop_drag(&mut self) {
        self.drag = None;
        // SAFETY: Ensuring drag poll timer is stopped when idle.
        unsafe {
            let _ = KillTimer(Some(self.launcher_hwnd), WM_DRAG_TIMER);
        }
        Self::hide_overlay();
    }

    fn show_overlay(rect: WmRect) {
        OVERLAY_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.show(rect);
            }
        });
    }

    fn hide_overlay() {
        OVERLAY_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.as_mut()
            {
                state.hide();
            }
        });
    }

    pub fn shutdown(&mut self) {
        self.movesize_hook = None;
        self.stop_drag();
        self.hide_switcher();
        if let Some(prev) = self.styled_foreground.take() {
            set_window_border_color(HWND(prev as _), None);
        }
        reset_all_window_borders();
        SWITCHER_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.take()
            {
                // SAFETY: Destroying switcher overlay HWND on shutdown.
                unsafe {
                    let _ = DestroyWindow(state.hwnd);
                }
            }
        });
        OVERLAY_STATE.with(|cell| {
            if let Ok(mut slot) = cell.try_borrow_mut()
                && let Some(state) = slot.take()
            {
                // SAFETY: Destroying snap preview overlay HWND on shutdown.
                unsafe {
                    let _ = DestroyWindow(state.hwnd);
                }
            }
        });
    }
}
