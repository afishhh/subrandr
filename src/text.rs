use std::{
    cell::{OnceCell, UnsafeCell},
    collections::HashSet,
    fmt::{Debug, Display},
};

use rasterize::{color::BGRA8, Rasterizer, RenderTarget, Texture};
use text_sys::*;
use util::{
    math::{I26Dot6, Vec2},
    ReadonlyAliasableBox,
};

mod face;
mod ft_utils;
pub use face::*;
pub use ft_utils::FreeTypeError;
pub mod font_db;
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
pub struct Glyph<'f> {
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

#[derive(Debug, Clone, Copy)]
pub struct TextMetrics {
    pub paint_size: Vec2<I26Dot6>,
    pub trailing_advance: I26Dot6,
    pub max_bearing_y: I26Dot6,
    pub max_ascender: I26Dot6,
    pub min_descender: I26Dot6,
    pub max_lineskip_descent: I26Dot6,
}

impl TextMetrics {
    pub fn extend_by_font(&mut self, font: &Font) {
        let metrics = font.metrics();
        self.max_ascender = self.max_ascender.max(metrics.ascender);
        self.min_descender = self.min_descender.min(metrics.descender);
        self.max_lineskip_descent = self
            .max_lineskip_descent
            .max(metrics.height - metrics.ascender);
    }
}

pub fn compute_extents_ex<'a, 'f: 'a, I>(
    cache: &GlyphCache,
    horizontal: bool,
    glyphs: I,
) -> Result<TextMetrics, FreeTypeError>
where
    I: IntoIterator + 'a,
    I::IntoIter: DoubleEndedIterator<Item = &'a Glyph<'f>>,
{
    let mut results = TextMetrics {
        paint_size: Vec2::ZERO,
        trailing_advance: I26Dot6::ZERO,
        max_bearing_y: I26Dot6::ZERO,
        max_ascender: I26Dot6::ZERO,
        min_descender: I26Dot6::ZERO,
        max_lineskip_descent: I26Dot6::ZERO,
    };

    let mut glyphs = glyphs.into_iter();

    if let Some(glyph) = glyphs.next_back() {
        let extents = glyph.font.glyph_extents(cache, glyph.index)?;
        results.paint_size.y += extents.height.abs();
        results.paint_size.x += extents.width;
        if horizontal {
            results.trailing_advance = glyph.x_advance - extents.width;
            results.max_bearing_y = results.max_bearing_y.max(extents.hori_bearing_y);
        } else {
            results.trailing_advance = glyph.y_advance - extents.height;
            results.max_bearing_y = results.max_bearing_y.max(extents.vert_bearing_y);
        }
        results.extend_by_font(glyph.font);
    }

    for glyph in glyphs {
        let extents = glyph.font.glyph_extents(cache, glyph.index)?;
        if horizontal {
            results.paint_size.y = results.paint_size.y.max(extents.height.abs());
            results.paint_size.x += glyph.x_advance;
            results.max_bearing_y = results.max_bearing_y.max(extents.hori_bearing_y);
        } else {
            results.paint_size.x = results.paint_size.x.max(extents.width.abs());
            results.paint_size.y += glyph.y_advance;
            results.max_bearing_y = results.max_bearing_y.max(extents.vert_bearing_y);
        }
        results.extend_by_font(glyph.font);
    }

    Ok(results)
}

struct GlyphBitmap {
    offset: (i32, i32),
    texture: Texture,
}

/// Merged monochrome bitmap of the whole text string, useful for shadows.
pub struct MonochromeImage {
    pub offset: Vec2<i32>,
    pub texture: Texture,
}

impl MonochromeImage {
    // TODO: Remove the need for this via a blit with monochrome filter operation
    //       or at least use a per-glyph monochrome cache.
    //       Non-monochrome glyphs are rare so maybe the second option will be just fine.
    pub fn from_image(rasterizer: &mut dyn Rasterizer, image: &Image) -> Self {
        let mut offset = Vec2::<i32>::ZERO;
        let (mut width, mut height) = (0, 0);
        for glyph in &image.glyphs {
            offset.x = offset.x.min(glyph.offset.0);
            offset.y = offset.y.min(glyph.offset.1);

            width = width.max(glyph.offset.0.max(0) as u32 + glyph.texture.width());
            height = height.max(glyph.offset.1.max(0) as u32 + glyph.texture.height());
        }

        width += (-offset.x).max(0) as u32;
        height += (-offset.y).max(0) as u32;

        let mut target = rasterizer.create_mono_texture_rendered(width, height);

        for glyph in &image.glyphs {
            unsafe {
                rasterizer.blit_to_mono_texture_unchecked(
                    &mut target,
                    glyph.offset.0 - offset.x,
                    glyph.offset.1 - offset.y,
                    &glyph.texture,
                );
            }
        }

        MonochromeImage {
            offset,
            texture: rasterizer.finalize_texture_render(target),
        }
    }

    pub fn blit(
        &self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget<'_>,
        dx: i32,
        dy: i32,
        color: BGRA8,
    ) {
        rasterizer.blit(target, dx, dy, &self.texture, color);
    }
}

pub struct Image {
    glyphs: Vec<GlyphBitmap>,
    monochrome: OnceCell<MonochromeImage>,
}

impl GlyphBitmap {
    fn blit(
        &self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        color: BGRA8,
    ) {
        rasterizer.blit(
            target,
            dx + self.offset.0,
            dy + self.offset.1,
            &self.texture,
            color,
        );
    }
}

impl Image {
    pub fn monochrome(&self, rasterizer: &mut dyn Rasterizer) -> &MonochromeImage {
        self.monochrome
            .get_or_init(|| MonochromeImage::from_image(rasterizer, self))
    }

    pub fn blit(
        &self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        color: BGRA8,
    ) {
        for glyph in &self.glyphs {
            glyph.blit(rasterizer, target, dx, dy, color);
        }
    }
}

pub fn render<'g, 'f: 'g>(
    cache: &GlyphCache,
    rasterizer: &mut dyn Rasterizer,
    xf: I26Dot6,
    yf: I26Dot6,
    blur_sigma: f32,
    glyphs: &mut dyn Iterator<Item = &'g Glyph<'f>>,
) -> Result<Image, GlyphRenderError> {
    let mut result = Image {
        glyphs: Vec::new(),
        monochrome: OnceCell::new(),
    };

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

        result.glyphs.push(GlyphBitmap {
            offset: (
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
