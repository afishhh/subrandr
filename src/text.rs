use std::{cell::OnceCell, mem::MaybeUninit, ops::Range, rc::Rc};

use ft_utils::IFixed26Dot6;
use text_sys::*;

mod face;
mod ft_utils;
pub use face::*;
pub mod font_select;
pub use font_select::*;

use crate::{
    color::{BlendMode, BGRA8},
    util::{calculate_blit_rectangle, AnyError, BlitRectangle, OrderedF32},
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

#[derive(Debug, Clone)]
pub struct Glyph {
    pub index: hb_codepoint_t,
    /// Byte position where this glyph started in the original UTF-8 string
    pub cluster: usize,
    pub x_advance: IFixed26Dot6,
    pub y_advance: IFixed26Dot6,
    pub x_offset: IFixed26Dot6,
    pub y_offset: IFixed26Dot6,
    pub font_index: usize,
    flags: hb_glyph_flags_t,
}

impl Glyph {
    fn from_info_and_position(
        info: &hb_glyph_info_t,
        position: &hb_glyph_position_t,
        original_cluster: usize,
        font_index: usize,
    ) -> Self {
        Self {
            index: info.codepoint,
            cluster: original_cluster,
            x_advance: IFixed26Dot6::from_raw(position.x_advance),
            y_advance: IFixed26Dot6::from_raw(position.y_advance),
            x_offset: IFixed26Dot6::from_raw(position.x_offset),
            y_offset: IFixed26Dot6::from_raw(position.y_offset),
            font_index,
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

pub fn compute_extents_ex(
    horizontal: bool,
    fonts: &[Font],
    glyphs: &[Glyph],
) -> (TextExtents, (IFixed26Dot6, IFixed26Dot6)) {
    let mut results = TextExtents {
        paint_height: IFixed26Dot6::ZERO,
        paint_width: IFixed26Dot6::ZERO,
    };

    let trailing_advance;

    let mut glyphs = glyphs.iter();

    if let Some(glyph) = glyphs.next_back() {
        let extents = fonts[glyph.font_index].as_ref().glyph_extents(glyph.index);
        results.paint_height += extents.height.abs();
        results.paint_width += extents.width;
        if horizontal {
            trailing_advance = ((glyph.x_advance - extents.width), IFixed26Dot6::ZERO);
        } else {
            trailing_advance = (IFixed26Dot6::ZERO, (glyph.y_advance - extents.height));
        }
    } else {
        trailing_advance = (IFixed26Dot6::ZERO, IFixed26Dot6::ZERO);
    }

    for glyph in glyphs {
        let extents = fonts[glyph.font_index].as_ref().glyph_extents(glyph.index);
        if horizontal {
            results.paint_height = results.paint_height.max(extents.height.abs());
            results.paint_width += glyph.x_advance;
        } else {
            results.paint_width = results.paint_width.max(extents.width.abs());
            results.paint_height += glyph.y_advance;
        }
    }

    (results, trailing_advance)
}

pub fn compute_extents(horizontal: bool, fonts: &[Font], glyphs: &[Glyph]) -> TextExtents {
    compute_extents_ex(horizontal, fonts, glyphs).0
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

    fn shape_rec(
        &mut self,
        result: &mut Vec<Glyph>,
        fallbacks: &mut Vec<Font>,
        font_index: usize,
        codepoints: &[(u32, u32)],
        properties: &hb_segment_properties_t,
        font: &Font,
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
                    font_index,
                )
            };
            let mut reshape_with_fallback = |range: Range<usize>,
                                             result: &mut Vec<Glyph>,
                                             fallbacks: &mut Vec<Font>,
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

                    // TODO: figure out a way to not clone this and still have corrent font_idx
                    //       will require MaybeUninit?
                    let new_idx = fallbacks.len();
                    fallbacks.push(font.clone());

                    sub_buffer.shape_rec(
                        result, fallbacks, new_idx, codepoints, properties, &font, fallback,
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
                        fallbacks,
                        (&infos[info_range.clone()], &positions[info_range]),
                    );
                }

                result.push(make_glyph(info, position));
            }

            if let Some(start) = invalid_range_start {
                if start == 0 && font_index != 0 {
                    for (info, position) in infos.iter().zip(positions.iter()) {
                        result.push(make_glyph(info, position));
                    }
                    return;
                }

                let info_range = start..infos.len();
                reshape_with_fallback(
                    infos[start].cluster as usize..infos.last().unwrap().cluster as usize + 1,
                    result,
                    fallbacks,
                    (&infos[info_range.clone()], &positions[info_range]),
                )
            }
        }
    }

    pub fn shape(
        &mut self,
        font: &Font,
        fonts_output: &mut Vec<Font>,
        fallback: &mut impl FallbackFontProvider,
    ) -> Vec<Glyph> {
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

        fonts_output.push(font.clone());

        let mut result = Vec::new();
        self.shape_rec(
            &mut result,
            fonts_output,
            0,
            &codepoints,
            &properties,
            font,
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
pub struct ShapedText {
    pub direction: Direction,
    pub glyphs: Vec<Glyph>,
}

pub fn shape_text(font: &Font, text: &str) -> ShapedText {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text);
    let glyphs = buffer.shape(font, &mut Vec::new(), &mut NoopFallbackProvider);
    ShapedText {
        direction: buffer.direction().unwrap(),
        glyphs,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextExtents {
    pub paint_height: IFixed26Dot6,
    pub paint_width: IFixed26Dot6,
}

struct GlyphBitmap {
    offset: (i32, i32),
    width: u32,
    height: u32,
    data: Rc<BufferData>,
}

/// Merged monochrome bitmap of the whole text string, useful for shadows.
pub struct MonochromeImage {
    pub offset: (i32, i32),
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl MonochromeImage {
    pub fn from_image(image: &Image) -> Self {
        let mut result = MonochromeImage {
            offset: (0, 0),
            width: 0,
            height: 0,
            data: Vec::new(),
        };

        for glyph in &image.glyphs {
            result.offset.0 = result.offset.0.min(glyph.offset.0);
            result.offset.1 = result.offset.1.min(glyph.offset.1);

            result.width = result.width.max(glyph.offset.0.max(0) as u32 + glyph.width);
            result.height = result
                .height
                .max(glyph.offset.1.max(0) as u32 + glyph.height);
        }

        result.width += (-result.offset.0).max(0) as u32;
        result.height += (-result.offset.1).max(0) as u32;

        // NOTE: We cannot MaybeUninit here because the glyphs may have gaps
        //       between them that will be left uninitialized
        result.data = vec![0; result.width as usize * result.height as usize];

        for bitmap in &image.glyphs {
            match &*bitmap.data {
                BufferData::Monochrome(source) => {
                    let offx = (bitmap.offset.0 - result.offset.0) as usize;
                    let offy = (bitmap.offset.1 - result.offset.1) as usize;
                    for sy in 0..bitmap.height as usize {
                        for sx in 0..bitmap.width as usize {
                            let si = sy * bitmap.width as usize + sx;
                            let di = (offy + sy) * result.width as usize + (offx + sx);
                            result.data[di] = source[si];
                        }
                    }
                }
                BufferData::Color(source) => {
                    let offx = (bitmap.offset.0 - result.offset.0) as usize;
                    let offy = (bitmap.offset.1 - result.offset.1) as usize;
                    for sy in 0..bitmap.height as usize {
                        for sx in 0..bitmap.width as usize {
                            let si = sy * bitmap.width as usize + sx;
                            let di = (offy + sy) * result.width as usize + (offx + sx);
                            result.data[di] = source[si].a;
                        }
                    }
                }
            }
        }

        result
    }
}

pub struct Image {
    // TODO: Test whether combining bitmaps of the same type helps with performance.
    //       Probably not worth it though even if it helps, it'll probably be very
    //       expensive even if done just once, will cause lag on first frames.
    glyphs: Vec<GlyphBitmap>,
    monochrome: OnceCell<MonochromeImage>,
}

impl GlyphBitmap {
    fn blit_monochrome(
        &self,
        dx: i32,
        dy: i32,
        buffer: &mut [BGRA8],
        stride: u32,
        color: [u8; 3],
        alpha: f32,
        ys: Range<usize>,
        xs: Range<usize>,
        source: &[u8],
    ) {
        for y in ys {
            let fy = dy + self.offset.1 + y as i32;
            for x in xs.clone() {
                let fx = dx + self.offset.0 + x as i32;

                let si = y * self.width as usize + x;
                let sv = *unsafe { source.get_unchecked(si) };
                let na = sv as f32 / 255.0 * alpha;
                let bgr = color.map(|c| srgb_to_linear(c) * na);

                let di = (fx as usize) + (fy as usize) * stride as usize;
                BlendMode::Over.blend_with_linear_parts(
                    unsafe { buffer.get_unchecked_mut(di) },
                    bgr,
                    na,
                );
            }
        }
    }

    fn blit_bgra(
        &self,
        dx: i32,
        dy: i32,
        buffer: &mut [BGRA8],
        stride: u32,
        alpha: f32,
        ys: Range<usize>,
        xs: Range<usize>,
        source: &[BGRA8],
    ) {
        for y in ys {
            let fy = dy + self.offset.1 + y as i32;
            for x in xs.clone() {
                let fx = dx + self.offset.0 + x as i32;

                let si = y * self.width as usize + x;
                let nbgr = source[si].to_bgr_bytes().map(|v| srgb_to_linear(v) * alpha);
                let na = source[si].a as f32 / 255.0 * alpha;

                let di = (fx as usize) + (fy as usize) * stride as usize;
                BlendMode::Over.blend_with_linear_parts(
                    unsafe { buffer.get_unchecked_mut(di) },
                    nbgr,
                    na,
                );
            }
        }
    }

    fn blit(
        &self,
        dx: i32,
        dy: i32,
        buffer: &mut [BGRA8],
        width: u32,
        stride: u32,
        height: u32,
        color: [u8; 3],
        alpha: f32,
    ) {
        let Some(BlitRectangle { xs, ys }) = calculate_blit_rectangle(
            self.offset.0 + dx,
            self.offset.1 + dy,
            width as usize,
            height as usize,
            self.width as usize,
            self.height as usize,
        ) else {
            return;
        };

        match &*self.data {
            BufferData::Monochrome(pixels) => {
                self.blit_monochrome(dx, dy, buffer, stride, color, alpha, ys, xs, pixels);
            }
            BufferData::Color(pixels) => {
                self.blit_bgra(dx, dy, buffer, stride, alpha, ys, xs, pixels);
            }
        }
    }
}

impl Image {
    pub fn monochrome(&self) -> &MonochromeImage {
        self.monochrome
            .get_or_init(|| MonochromeImage::from_image(self))
    }
}

impl Image {
    pub fn blit(
        &self,
        dx: i32,
        dy: i32,
        buffer: &mut [BGRA8],
        width: u32,
        stride: u32,
        height: u32,
        color: [u8; 3],
        alpha: f32,
    ) {
        for glyph in &self.glyphs {
            glyph.blit(dx, dy, buffer, width, stride, height, color, alpha);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(xf: IFixed26Dot6, yf: IFixed26Dot6, fonts: &[Font], glyphs: &[Glyph]) -> Image {
    let mut result = Image {
        glyphs: Vec::new(),
        monochrome: OnceCell::new(),
    };

    assert!((-IFixed26Dot6::ONE..IFixed26Dot6::ONE).contains(&xf));
    assert!((-IFixed26Dot6::ONE..IFixed26Dot6::ONE).contains(&yf));

    let mut x = xf;
    let mut y = yf;
    for shaped_glyph in glyphs {
        let font = &fonts[shaped_glyph.font_index];
        let cached = font.render_glyph(shaped_glyph.index);

        result.glyphs.push(GlyphBitmap {
            offset: (
                (x + cached.offset.0 + shaped_glyph.x_offset).trunc_to_inner(),
                (y + cached.offset.1 + shaped_glyph.y_offset).trunc_to_inner(),
            ),
            width: cached.width,
            height: cached.height,
            data: cached.data.clone(),
        });

        x += shaped_glyph.x_advance;
        y += shaped_glyph.y_advance;
    }

    result
}
