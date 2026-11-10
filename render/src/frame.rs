//! Frame descriptions submitted to a `Renderer`.

use std::collections::HashMap;
use std::sync::Arc;

use leanshell_core::model::IconPixels;
use leanshell_core::search::ResultItem;

use crate::color::Palette;
use crate::layout::Rect;

/// Description of the launcher scene to draw in a single frame.
pub struct Frame<'a> {
    pub palette: &'a Palette,
    pub text: &'a str,
    /// UTF-16 offsets into `text`.
    pub caret: u32,
    pub anchor: u32,
    pub caret_visible: bool,
    pub placeholder: &'a str,
    /// The open list's name, shown before the query, as in "Wi-Fi ›".
    pub chip: Option<&'a str>,
    pub results: &'a [ResultItem],
    pub selected: usize,
    pub scroll: usize,
    pub rows: usize,
    pub icons: &'a HashMap<String, Arc<IconPixels>>,
    /// Drawn instead of the results while a surface is open.
    pub surface: Option<SurfaceFrame<'a>>,
    /// The query box is at the bottom and results stack upward.
    pub bottom: bool,
    pub opacity: f32,
    pub dock_opacity: f32,
    pub shared: bool,
    pub radius: f32,
    pub content_size: Option<(f32, f32)>,
}

/// Description of an active surface embedded in the launcher.
pub struct SurfaceFrame<'a> {
    pub widgets: &'a [SurfaceWidgetDescription],
    pub rects: &'a [Rect],
    pub focus: usize,
    pub stop: usize,
    pub scroll: f32,
    pub rows: usize,
}

#[derive(Debug, Clone)]
pub struct SurfaceWidgetDescription {
    pub label: String,
    pub value: Option<String>,
    pub glyph: Option<&'static str>,
}
