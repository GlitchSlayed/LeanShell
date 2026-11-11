//! Direct2D software renderer for LeanShell.
//!
//! Reference implementation of `leanshell_render::Renderer` targeting Win32 HWNDs
//! using Direct2D and DirectWrite.

use leanshell_render::{Color, Frame, Renderer};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
use windows::core::Result;

pub fn to_d2d_color(c: Color) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: c.r,
        g: c.g,
        b: c.b,
        a: c.a,
    }
}

pub struct D2DRenderer {
    dpi: f32,
    factory: Option<ID2D1Factory>,
    rt: Option<ID2D1HwndRenderTarget>,
}

impl Default for D2DRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl D2DRenderer {
    pub fn new() -> Self {
        Self {
            dpi: 96.0,
            factory: None,
            rt: None,
        }
    }

    fn get_or_create_rt(&mut self, hwnd: HWND) -> Result<&ID2D1HwndRenderTarget> {
        if self.factory.is_none() {
            // SAFETY: Creating single-threaded Direct2D factory
            let factory: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
            self.factory = Some(factory);
        }
        if self.rt.is_none() {
            let mut rc = Default::default();
            // SAFETY: Querying client dimensions for target window
            unsafe {
                let _ = GetClientRect(hwnd, &mut rc);
            }
            let width = (rc.right - rc.left).max(1) as u32;
            let height = (rc.bottom - rc.top).max(1) as u32;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: self.dpi,
                dpiY: self.dpi,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd,
                pixelSize: D2D_SIZE_U { width, height },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let factory = self.factory.as_ref().unwrap();
            // SAFETY: Instantiating Direct2D HWND render target
            let rt = unsafe { factory.CreateHwndRenderTarget(&props, &hwnd_props)? };
            self.rt = Some(rt);
        }
        Ok(self.rt.as_ref().unwrap())
    }
}

impl Renderer for D2DRenderer {
    type Error = windows::core::Error;

    fn draw(&mut self, hwnd: isize, frame: &Frame<'_>) -> std::result::Result<(), Self::Error> {
        let hwnd = HWND(hwnd as _);
        let rt = self.get_or_create_rt(hwnd)?;
        // SAFETY: Rendering frame buffer with Direct2D
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&to_d2d_color(frame.palette.background)));
            rt.EndDraw(None, None)?;
        }
        Ok(())
    }

    fn resize(&mut self, hwnd: isize, dpi: f32) -> std::result::Result<(), Self::Error> {
        self.dpi = dpi;
        let hwnd = HWND(hwnd as _);
        let mut rc = Default::default();
        // SAFETY: Querying client area for render target resize
        unsafe {
            let _ = GetClientRect(hwnd, &mut rc);
        }
        let width = (rc.right - rc.left).max(1) as u32;
        let height = (rc.bottom - rc.top).max(1) as u32;
        if let Some(rt) = &self.rt {
            // SAFETY: Updating DPI and resizing target buffer
            unsafe {
                rt.SetDpi(dpi, dpi);
                let _ = rt.Resize(&D2D_SIZE_U { width, height });
            }
        }
        Ok(())
    }
}
