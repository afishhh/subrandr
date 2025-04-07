use std::{
    cell::{OnceCell, UnsafeCell},
    collections::HashSet,
    mem::MaybeUninit,
    ops::Range,
};

use text_sys::*;

mod face;
mod ft_utils;
pub use face::*;
pub mod font_select;
pub use font_select::*;
pub mod layout;

use crate::{
    color::BGRA8,
    math::{I26Dot6, Vec2},
    rasterize::{Rasterizer, RenderTarget, Texture},
    util::{AnyError, OrderedF32, ReadonlyAliasableBox},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ltr = HB_DIRECTION_LTR as isize,
    Rtl = HB_DIRECTION_RTL as isize,
    Ttb = HB_DIRECTION_TTB as isize,
    Btt = HB_DIRECTION_BTT as isize,
}

impl Direction {
    fn from_hb(value: hb_direction_t) -> Self {
        match value {
            HB_DIRECTION_LTR => Self::Ltr,
            HB_DIRECTION_RTL => Self::Rtl,
            HB_DIRECTION_TTB => Self::Ttb,
            HB_DIRECTION_BTT => Self::Btt,
            _ => panic!("Invalid harfbuzz direction: {value}"),
        }
    }

    fn from_hb_optional(value: hb_direction_t) -> Option<Self> {
        if value == HB_DIRECTION_INVALID {
            None
        } else {
            Some(Self::from_hb(value))
        }
    }

    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::Ltr | Self::Rtl)
    }

    pub const fn is_vertical(self) -> bool {
        !self.is_horizontal()
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
    fn extend_by_font(&mut self, font: &Font) {
        // FIXME: Is this bad for perf when done on all glyphs instead of just the unique fonts?
        let metrics = font.metrics();
        self.max_ascender = self.max_ascender.max(I26Dot6::from_ft(metrics.ascender));
        self.min_descender = self.min_descender.min(I26Dot6::from_ft(metrics.descender));
        self.max_lineskip_descent = self
            .max_lineskip_descent
            .max(I26Dot6::from_ft(metrics.height - metrics.ascender));
    }
}

pub fn compute_extents_ex(horizontal: bool, glyphs: &[Glyph]) -> TextMetrics {
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
        return results;
    }

    let mut glyphs = glyphs.iter();

    if let Some(glyph) = glyphs.next_back() {
        let extents = glyph.font.glyph_extents(glyph.index);
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
        let extents = glyph.font.glyph_extents(glyph.index);
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

    results
}

impl AsRef<Self> for Font {
    fn as_ref(&self) -> &Self {
        self
    }
}

// TODO: exact lookup table instead of this approximation?
#[inline(always)]
fn srgb_to_linear(color: u8) -> f32 {
    (color as f32 / 255.0).powf(1.0 / 2.2)
}

#[inline(always)]
fn blend_over(dst: f32, src: f32, alpha: f32) -> f32 {
    alpha * src + (1.0 - alpha) * dst
}

#[inline(always)]
fn linear_to_srgb(color: f32) -> u8 {
    (color.powf(2.2 / 1.0) * 255.0).round() as u8
}

pub trait FallbackFontProvider {
    fn get_font_for_glyph(
        &mut self,
        weight: f32,
        italic: bool,
        codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, AnyError>;
}

pub struct NoopFallbackProvider;
impl FallbackFontProvider for NoopFallbackProvider {
    fn get_font_for_glyph(
        &mut self,
        _weight: f32,
        _italic: bool,
        _codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, AnyError> {
        Ok(None)
    }
}

impl FallbackFontProvider for FontSelect {
    fn get_font_for_glyph(
        &mut self,
        weight: f32,
        italic: bool,
        codepoint: hb_codepoint_t,
    ) -> Result<Option<Face>, AnyError> {
        let request = FontRequest {
            families: Vec::new(),
            weight: OrderedF32(weight),
            italic,
            codepoint: Some(codepoint),
        };
        match self.select(&request) {
            Ok(face) => Ok(Some(face)),
            Err(Error::NotFound) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

pub struct ShapingBuffer {
    buffer: *mut hb_buffer_t,
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

    pub fn add(&mut self, text: &str) {
        unsafe {
            hb_buffer_add_utf8(
                self.buffer,
                text.as_ptr() as *const _,
                text.len() as i32,
                0,
                -1,
            );
        }
    }

    pub fn add_with_context(&mut self, text: &str, range: Range<usize>) {
        unsafe {
            hb_buffer_add_utf8(
                self.buffer,
                text.as_ptr() as *const _,
                text.len() as i32,
                range.start as u32,
                range.len() as i32,
            );
        }
    }

    pub fn len(&self) -> usize {
        unsafe { hb_buffer_get_length(self.buffer) as usize }
    }

    pub fn direction(&self) -> Option<Direction> {
        unsafe { Direction::from_hb_optional(hb_buffer_get_direction(self.buffer)) }
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
    ) {
        let (_, hb_font) = font.with_applied_size_and_hb();

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
            )| {
                if let Some(font) = fallback
                    .get_font_for_glyph(font.weight(), font.italic(), codepoints[range.start].0)
                    .unwrap()
                    .map(|face| face.with_size_from(font))
                {
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
                    );
                } else {
                    result.extend(
                        passthrough
                            .0
                            .iter()
                            .zip(passthrough.1.iter())
                            .map(|(a, b)| make_glyph(a, b)),
                    );
                }
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
                        infos[start].cluster as usize..info.cluster as usize,
                        result,
                        font_arena,
                        (&infos[info_range.clone()], &positions[info_range]),
                    );
                }

                result.push(make_glyph(info, position));
            }

            if let Some(start) = invalid_range_start {
                if start == 0 && !initial_pass {
                    for (info, position) in infos.iter().zip(positions.iter()) {
                        result.push(make_glyph(info, position));
                    }
                    return;
                }

                let info_range = start..infos.len();
                reshape_with_fallback(
                    infos[start].cluster as usize..infos.last().unwrap().cluster as usize + 1,
                    result,
                    font_arena,
                    (&infos[info_range.clone()], &positions[info_range]),
                )
            }
        }
    }

    pub fn shape<'f>(
        &mut self,
        font: &Font,
        font_arena: &'f FontArena,
        fallback: &mut impl FallbackFontProvider,
    ) -> Vec<Glyph<'f>> {
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
        );

        result
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

#[derive(Debug, Clone)]
pub struct ShapedText<'f> {
    pub glyphs: Vec<Glyph<'f>>,
    pub direction: Direction,
}

pub fn shape_text<'f>(font: &Font, font_arena: &'f FontArena, text: &str) -> ShapedText<'f> {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text);
    let glyphs = buffer.shape(font, font_arena, &mut NoopFallbackProvider);
    ShapedText {
        direction: buffer.direction().unwrap(),
        glyphs,
    }
}

struct GlyphBitmap {
    offset: (i32, i32),
    texture: Texture<'static>,
}

/// Merged monochrome bitmap of the whole text string, useful for shadows.
pub struct MonochromeImage {
    pub offset: Vec2<i32>,
    pub texture: Texture<'static>,
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

    pub fn blit_for_blur(&self, rasterizer: &mut dyn Rasterizer, sigma: f32) {
        let mut offset = (0, 0);
        let (mut width, mut height) = (0, 0);

        for glyph in &self.glyphs {
            offset.0 = offset.0.min(glyph.offset.0);
            offset.1 = offset.1.min(glyph.offset.1);

            width = width.max(glyph.offset.0.max(0) as u32 + glyph.texture.width());
            height = height.max(glyph.offset.1.max(0) as u32 + glyph.texture.height());
        }

        width += (-offset.0).max(0) as u32;
        height += (-offset.1).max(0) as u32;

        rasterizer.blur_prepare(width, height, sigma);

        for bitmap in &self.glyphs {
            let offx = bitmap.offset.0 - offset.0;
            let offy = bitmap.offset.1 - offset.1;

            rasterizer.blur_buffer_blit(offx, offy, &bitmap.texture);
        }
    }
}

pub fn render(
    rasterizer: &mut dyn Rasterizer,
    xf: I26Dot6,
    yf: I26Dot6,
    glyphs: &[Glyph],
) -> Image {
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
        let cached = font.render_glyph(rasterizer, shaped_glyph.index);

        result.glyphs.push(GlyphBitmap {
            offset: (
                (x + cached.offset.0 + shaped_glyph.x_offset).trunc_to_inner(),
                (y + cached.offset.1 + shaped_glyph.y_offset).trunc_to_inner(),
            ),
            texture: cached.texture.clone(),
        });

        x += shaped_glyph.x_advance;
        y += shaped_glyph.y_advance;
    }

    result
}
