//! Core rendering trait and frame description types for LeanShell.
//!
//! Provides layout geometry, color palettes, and frame description
//! structures consumed by the native Direct2D (`ui-d2d`) renderer.

pub mod color;
pub mod frame;
pub mod layout;

pub use color::{BRAND, Color, Palette};
pub use frame::{Frame, SurfaceFrame, SurfaceWidgetDescription};
pub use layout::Rect;

/// Abstraction for native Direct2D frame rendering.
pub trait Renderer {
    type Error: std::fmt::Debug;

    /// Renders the launcher frame into the designated window target.
    fn draw(&mut self, hwnd: isize, frame: &Frame<'_>) -> Result<(), Self::Error>;

    /// Updates dpi scaling and target buffer dimensions.
    fn resize(&mut self, hwnd: isize, dpi: f32) -> Result<(), Self::Error>;
}
