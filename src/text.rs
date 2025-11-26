use std::{
    cell::UnsafeCell,
    collections::HashSet,
    fmt::{Debug, Display},
};

use rasterize::{Rasterizer, Texture};
use text_sys::*;
use util::{
    math::{I26Dot6, Vec2},
    ReadonlyAliasableBox,
};

mod face;
mod ft_utils;
pub use face::*;
pub use ft_utils::FreeTypeError;
mod font_db;
pub use font_db::*;
mod font_match;
pub use font_match::*;
mod glyph_cache;
pub use glyph_cache::*;
pub mod platform_font_provider;
mod shape;
pub use shape::*;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct OpenTypeTag(u32);

impl OpenTypeTag {
    pub const fn from_bytes(text: [u8; 4]) -> Self {
        Self(
            ((text[0] as u32) << 24)
                + ((text[1] as u32) << 16)
                + ((text[2] as u32) << 8)
                + (text[3] as u32),
        )
    }

    pub const fn to_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub fn to_bytes_in(self, buf: &mut [u8; 4]) -> &[u8] {
        *buf = self.to_bytes();
        let offset = buf.iter().position(|b| *b != b'0').unwrap_or(buf.len());
        &buf[offset..]
    }

    pub const AXIS_WEIGHT: OpenTypeTag = OpenTypeTag::from_bytes(*b"wght");
    #[expect(dead_code)]
    pub const AXIS_WIDTH: OpenTypeTag = OpenTypeTag::from_bytes(*b"wdth");
    pub const AXIS_ITALIC: OpenTypeTag = OpenTypeTag::from_bytes(*b"ital");

    pub const FEAT_RUBY: OpenTypeTag = OpenTypeTag::from_bytes(*b"ruby");
}

impl Display for OpenTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0; 4];
        let bytes = self.to_bytes_in(&mut buf);
        if let Ok(string) = std::str::from_utf8(bytes) {
            write!(f, "{string}")
        } else {
            write!(f, "{}", bytes.escape_ascii())
        }
    }
}

impl Debug for OpenTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0; 4];
        let bytes = self.to_bytes_in(&mut buf);
        if let Ok(string) = std::str::from_utf8(bytes) {
            write!(f, "{string:?}")
        } else {
            write!(f, "{bytes:?}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ltr = HB_DIRECTION_LTR as isize,
    Rtl = HB_DIRECTION_RTL as isize,
    Ttb = HB_DIRECTION_TTB as isize,
    Btt = HB_DIRECTION_BTT as isize,
}

impl Direction {
    fn try_from_hb(value: hb_direction_t) -> Option<Self> {
        Some(match value {
            HB_DIRECTION_LTR => Self::Ltr,
            HB_DIRECTION_RTL => Self::Rtl,
            HB_DIRECTION_TTB => Self::Ttb,
            HB_DIRECTION_BTT => Self::Btt,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn is_reverse(self) -> bool {
        matches!(self, Self::Rtl | Self::Btt)
    }

    #[must_use]
    pub const fn to_horizontal(self) -> Self {
        match self {
            Self::Ltr => Self::Ltr,
            Self::Rtl => Self::Rtl,
            Self::Ttb => Self::Ltr,
            Self::Btt => Self::Rtl,
        }
    }
}

// This arena holds fonts and allows `Glyph` to safely store references
// to its fonts instead of having to do reference counting.
#[derive(Debug)]
pub struct FontArena {
    fonts: UnsafeCell<HashSet<ReadonlyAliasableBox<Font>>>,
}

impl FontArena {
    pub fn new() -> Self {
        Self {
            fonts: UnsafeCell::new(HashSet::new()),
        }
    }

    pub fn insert<'f>(&'f self, font: &Font) -> &'f Font {
        let fonts = unsafe { &mut *self.fonts.get() };
        if fonts.contains(font) {
            unsafe { fonts.get(font).unwrap_unchecked() }
        } else {
            let boxed = ReadonlyAliasableBox::new(font.clone());
            let ptr = ReadonlyAliasableBox::as_nonnull(&boxed);
            unsafe {
                fonts.insert(boxed);
                ptr.as_ref()
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
// FIXME: If this isn't `(crate)` pub then a bunch of stuff is considered public, why?
pub(crate) struct Glyph<'f> {
    pub index: hb_codepoint_t,
    /// Position of the directionally-first byte where this glyph starts in the original UTF-8 string.
    /// If left-to-right, this will be the first byte of the relevant codepoint.
    /// If right-to-left, this will be the last byte of the relevant codepoint.
    pub cluster: usize,
    pub x_advance: I26Dot6,
    pub y_advance: I26Dot6,
    pub x_offset: I26Dot6,
    pub y_offset: I26Dot6,
    pub font: &'f Font,
    flags: hb_glyph_flags_t,
}

impl<'f> Glyph<'f> {
    fn from_info_and_position(
        info: &hb_glyph_info_t,
        position: &hb_glyph_position_t,
        original_cluster: usize,
        font: &'f Font,
    ) -> Self {
        // Fix up incorrect metrics for scaled bitmap glyphs which HarfBuzz sees as unscaled.
        let scale = font.harfbuzz_scale_factor_for(info.codepoint);

        Self {
            index: info.codepoint,
            cluster: original_cluster,
            x_advance: I26Dot6::from_raw(position.x_advance) * scale,
            y_advance: I26Dot6::from_raw(position.y_advance) * scale,
            x_offset: I26Dot6::from_raw(position.x_offset) * scale,
            y_offset: I26Dot6::from_raw(position.y_offset) * scale,
            font,
            flags: unsafe { hb_glyph_info_get_glyph_flags(info) },
        }
    }

    pub fn unsafe_to_break(&self) -> bool {
        (self.flags & HB_GLYPH_FLAG_UNSAFE_TO_BREAK) != 0
    }

    pub fn unsafe_to_concat(&self) -> bool {
        (self.flags & HB_GLYPH_FLAG_UNSAFE_TO_CONCAT) != 0
    }
}

pub struct GlyphBitmap {
    pub offset: Vec2<i32>,
    pub texture: Texture,
}

pub fn render<'g, 'f: 'g>(
    cache: &GlyphCache,
    rasterizer: &mut dyn Rasterizer,
    xf: I26Dot6,
    yf: I26Dot6,
    blur_sigma: f32,
    glyphs: &mut dyn Iterator<Item = &'g Glyph<'f>>,
) -> Result<Vec<GlyphBitmap>, GlyphRenderError> {
    let mut result = Vec::new();

    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&xf));
    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&yf));

    let mut x = xf;
    let mut y = yf;
    for shaped_glyph in glyphs {
        // TODO: Once vertical text is supported, this should change depending on main axis
        let subpixel_offset = if x < 0 {
            I26Dot6::ONE - x.abs().fract()
        } else {
            x.fract()
        };

        let font = shaped_glyph.font;
        let bitmap = font.render_glyph(
            cache,
            rasterizer,
            shaped_glyph.index,
            blur_sigma,
            subpixel_offset,
            false,
        )?;

        result.push(GlyphBitmap {
            offset: Vec2::new(
                (x + bitmap.offset.x + shaped_glyph.x_offset).floor_to_inner(),
                (y + bitmap.offset.y + shaped_glyph.y_offset).floor_to_inner(),
            ),
            texture: bitmap.texture.clone(),
        });

        x += shaped_glyph.x_advance;
        y += shaped_glyph.y_advance;
    }

    Ok(result)
}
