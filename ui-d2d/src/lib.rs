//! Direct2D software renderer for LeanShell.
//!
//! Reference implementation of `leanshell_render::Renderer` targeting Win32 HWNDs
//! using Direct2D and DirectWrite.

use leanshell_render::{Color, Frame, Renderer};
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

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
}

impl Default for D2DRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl D2DRenderer {
    pub fn new() -> Self {
        Self { dpi: 96.0 }
    }
}

impl Renderer for D2DRenderer {
    type Error = windows::core::Error;

    fn draw(&mut self, _hwnd: isize, _frame: &Frame<'_>) -> Result<(), Self::Error> {
        // Frame rendering routine
        Ok(())
    }

    fn resize(&mut self, _hwnd: isize, dpi: f32) -> Result<(), Self::Error> {
        self.dpi = dpi;
        Ok(())
    }
}
