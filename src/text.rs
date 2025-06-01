use std::{
    cell::{OnceCell, UnsafeCell},
    collections::HashSet,
    mem::MaybeUninit,
    ops::{Range, RangeFrom, RangeFull},
};

use text_sys::*;
use thiserror::Error;

mod face;
mod ft_utils;
pub use face::*;
pub use ft_utils::FreeTypeError;
pub mod font_db;
pub use font_db::*;
mod glyphstring;
pub use glyphstring::*;
mod font_match;
pub use font_match::*;
pub mod layout;

use crate::{
    color::BGRA8,
    math::{I26Dot6, Vec2},
    rasterize::{Rasterizer, RenderTarget, Texture},
    util::ReadonlyAliasableBox,
};

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

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Ltr | Self::Rtl)
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
    /// Byte position where this glyph started in the original UTF-8 string
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
        Self {
            index: info.codepoint,
            cluster: original_cluster,
            x_advance: I26Dot6::from_raw(position.x_advance),
            y_advance: I26Dot6::from_raw(position.y_advance),
            x_offset: I26Dot6::from_raw(position.x_offset),
            y_offset: I26Dot6::from_raw(position.y_offset),
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
        let extents = glyph.font.glyph_extents(glyph.index)?;
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
        let extents = glyph.font.glyph_extents(glyph.index)?;
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

impl AsRef<Self> for Font {
    fn as_ref(&self) -> &Self {
        self
    }
}

mod sealed {
    use std::ops::{Range, RangeFrom, RangeFull};

    pub trait Sealed {}
    impl Sealed for Range<usize> {}
    impl Sealed for RangeFrom<usize> {}
    impl Sealed for RangeFull {}
}

pub trait ItemRange: sealed::Sealed {
    fn bounds_check(&self, length: usize);
    fn start(&self) -> u32;
    fn length(&self) -> i32;
}

impl ItemRange for Range<usize> {
    fn bounds_check(&self, length: usize) {
        assert!(self.start <= self.end);
        assert!(self.end <= length);
    }

    fn start(&self) -> u32 {
        self.start as u32
    }

    fn length(&self) -> i32 {
        (self.end - self.start) as i32
    }
}

impl ItemRange for RangeFrom<usize> {
    fn bounds_check(&self, length: usize) {
        assert!(self.start <= length);
    }

    fn start(&self) -> u32 {
        self.start as u32
    }

    fn length(&self) -> i32 {
        -1
    }
}

impl ItemRange for RangeFull {
    fn bounds_check(&self, _length: usize) {}

    fn start(&self) -> u32 {
        0
    }

    fn length(&self) -> i32 {
        -1
    }
}

// Right to left text will have clusters monotonically decreasing instead of increasing,
// so we need to fixup cluster ranges so we don't crash when slicing with them.
fn fixup_range(a: usize, b: usize) -> Range<usize> {
    if a > b {
        // fixup_range accepts an *exclusive* range where b is excluded and a is included
        // *regardless* of which is higher, therefore when reversing it we have to make
        // sure we're taking this into account.
        b + 1..a + 1
    } else {
        a..b
    }
}

pub struct ShapingBuffer {
    buffer: *mut hb_buffer_t,
}

#[derive(Debug, Error)]
pub enum ShapingError {
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
    #[error("font selection: {0}")]
    FontSelect(#[from] font_db::SelectError),
}

impl ShapingBuffer {
    pub fn new() -> Self {
        Self {
            buffer: unsafe {
                let buffer = hb_buffer_create();
                if hb_buffer_allocation_successful(buffer) == 0 {
                    panic!("failed to allocate a harfbuzz buffer")
                }
                buffer
            },
        }
    }

    pub fn add(&mut self, text: &str, range: impl ItemRange) {
        range.bounds_check(text.len());

        unsafe {
            hb_buffer_add_utf8(
                self.buffer,
                text.as_ptr() as *const _,
                text.len() as i32,
                range.start(),
                range.length(),
            );
        }
    }

    pub fn direction(&self) -> Option<Direction> {
        unsafe { Direction::try_from_hb(hb_buffer_get_direction(self.buffer)) }
    }

    pub fn set_direction(&mut self, direction: Direction) {
        unsafe {
            hb_buffer_set_direction(self.buffer, direction as hb_direction_t);
        }
    }

    pub fn guess_properties(&mut self) -> Direction {
        unsafe {
            hb_buffer_guess_segment_properties(self.buffer);
        }
        self.direction().unwrap()
    }

    fn glyphs(&mut self) -> (&mut [hb_glyph_info_t], &mut [hb_glyph_position_t]) {
        let infos: &mut [hb_glyph_info_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(self.buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        let positions: &mut [hb_glyph_position_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(self.buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        assert_eq!(infos.len(), positions.len());

        (infos, positions)
    }

    // TODO: Make this operate directly on UTF-8 cluster values and reshape only on grapheme boundaries.
    fn shape_rec<'f>(
        &mut self,
        result: &mut Vec<Glyph<'f>>,
        font_arena: &'f FontArena,
        codepoints: &[(u32, u32)],
        start: usize,
        properties: &hb_segment_properties_t,
        mut font_iterator: FontMatchIterator<'_, 'f>,
        force_tofu: bool,
        fonts: &mut FontDb,
    ) -> Result<(), ShapingError> {
        let Some(&(first_codepoint, _)) = codepoints.get(start) else {
            return Ok(());
        };

        let font = if force_tofu {
            font_iterator.matcher().tofu(font_arena)
        } else {
            font_iterator
                .next_with_fallback(first_codepoint, font_arena, fonts)?
                .unwrap_or_else(|| font_iterator.matcher().tofu(font_arena))
        };
        let hb_font = font.as_harfbuzz_font()?;

        unsafe {
            hb_shape(hb_font, self.buffer, std::ptr::null(), 0);

            let (infos, positions) = self.glyphs();

            result.reserve(infos.len());
            let mut invalid_range_start = None;

            let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t| {
                Glyph::from_info_and_position(
                    info,
                    position,
                    codepoints[info.cluster as usize].1 as usize,
                    font,
                )
            };
            let mut retry_shaping = |range: Range<usize>,
                                     result: &mut Vec<Glyph<'f>>,
                                     font_arena: &'f FontArena,
                                     force_tofu: bool|
             -> Result<(), ShapingError> {
                let mut sub_buffer = Self::new();
                for ((codepoint, _), i) in
                    codepoints[range.clone()].iter().copied().zip(range.clone())
                {
                    hb_buffer_add(sub_buffer.buffer, codepoint, i as u32);
                }
                hb_buffer_set_segment_properties(sub_buffer.buffer, properties);
                hb_buffer_set_content_type(sub_buffer.buffer, HB_BUFFER_CONTENT_TYPE_UNICODE);

                sub_buffer.shape_rec(
                    result,
                    font_arena,
                    codepoints,
                    range.start,
                    properties,
                    font_iterator.clone(),
                    force_tofu,
                    fonts,
                )?;

                Ok(())
            };

            for (i, (info, position)) in infos.iter().zip(positions.iter()).enumerate() {
                if info.codepoint == 0 {
                    if invalid_range_start.is_none() {
                        invalid_range_start = Some(i)
                    }
                    continue;
                } else if let Some(start) = invalid_range_start.take() {
                    retry_shaping(
                        fixup_range(infos[start].cluster as usize, info.cluster as usize),
                        result,
                        font_arena,
                        force_tofu,
                    )?;
                }

                result.push(make_glyph(info, position));
            }

            if let Some(start) = invalid_range_start {
                // This means the font fallback system lied to us and gave us
                // a font that does not, in fact, have the character we asked for.
                // Or the tofu font failed to shape any characters but that shouldn't
                // happen, if it does anyway it will just incur an additional shaping pass.
                let next_force_tofu = start == 0 && font_iterator.did_system_fallback();

                retry_shaping(
                    fixup_range(
                        infos[start].cluster as usize,
                        // FIXME: Is this correct for RTL text?
                        infos.last().unwrap().cluster as usize + 1,
                    ),
                    result,
                    font_arena,
                    next_force_tofu,
                )?
            }

            Ok(())
        }
    }

    pub fn shape<'f>(
        &mut self,
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Vec<Glyph<'f>>, ShapingError> {
        let codepoints: Vec<_> = self
            .glyphs()
            .0
            .iter_mut()
            .enumerate()
            // TODO: What if a surrogate code point appears here?
            .map(|(i, x)| {
                let original_cluster = x.cluster;
                x.cluster = i as u32;
                (x.codepoint, original_cluster)
            })
            .collect();

        let properties = unsafe {
            let mut buf = MaybeUninit::uninit();
            hb_buffer_guess_segment_properties(self.buffer);
            hb_buffer_get_segment_properties(self.buffer, buf.as_mut_ptr());
            buf.assume_init()
        };

        unsafe {
            hb_buffer_set_flags(self.buffer, HB_BUFFER_FLAG_PRODUCE_UNSAFE_TO_CONCAT);
        }

        let mut result = Vec::new();
        self.shape_rec(
            &mut result,
            font_arena,
            &codepoints,
            0,
            &properties,
            font_iterator,
            false,
            fonts,
        )?;

        Ok(result)
    }

    pub fn clear(&mut self) {
        unsafe {
            hb_buffer_clear_contents(self.buffer);
        }
    }

    pub fn reset(&mut self) {
        unsafe {
            hb_buffer_reset(self.buffer);
        }
    }
}

impl Drop for ShapingBuffer {
    fn drop(&mut self) {
        unsafe {
            hb_buffer_destroy(self.buffer);
        }
    }
}

pub fn simple_shape_text<'f>(
    font_iterator: FontMatchIterator<'_, 'f>,
    font_arena: &'f FontArena,
    text: &str,
    fonts: &mut FontDb,
) -> Result<Vec<Glyph<'f>>, ShapingError> {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text, ..);
    buffer.shape(font_iterator, font_arena, fonts)
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

    pub fn prepare_for_blur(&self, rasterizer: &mut dyn Rasterizer, sigma: f32) -> Vec2<i32> {
        let mut offset = Vec2::new(0, 0);
        let (mut width, mut height) = (0, 0);

        for glyph in &self.glyphs {
            offset.x = offset.x.min(glyph.offset.0);
            offset.y = offset.y.min(glyph.offset.1);

            width = width.max(glyph.offset.0.max(0) as u32 + glyph.texture.width());
            height = height.max(glyph.offset.1.max(0) as u32 + glyph.texture.height());
        }

        width += (-offset.x).max(0) as u32;
        height += (-offset.y).max(0) as u32;

        rasterizer.blur_prepare(width, height, sigma);

        for bitmap in &self.glyphs {
            let offx = bitmap.offset.0 - offset.x;
            let offy = bitmap.offset.1 - offset.y;

            rasterizer.blur_buffer_blit(offx, offy, &bitmap.texture);
        }

        offset
    }
}

pub fn render(
    rasterizer: &mut dyn Rasterizer,
    xf: I26Dot6,
    yf: I26Dot6,
    glyphs: &GlyphString<'_, impl GlyphStringText>,
) -> Result<Image, GlyphRenderError> {
    let mut result = Image {
        glyphs: Vec::new(),
        monochrome: OnceCell::new(),
    };

    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&xf));
    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&yf));

    let mut x = xf;
    let mut y = yf;
    for shaped_glyph in glyphs.iter_glyphs() {
        // TODO: Once vertical text is supported, this should change depending on main axis
        let subpixel_offset = if x < 0 {
            I26Dot6::ONE - x.abs().fract()
        } else {
            x.fract()
        };

        let font = shaped_glyph.font;
        let cached = font.render_glyph(rasterizer, shaped_glyph.index, subpixel_offset, false)?;

        result.glyphs.push(GlyphBitmap {
            offset: (
                (x + cached.offset.x + shaped_glyph.x_offset).floor_to_inner(),
                (y + cached.offset.y + shaped_glyph.y_offset).floor_to_inner(),
            ),
            texture: cached.texture.clone(),
        });

        x += shaped_glyph.x_advance;
        y += shaped_glyph.y_advance;
    }

    Ok(result)
}
