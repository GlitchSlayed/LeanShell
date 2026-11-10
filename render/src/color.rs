//! Agnostic color and palette representations for LeanShell rendering backends.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(hex: u32, a: f32) -> Self {
        Self {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a,
        }
    }
}

/// Brand accent color.
pub const BRAND: u32 = 0x0891B2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color,
    pub text: Color,
    pub subtext: Color,
    pub placeholder: Color,
    pub row_selected: Color,
    pub divider: Color,
    pub accent: Color,
    pub text_selection: Color,
    pub tile: Color,
    pub scrollbar: Color,
    pub background_solid: Color,
}

impl Palette {
    pub fn new(dark: bool, translucent: bool) -> Self {
        if dark {
            Self {
                background: if translucent {
                    Color::from_hex(0x1C1C1C, 0.3)
                } else {
                    Color::from_hex(0x202020, 1.0)
                },
                text: Color::from_hex(0xF5F5F5, 1.0),
                subtext: Color::from_hex(0xFFFFFF, 0.58),
                placeholder: Color::from_hex(0xFFFFFF, 0.42),
                row_selected: Color::from_hex(0xFFFFFF, 0.08),
                divider: Color::from_hex(0xFFFFFF, 0.08),
                accent: Color::from_hex(0x22D3EE, 1.0),
                text_selection: Color::from_hex(BRAND, 0.6),
                tile: Color::from_hex(0xFFFFFF, 0.07),
                scrollbar: Color::from_hex(0xFFFFFF, 0.22),
                background_solid: Color::from_hex(0x1C1C1C, 1.0),
            }
        } else {
            Self {
                background: if translucent {
                    Color::from_hex(0xF3F3F3, 0.4)
                } else {
                    Color::from_hex(0xF9F9F9, 1.0)
                },
                text: Color::from_hex(0x1B1B1B, 1.0),
                subtext: Color::from_hex(0x000000, 0.58),
                placeholder: Color::from_hex(0x000000, 0.42),
                row_selected: Color::from_hex(0x000000, 0.06),
                divider: Color::from_hex(0x000000, 0.08),
                accent: Color::from_hex(BRAND, 1.0),
                text_selection: Color::from_hex(BRAND, 0.35),
                tile: Color::from_hex(0x000000, 0.05),
                scrollbar: Color::from_hex(0x000000, 0.22),
                background_solid: Color::from_hex(0xF3F3F3, 1.0),
            }
        }
    }
}
