use std::{cell::OnceCell, mem::MaybeUninit, ops::Range};

use text_sys::*;

mod ft_utils;
use ft_utils::*;
mod face;
pub use face::*;
pub mod font_select;
pub use font_select::*;

use crate::{
    color::{BlendMode, BGRA8},
    util::{AnyError, OrderedF32},
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
    // NOTE: hb_position_t seems to be a Fixed<6>
    pub x_advance: hb_position_t,
    pub y_advance: hb_position_t,
    pub x_offset: hb_position_t,
    pub y_offset: hb_position_t,
    pub font_index: usize,
}

impl Glyph {
    const fn from_info_and_position(
        info: &hb_glyph_info_t,
        position: &hb_glyph_position_t,
        original_cluster: usize,
        font_index: usize,
    ) -> Self {
        Self {
            index: info.codepoint,
            cluster: original_cluster,
            x_advance: position.x_advance,
            y_advance: position.y_advance,
            x_offset: position.x_offset,
            y_offset: position.y_offset,
            font_index,
        }
    }
}

pub fn compute_extents_ex(
    horizontal: bool,
    fonts: &[Font],
    glyphs: &[Glyph],
) -> (TextExtents, (i32, i32)) {
    let mut results = TextExtents {
        paint_height: 0,
        paint_width: 0,
    };

    let trailing_advance;

    let mut glyphs = glyphs.iter();

    if let Some(glyph) = glyphs.next_back() {
        let extents = fonts[glyph.font_index].as_ref().glyph_extents(glyph.index);
        results.paint_height += extents.height.abs() as i32;
        results.paint_width += extents.width as i32;
        if horizontal {
            trailing_advance = ((glyph.x_advance - extents.width as i32), 0);
        } else {
            trailing_advance = (0, (glyph.y_advance - extents.height as i32));
        }
    } else {
        trailing_advance = (0, 0);
    }

    for glyph in glyphs {
        let extents = fonts[glyph.font_index].as_ref().glyph_extents(glyph.index);
        if horizontal {
            results.paint_height = results.paint_height.max(extents.height.abs() as i32);
            results.paint_width += glyph.x_advance;
        } else {
            results.paint_width = results.paint_width.max(extents.width.abs() as i32);
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
    pub paint_height: i32,
    pub paint_width: i32,
}

struct GlyphBitmap {
    offset: (i32, i32),
    width: u32,
    height: u32,
    data: BufferData,
}

enum BufferData {
    Monochrome(Vec<u8>),
    Color(Vec<BGRA8>),
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
            match &bitmap.data {
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
                BufferData::Color(vec) => todo!(),
            }
        }

        result
    }
}

pub struct Image {
    // TODO: Test whether combining bitmaps of the same type helps with performance
    //       Probably will but combining is non-trivial, switching to a column-major order
    //       would likely hurt as cache locality will be terrible while blitting.
    glyphs: Vec<GlyphBitmap>,
    monochrome: OnceCell<MonochromeImage>,
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
            // TODO: delegate to specialized blit functions
            for y in 0..glyph.height as usize {
                for x in 0..glyph.width as usize {
                    let fx = dx + glyph.offset.0 + x as i32;
                    let fy = dy + glyph.offset.1 + y as i32;
                    if fx < 0 || fy < 0 || fx as u32 >= width || fy as u32 >= height {
                        continue;
                    }

                    let si = y * glyph.width as usize + x;
                    let ([b, g, r], a) = match &glyph.data {
                        BufferData::Monochrome(pixels) => {
                            let na = (pixels[si] as f32) / 255.0 * alpha;
                            (color.map(|c| (c as f32 * na) as u8), na)
                        }
                        BufferData::Color(pixels) => (
                            pixels[si].to_bgr_bytes(),
                            (pixels[si].a as f32) / 255.0 * alpha,
                        ),
                    };

                    let di = (fx as usize) + (fy as usize) * stride as usize;
                    BlendMode::Over.blend_with_parts(&mut buffer[di], [b, g, r], a);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(fonts: &[Font], glyphs: &[Glyph]) -> Image {
    let mut result = Image {
        glyphs: Vec::new(),
        monochrome: OnceCell::new(),
    };

    unsafe {
        let mut x = 0;
        let mut y = 0;
        for shaped_glyph in glyphs {
            let font = &fonts[shaped_glyph.font_index];
            let face = font.with_applied_size();

            fttry!(FT_Load_Glyph(
                face,
                shaped_glyph.index,
                FT_LOAD_COLOR as i32
            ));
            let glyph = (*face).glyph;
            fttry!(FT_Render_Glyph(glyph, FT_RENDER_MODE_NORMAL));

            let scale6 = font.scale.into_raw();

            let (ox, oy) = (
                ((*glyph).bitmap_left * scale6 + shaped_glyph.x_offset) >> 6,
                (-(*glyph).bitmap_top * scale6 + shaped_glyph.y_offset) >> 6,
            );

            let bitmap = &(*glyph).bitmap;

            let scaled_width = (bitmap.width * scale6 as u32) >> 6;
            let scaled_height = (bitmap.rows * scale6 as u32) >> 6;

            const MAX_PIXEL_WIDTH: usize = 4;

            let pixel_width = match bitmap.pixel_mode.into() {
                FT_PIXEL_MODE_GRAY => 1,
                FT_PIXEL_MODE_BGRA => 4,
                _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
            };

            let n_pixels = scaled_width as usize * scaled_height as usize;
            let mut glyph_result = GlyphBitmap {
                offset: ((x >> 6) + ox, (y >> 6) + oy),
                width: scaled_width,
                height: scaled_height,
                data: if pixel_width == 1 {
                    BufferData::Monochrome(Vec::with_capacity(n_pixels))
                } else {
                    BufferData::Color(Vec::with_capacity(n_pixels))
                },
            };

            let buffer_data = match &mut glyph_result.data {
                BufferData::Monochrome(vec) => vec.spare_capacity_mut(),
                BufferData::Color(vec) => std::slice::from_raw_parts_mut(
                    vec.spare_capacity_mut().as_mut_ptr() as *mut MaybeUninit<u8>,
                    vec.capacity() * std::mem::size_of::<BGRA8>(),
                ),
            };

            for biy in 0..scaled_height {
                for bix in 0..scaled_width {
                    let get_pixel_values = |x: u32, y: u32| -> [u8; MAX_PIXEL_WIDTH] {
                        let bpos = (y as i32 * bitmap.pitch) + (x * pixel_width) as i32;
                        let bslice = std::slice::from_raw_parts(
                            bitmap.buffer.offset(bpos as isize),
                            pixel_width as usize,
                        );
                        let mut pixel_data: [u8; MAX_PIXEL_WIDTH] = [0; 4];
                        pixel_data[..pixel_width as usize].copy_from_slice(bslice);
                        pixel_data
                    };

                    let interpolate_pixel_values =
                        |a: [u8; MAX_PIXEL_WIDTH], fa: u32, b: [u8; MAX_PIXEL_WIDTH], fb: u32| {
                            let mut r = [0; MAX_PIXEL_WIDTH];
                            for i in 0..pixel_width as usize {
                                r[i] = (((a[i] as u32 * fa) + (b[i] as u32 * fb)) >> 6) as u8;
                            }
                            r
                        };

                    let pixel_data = if scale6 == 64 {
                        get_pixel_values(bix, biy)
                    } else {
                        // bilinear scaling
                        let source_pixel_x6 = (bix << 12) / scale6 as u32;
                        let source_pixel_y6 = (biy << 12) / scale6 as u32;

                        let floor_x = source_pixel_x6 >> 6;
                        let floor_y = source_pixel_y6 >> 6;
                        let next_x = floor_x + 1;
                        let next_y = floor_y + 1;

                        let factor_floor_x = 64 - (source_pixel_x6 & 0x3F);
                        let factor_next_x = source_pixel_x6 & 0x3F;
                        let factor_floor_y = 64 - (source_pixel_y6 & 0x3F);
                        let factor_next_y = source_pixel_y6 & 0x3F;

                        if next_x >= bitmap.width {
                            if next_y >= bitmap.rows {
                                get_pixel_values(floor_x, floor_y)
                            } else {
                                let a = get_pixel_values(floor_x, floor_y);
                                let b = get_pixel_values(floor_x, next_y);
                                interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                            }
                        } else if next_y >= bitmap.rows {
                            let a = get_pixel_values(floor_x, floor_y);
                            let b = get_pixel_values(next_x, floor_y);
                            interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                        } else {
                            let a = {
                                let a = get_pixel_values(floor_x, floor_y);
                                let b = get_pixel_values(next_x, floor_y);
                                interpolate_pixel_values(a, factor_floor_x, b, factor_next_x)
                            };
                            let b = {
                                let a = get_pixel_values(floor_x, next_y);
                                let b = get_pixel_values(next_x, next_y);
                                interpolate_pixel_values(a, factor_floor_x, b, factor_next_x)
                            };
                            interpolate_pixel_values(a, factor_floor_y, b, factor_next_y)
                        }
                    };

                    let i = (bix as usize + biy as usize * scaled_width as usize)
                        * pixel_width as usize;
                    buffer_data[i..i + pixel_width as usize]
                        .copy_from_slice(std::mem::transmute(&pixel_data[..pixel_width as usize]));
                }
            }

            match &mut glyph_result.data {
                BufferData::Monochrome(vec) => vec.set_len(n_pixels),
                BufferData::Color(vec) => vec.set_len(n_pixels),
            }

            result.glyphs.push(glyph_result);

            x += shaped_glyph.x_advance;
            y += shaped_glyph.y_advance;
        }
    }

    result
}
