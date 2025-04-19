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
pub mod font_select;
pub use font_select::*;
pub mod layout;

use crate::{
    color::BGRA8,
    math::{I16Dot16, I26Dot6, Vec2},
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

#[derive(Debug, Clone)]
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

    // TODO: These will be useful for improving the correctness and performance of line breaking
    #[expect(dead_code)]
    pub fn unsafe_to_break(&self) -> bool {
        (self.flags & HB_GLYPH_FLAG_UNSAFE_TO_BREAK) != 0
    }

    #[expect(dead_code)]
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

pub fn compute_extents_ex(
    horizontal: bool,
    glyphs: &[Glyph],
) -> Result<TextMetrics, FreeTypeError> {
    let mut results = TextMetrics {
        paint_size: Vec2::ZERO,
        trailing_advance: I26Dot6::ZERO,
        max_bearing_y: I26Dot6::ZERO,
        max_ascender: I26Dot6::ZERO,
        min_descender: I26Dot6::MAX,
        max_lineskip_descent: I26Dot6::ZERO,
    };

    if glyphs.len() == 0 {
        results.min_descender = I26Dot6::ZERO;
        return Ok(results);
    }

    let mut glyphs = glyphs.iter();

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

pub trait FallbackFontProvider {
    fn get_font_for_glyph(
        &mut self,
        weight: I16Dot16,
        italic: bool,
        codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, font_select::Error>;
}

pub struct NoopFallbackProvider;
impl FallbackFontProvider for NoopFallbackProvider {
    fn get_font_for_glyph(
        &mut self,
        _weight: I16Dot16,
        _italic: bool,
        _codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, font_select::Error> {
        Ok(None)
    }
}

impl FallbackFontProvider for FontSelect<'_> {
    fn get_font_for_glyph(
        &mut self,
        weight: I16Dot16,
        italic: bool,
        codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, font_select::Error> {
        let request = FontRequest {
            families: Vec::new(),
            weight,
            italic,
            codepoint: Some(codepoint),
        };
        match self.select(&request) {
            Ok(face) => Ok(Some(face)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e),
        }
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
        b..a
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
    FontSelect(#[from] font_select::Error),
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

    fn glyphs(&self) -> (&mut [hb_glyph_info_t], &mut [hb_glyph_position_t]) {
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

    fn shape_rec<'f>(
        &mut self,
        result: &mut Vec<Glyph<'f>>,
        font_arena: &'f FontArena,
        initial_pass: bool,
        codepoints: &[(u32, u32)],
        properties: &hb_segment_properties_t,
        font: &'f Font,
        fallback: &mut impl FallbackFontProvider,
    ) -> Result<(), ShapingError> {
        let (_, hb_font) = font.with_applied_size_and_hb()?;

        unsafe {
            hb_shape(hb_font, self.buffer, std::ptr::null(), 0);

            let (infos, positions) = self.glyphs();

            result.reserve(self.glyphs().0.len());
            let mut invalid_range_start = None;

            let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t| {
                Glyph::from_info_and_position(
                    info,
                    position,
                    codepoints[info.cluster as usize].1 as usize,
                    font,
                )
            };
            let mut reshape_with_fallback = |range: Range<usize>,
                                             result: &mut Vec<Glyph<'f>>,
                                             font_arena: &'f FontArena,
                                             passthrough: (
                &[hb_glyph_info_t],
                &[hb_glyph_position_t],
            )|
             -> Result<(), ShapingError> {
                if let Some(face) = fallback.get_font_for_glyph(
                    font.weight(),
                    font.italic(),
                    codepoints[range.start].0,
                )? {
                    let font = face.with_size_from(font)?;
                    let mut sub_buffer = Self::new();
                    for ((codepoint, _), i) in codepoints[range.clone()].iter().copied().zip(range)
                    {
                        hb_buffer_add(sub_buffer.buffer, codepoint, i as u32);
                    }
                    hb_buffer_set_segment_properties(sub_buffer.buffer, properties);
                    hb_buffer_set_content_type(sub_buffer.buffer, HB_BUFFER_CONTENT_TYPE_UNICODE);

                    sub_buffer.shape_rec(
                        result,
                        font_arena,
                        false,
                        codepoints,
                        properties,
                        font_arena.insert(&font),
                        fallback,
                    )?;
                } else {
                    result.extend(
                        passthrough
                            .0
                            .iter()
                            .zip(passthrough.1.iter())
                            .map(|(a, b)| make_glyph(a, b)),
                    );
                }

                Ok(())
            };

            for (i, (info, position)) in infos.iter().zip(positions.iter()).enumerate() {
                if info.codepoint == 0 {
                    if invalid_range_start.is_none() {
                        invalid_range_start = Some(i)
                    }
                    continue;
                } else if let Some(start) = invalid_range_start.take() {
                    let info_range = start..i;
                    reshape_with_fallback(
                        fixup_range(infos[start].cluster as usize, info.cluster as usize),
                        result,
                        font_arena,
                        (&infos[info_range.clone()], &positions[info_range]),
                    )?;
                }

                result.push(make_glyph(info, position));
            }

            if let Some(start) = invalid_range_start {
                if start == 0 && !initial_pass {
                    for (info, position) in infos.iter().zip(positions.iter()) {
                        result.push(make_glyph(info, position));
                    }
                    return Ok(());
                }

                let info_range = start..infos.len();
                reshape_with_fallback(
                    fixup_range(
                        infos[start].cluster as usize,
                        infos.last().unwrap().cluster as usize + 1,
                    ),
                    result,
                    font_arena,
                    (&infos[info_range.clone()], &positions[info_range]),
                )?
            }

            Ok(())
        }
    }

    pub fn shape<'f>(
        &mut self,
        font: &Font,
        font_arena: &'f FontArena,
        fallback: &mut impl FallbackFontProvider,
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

        let mut result = Vec::new();
        self.shape_rec(
            &mut result,
            font_arena,
            true,
            &codepoints,
            &properties,
            font_arena.insert(font),
            fallback,
        )?;

        Ok(result)
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
    font: &Font,
    font_arena: &'f FontArena,
    text: &str,
) -> Result<Vec<Glyph<'f>>, ShapingError> {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text, ..);
    buffer.shape(font, font_arena, &mut NoopFallbackProvider)
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
    glyphs: &[Glyph],
) -> Result<Image, FreeTypeError> {
    let mut result = Image {
        glyphs: Vec::new(),
        monochrome: OnceCell::new(),
    };

    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&xf));
    assert!((-I26Dot6::ONE..I26Dot6::ONE).contains(&yf));

    let mut x = xf;
    let mut y = yf;
    for shaped_glyph in glyphs {
        let font = shaped_glyph.font;
        let cached = font.render_glyph(rasterizer, shaped_glyph.index)?;

        result.glyphs.push(GlyphBitmap {
            offset: (
                (x + cached.offset.x + shaped_glyph.x_offset).trunc_to_inner(),
                (y + cached.offset.y + shaped_glyph.y_offset).trunc_to_inner(),
            ),
            texture: cached.texture.clone(),
        });

        x += shaped_glyph.x_advance;
        y += shaped_glyph.y_advance;
    }

    Ok(result)
}
