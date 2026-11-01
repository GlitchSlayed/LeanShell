//! The dock's side of the notification area: which icons show where, and
//! their pixels.

use std::collections::HashMap;
use std::sync::Arc;

use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DrawIconEx, HICON};

use super::logic;
use crate::indexer::IconPixels;
use crate::tray_host::{Icon, Key};

/// An icon the dock shows, with its pixels at the dock's DPI.
pub struct Shown {
    pub icon: Icon,
    pub pixels: Option<Arc<IconPixels>>,
}

/// What a tooltip belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tip {
    Chevron,
    /// An icon in the dock, by its inline position.
    Inline(usize),
    /// An icon in the flyout, by its index in `shown`.
    Flyout(usize),
}

/// Explorer's own icons without a callback, such as its volume icon, belong
/// to the system indicators the dock already shows.
fn wanted(icon: &Icon) -> bool {
    icon.visible() && !(icon.callback == 0 && icon.exes.iter().any(|exe| exe.ends_with(r"\explorer.exe")))
}

/// Picks the shown icons and the promoted ones for the dock, reusing pixels
/// for icons that didn't change.
pub fn update(old: Vec<Shown>, icons: Vec<Icon>, size: i32) -> (Vec<Shown>, Vec<usize>) {
    let mut cache: HashMap<Key, (u64, Arc<IconPixels>)> = old
        .into_iter()
        .filter_map(|shown| Some((shown.icon.key(), (shown.icon.revision, shown.pixels?))))
        .collect();
    let shown: Vec<Shown> = icons
        .into_iter()
        .filter(wanted)
        .map(|icon| {
            let cached = cache
                .remove(&icon.key())
                .filter(|(revision, pixels)| *revision == icon.revision && pixels.width == size as u32);
            let pixels = cached.map(|(_, pixels)| pixels).or_else(|| {
                let image = icon.image.as_ref()?;
                icon_pixels(image.0, size).map(Arc::new)
            });
            Shown { icon, pixels }
        })
        .collect();
    let inline = shown
        .iter()
        .enumerate()
        .filter(|(_, shown)| shown.icon.promoted == Some(true) && shown.pixels.is_some())
        .map(|(index, _)| index)
        .take(logic::MAX_INLINE)
        .collect();
    (shown, inline)
}

/// The physical pixel size of a tray icon at `dpi`.
pub fn icon_size(dpi: u32) -> i32 {
    (logic::TRAY_ICON * dpi as f32 / 96.0).round() as i32
}

/// Renders an icon onto black and onto white: their difference is its
/// coverage, so alpha icons and old masked icons both come out right.
/// The result is premultiplied BGRA.
pub fn icon_pixels(icon: HICON, size: i32) -> Option<IconPixels> {
    let black = draw_on(icon, size, 0x00)?;
    let white = draw_on(icon, size, 0xFF)?;
    let mut bgra = vec![0u8; black.len()];
    let (out, _) = bgra.as_chunks_mut::<4>();
    let (black, _) = black.as_chunks::<4>();
    let (white, _) = white.as_chunks::<4>();
    for ((out, b), w) in out.iter_mut().zip(black).zip(white) {
        // On black a pixel is its premultiplied color; white adds 1 - alpha.
        let alpha = 255 - (0..3).map(|c| w[c].saturating_sub(b[c])).max().unwrap_or(0);
        for c in 0..3 {
            out[c] = b[c].min(alpha);
        }
        out[3] = alpha;
    }
    if out.iter().all(|p| p[3] == 0) {
        return None;
    }
    Some(IconPixels {
        width: size as u32,
        height: size as u32,
        bgra,
    })
}

/// The icon drawn at `size` pixels over a gray `level` background, as BGRA rows.
fn draw_on(icon: HICON, size: i32, level: u8) -> Option<Vec<u8>> {
    unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size, // Top-down rows.
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let dc = CreateCompatibleDC(None);
        if dc.is_invalid() {
            return None;
        }
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(dc);
            return None;
        };
        let previous = SelectObject(dc, bitmap.into());
        let len = (size * size * 4) as usize;
        std::slice::from_raw_parts_mut(bits as *mut u8, len).fill(level);
        let drawn = DrawIconEx(dc, 0, 0, icon, size, size, 0, None, DI_NORMAL).is_ok();
        let _ = GdiFlush();
        let pixels = drawn.then(|| std::slice::from_raw_parts(bits as *const u8, len).to_vec());
        SelectObject(dc, previous);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
        pixels
    }
}

/// Loads shell artwork for the exact physical size used by the dock.
pub fn app_pixels(path: &str, size: i32) -> Option<IconPixels> {
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName};
    use windows::core::PCWSTR;
    let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(PCWSTR(wide.as_ptr()), None).ok()?;
        let source = crate::indexer::load_icon(&item, size.max(64))?;
        let mut bgra = vec![0; (size * size * 4) as usize];
        for y in 0..size as usize {
            for x in 0..size as usize {
                let sx = ((x as f32 + 0.5) * source.width as f32 / size as f32 - 0.5).max(0.0);
                let sy = ((y as f32 + 0.5) * source.height as f32 / size as f32 - 0.5).max(0.0);
                let (ix, iy) = (sx as u32, sy as u32);
                for c in 0..4 {
                    let at = |x: u32, y: u32| {
                        source.bgra
                            [((y.min(source.height - 1) * source.width + x.min(source.width - 1)) * 4) as usize + c]
                            as f32
                    };
                    let top = at(ix, iy) * (1.0 - sx.fract()) + at(ix + 1, iy) * sx.fract();
                    let bottom = at(ix, iy + 1) * (1.0 - sx.fract()) + at(ix + 1, iy + 1) * sx.fract();
                    bgra[(y * size as usize + x) * 4 + c] =
                        (top * (1.0 - sy.fract()) + bottom * sy.fract()).round() as u8;
                }
            }
        }
        Some(IconPixels {
            width: size as u32,
            height: size as u32,
            bgra,
        })
    }
}
