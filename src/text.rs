use std::mem::MaybeUninit;

use text_sys::*;

mod ft_utils;
use ft_utils::*;
mod face;
pub use face::*;
mod font_manager;
pub use font_manager::*;

pub mod font_backend {
    #[cfg(target_family = "unix")]
    pub mod fontconfig;
    pub use fontconfig::FontconfigFontBackend;

    use super::FontBackend;
    use crate::util::AnyError;

    pub fn platform_default() -> Result<Box<dyn FontBackend>, AnyError> {
        #[cfg(target_family = "unix")]
        FontconfigFontBackend::new()
            .map(|x| Box::new(x) as Box<dyn FontBackend>)
            .map_err(Into::into)
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

#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub codepoint: hb_codepoint_t,
    pub x_advance: hb_position_t,
    pub y_advance: hb_position_t,
    pub x_offset: hb_position_t,
    pub y_offset: hb_position_t,
}

fn copy_hb_buffer_glyphs(buffer: *mut hb_buffer_t) -> Box<[Glyph]> {
    let infos: &[hb_glyph_info_t] = unsafe {
        let mut nglyphs = 0;
        let infos = hb_buffer_get_glyph_infos(buffer, &mut nglyphs);
        if infos.is_null() {
            &mut []
        } else {
            std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
        }
    };

    let positions: &[hb_glyph_position_t] = unsafe {
        let mut nglyphs = 0;
        let infos = hb_buffer_get_glyph_positions(buffer, &mut nglyphs);
        if infos.is_null() {
            &mut []
        } else {
            std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
        }
    };

    assert_eq!(infos.len(), positions.len());

    let mut result = Box::new_uninit_slice(infos.len());

    for (i, (info, position)) in infos.iter().zip(positions.iter()).enumerate() {
        result[i].write(Glyph {
            codepoint: info.codepoint,
            x_advance: position.x_advance,
            y_advance: position.y_advance,
            x_offset: position.x_offset,
            y_offset: position.y_offset,
        });
    }

    unsafe { result.assume_init() }
}

pub fn compute_extents_ex(
    horizontal: bool,
    font: &Font,
    mut glyphs: &[Glyph],
) -> (TextExtents, (i32, i32)) {
    unsafe {
        let (_, hb_font) = font.with_applied_size_and_hb();

        let mut results = TextExtents {
            paint_height: 0,
            paint_width: 0,
        };

        let trailing_advance;

        if let Some(glyph) = {
            if !glyphs.is_empty() {
                let glyph = glyphs.last().unwrap_unchecked();
                glyphs = &glyphs[..glyphs.len() - 1];
                Some(glyph)
            } else {
                None
            }
        } {
            let mut extents = MaybeUninit::uninit();
            assert!(hb_font_get_glyph_extents(hb_font, glyph.codepoint, extents.as_mut_ptr(),) > 0);
            let extents = extents.assume_init();
            results.paint_height += extents.height.abs();
            results.paint_width += extents.width;
            if horizontal {
                trailing_advance = ((glyph.x_advance - extents.width), 0);
            } else {
                trailing_advance = (0, (glyph.y_advance - extents.height));
            }
        } else {
            trailing_advance = (0, 0);
        }

        for glyph in glyphs {
            let mut extents = MaybeUninit::uninit();
            assert!(hb_font_get_glyph_extents(hb_font, glyph.codepoint, extents.as_mut_ptr()) > 0);
            let extents = extents.assume_init();
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
}

pub fn compute_extents(horizontal: bool, font: &Font, glyphs: &[Glyph]) -> TextExtents {
    compute_extents_ex(horizontal, font, glyphs).0
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

    pub fn shape(&mut self, font: &Font) -> Box<[Glyph]> {
        let (_, hb_font) = font.with_applied_size_and_hb();

        unsafe {
            hb_buffer_guess_segment_properties(self.buffer);
            hb_shape(hb_font, self.buffer, std::ptr::null(), 0);

            copy_hb_buffer_glyphs(self.buffer)
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

#[derive(Debug, Clone)]
pub struct ShapedText {
    pub direction: Direction,
    pub glyphs: Box<[Glyph]>,
}

pub fn shape_text(font: &Font, text: &str) -> ShapedText {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text);
    let glyphs = buffer.shape(font);
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

#[allow(clippy::too_many_arguments)]
pub fn paint(
    // RGBA8 buffer
    buffer: &mut [u8],
    baseline_x: i32,
    baseline_y: i32,
    width: usize,
    height: usize,
    stride: usize,
    font: &Font,
    glyphs: &[Glyph],
    // RGB color components
    color: [u8; 3],
    alpha: f32,
) -> (i32, i32) {
    unsafe {
        let face = font.with_applied_size();

        let mut x = baseline_x;
        let mut y = baseline_y;
        for shaped_glyph in glyphs {
            fttry!(FT_Load_Glyph(
                face,
                shaped_glyph.codepoint,
                FT_LOAD_COLOR as i32
            ));
            let glyph = (*face).glyph;
            fttry!(FT_Render_Glyph(glyph, FT_RENDER_MODE_NORMAL));

            let (ox, oy) = (
                (*glyph).bitmap_left + shaped_glyph.x_offset / 64,
                -(*glyph).bitmap_top + shaped_glyph.y_offset / 64,
            );
            let bitmap = &(*glyph).bitmap;

            let pixel_width = match bitmap.pixel_mode.into() {
                FT_PIXEL_MODE_GRAY => 1,
                _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
            };

            for biy in 0..bitmap.rows {
                for bix in 0..(bitmap.width / pixel_width) {
                    let fx = x + ox + bix as i32;
                    let fy = y + oy + biy as i32;

                    if fx < 0 || fy < 0 {
                        continue;
                    }

                    let fx = fx as usize;
                    let fy = fy as usize;
                    if fx >= width || fy >= height {
                        continue;
                    }

                    let bpos = (biy as i32 * bitmap.pitch) + (bix * pixel_width) as i32;
                    let bslice = std::slice::from_raw_parts(
                        bitmap.buffer.offset(bpos as isize),
                        pixel_width as usize,
                    );
                    let (colors, alpha) = match bitmap.pixel_mode.into() {
                        FT_PIXEL_MODE_GRAY => (
                            [color[0], color[1], color[2]],
                            (bslice[0] as f32 / 255.0) * alpha,
                        ),
                        _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
                    };

                    let i = fy * stride + fx * 4;
                    buffer[i] = linear_to_srgb(blend_over(
                        srgb_to_linear(buffer[i]),
                        srgb_to_linear(colors[0]),
                        alpha,
                    ));
                    buffer[i + 1] = linear_to_srgb(blend_over(
                        srgb_to_linear(buffer[i + 1]),
                        srgb_to_linear(colors[1]),
                        alpha,
                    ));
                    buffer[i + 2] = linear_to_srgb(blend_over(
                        srgb_to_linear(buffer[i + 2]),
                        srgb_to_linear(colors[2]),
                        alpha,
                    ));
                    buffer[i + 3] =
                        ((alpha + (buffer[i + 3] as f32 / 255.0) * (1.0 - alpha)) * 255.0) as u8;
                }
            }

            x += shaped_glyph.x_advance / 64;
            y += shaped_glyph.y_advance / 64;
        }

        (x, y)
    }
}
