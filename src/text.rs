use std::{
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
};

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

// TODO: Just copy the glyphs...
pub struct Glyphs {
    // NOTE: These are not 'static, just self referential
    infos: &'static mut [hb_glyph_info_t],
    positions: &'static mut [hb_glyph_position_t],
    buffer: *mut hb_buffer_t,
}

macro_rules! define_glyph_accessors {
    () => {
        #[inline(always)]
        #[allow(dead_code)]
        pub fn codepoint(&self) -> u32 {
            self.info.codepoint
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn x_advance(&self) -> i32 {
            self.position.x_advance
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn y_advance(&self) -> i32 {
            self.position.y_advance
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn x_offset(&self) -> i32 {
            self.position.x_offset
        }

        #[inline(always)]
        #[allow(dead_code)]
        pub fn y_offset(&self) -> i32 {
            self.position.y_offset
        }
    };
}

#[derive(Clone, Copy)]
pub struct Glyph<'a> {
    info: &'a hb_glyph_info_t,
    position: &'a hb_glyph_position_t,
}

impl Glyph<'_> {
    define_glyph_accessors!();
}

pub struct GlyphMut<'a> {
    info: &'a mut hb_glyph_info_t,
    position: &'a mut hb_glyph_position_t,
}

impl GlyphMut<'_> {
    define_glyph_accessors!();
}

macro_rules! define_glyph_fmt {
    () => {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Glyph")
                .field("codepoint", &self.codepoint())
                .field("x_advance", &self.x_advance())
                .field("y_advance", &self.y_advance())
                .field("x_offset", &self.x_offset())
                .field("y_offset", &self.y_offset())
                .finish()
        }
    };
}

impl std::fmt::Debug for Glyph<'_> {
    define_glyph_fmt!();
}

impl std::fmt::Debug for GlyphMut<'_> {
    define_glyph_fmt!();
}

impl Glyphs {
    unsafe fn from_shaped_buffer(buffer: *mut hb_buffer_t) -> Self {
        let infos = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        let positions = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(buffer, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        assert_eq!(infos.len(), positions.len());

        Self {
            infos,
            positions,
            buffer,
        }
    }

    pub fn len(&self) -> usize {
        self.infos.len()
    }

    #[expect(dead_code)]
    pub fn get(&self, index: usize) -> Option<Glyph> {
        self.infos.get(index).map(|info| unsafe {
            let position = self.positions.get_unchecked(index);
            Glyph { info, position }
        })
    }

    pub fn last(&self) -> Option<Glyph> {
        self.infos.last().map(|info| unsafe {
            let position = self.positions.last().unwrap_unchecked();
            Glyph { info, position }
        })
    }

    #[expect(dead_code)]
    pub fn get_mut(&mut self, index: usize) -> Option<GlyphMut> {
        self.infos.get_mut(index).map(|info| unsafe {
            let position = self.positions.get_unchecked_mut(index);
            GlyphMut { info, position }
        })
    }

    pub fn iter_slice(
        &self,
        start: usize,
        end: usize,
    ) -> impl Iterator<Item = Glyph> + ExactSizeIterator + DoubleEndedIterator {
        (start..end).into_iter().map(|i| Glyph {
            info: &self.infos[i],
            position: &self.positions[i],
        })
    }

    pub fn iter(&self) -> impl Iterator<Item = Glyph> + ExactSizeIterator + DoubleEndedIterator {
        self.iter_slice(0, self.infos.len())
    }

    pub fn compute_extents_for_slice_ex(
        &self,
        font: &Font,
        range: Range<usize>,
    ) -> (TextExtents, (i32, i32)) {
        unsafe {
            let (_, hb_font) = font.with_applied_size_and_hb();

            let direction = hb_buffer_get_direction(self.buffer);

            let mut results = TextExtents {
                paint_height: 0,
                paint_width: 0,
            };

            let mut iterator = self.iter_slice(range.start, range.end);

            let trailing_advance;

            if let Some(glyph) = iterator.next_back() {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, glyph.codepoint(), extents.as_mut_ptr(),)
                        > 0
                );
                let extents = extents.assume_init();
                results.paint_height += extents.height.abs();
                results.paint_width += extents.width;
                if direction_is_horizontal(direction) {
                    trailing_advance = ((glyph.x_advance() - extents.width), 0);
                } else {
                    trailing_advance = (0, (glyph.y_advance() - extents.height));
                }
            } else {
                trailing_advance = (0, 0);
            }

            for glyph in iterator {
                let mut extents = MaybeUninit::uninit();
                assert!(
                    hb_font_get_glyph_extents(hb_font, glyph.codepoint(), extents.as_mut_ptr()) > 0
                );
                let extents = extents.assume_init();
                if direction_is_horizontal(direction) {
                    results.paint_height = results.paint_height.max(extents.height.abs());
                    results.paint_width += glyph.x_advance();
                } else {
                    results.paint_width = results.paint_width.max(extents.width.abs());
                    results.paint_height += glyph.y_advance();
                }
            }

            (results, trailing_advance)
        }
    }

    pub fn compute_extents_ex(&self, font: &Font) -> (TextExtents, (i32, i32)) {
        self.compute_extents_for_slice_ex(font, 0..self.infos.len())
    }

    pub fn compute_extents(&self, font: &Font) -> TextExtents {
        self.compute_extents_ex(font).0
    }
}

impl Drop for Glyphs {
    fn drop(&mut self) {
        unsafe {
            hb_buffer_destroy(self.buffer);
        }
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

#[inline(always)]
fn direction_is_horizontal(dir: hb_direction_t) -> bool {
    dir == hb_direction_t_HB_DIRECTION_LTR || dir == hb_direction_t_HB_DIRECTION_RTL
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

    pub fn shape(self, font: &Font) -> Glyphs {
        let (_, hb_font) = font.with_applied_size_and_hb();

        unsafe {
            hb_buffer_guess_segment_properties(self.buffer);
            hb_shape(hb_font, self.buffer, std::ptr::null(), 0);

            Glyphs::from_shaped_buffer(ManuallyDrop::new(self).buffer)
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

pub fn shape_text(font: &Font, text: &str) -> Glyphs {
    let mut buffer = ShapingBuffer::new();
    buffer.add(text);
    buffer.shape(font)
}

#[derive(Debug, Clone, Copy)]
pub struct TextExtents {
    pub paint_height: i32,
    pub paint_width: i32,
}

#[allow(clippy::too_many_arguments)]
pub fn paint<'a>(
    // RGBA8 buffer
    buffer: &mut [u8],
    baseline_x: usize,
    baseline_y: usize,
    width: usize,
    height: usize,
    stride: usize,
    font: &Font,
    glyphs: impl IntoIterator<Item = Glyph<'a>>,
    // RGB color components
    color: [u8; 3],
    alpha: f32,
) -> (u32, u32) {
    unsafe {
        let face = font.with_applied_size();

        let mut x = baseline_x as u32;
        let mut y = baseline_y as u32;
        for Glyph { info, position } in glyphs {
            fttry!(FT_Load_Glyph(face, info.codepoint, FT_LOAD_COLOR as i32));
            let glyph = (*face).glyph;
            fttry!(FT_Render_Glyph(
                glyph,
                FT_Render_Mode__FT_RENDER_MODE_NORMAL
            ));

            let (ox, oy) = (
                (*glyph).bitmap_left + position.x_offset / 64,
                -(*glyph).bitmap_top + position.y_offset / 64,
            );
            let bitmap = &(*glyph).bitmap;

            // dbg!(bitmap.width, bitmap.rows);
            // dbg!((*glyph).bitmap_left, (*glyph).bitmap_top);

            #[expect(non_upper_case_globals)]
            let pixel_width = match bitmap.pixel_mode.into() {
                FT_Pixel_Mode__FT_PIXEL_MODE_GRAY => 1,
                _ => todo!("ft pixel mode {:?}", bitmap.pixel_mode),
            };

            for biy in 0..bitmap.rows {
                for bix in 0..(bitmap.width / pixel_width) {
                    let fx = x as i32 + ox + bix as i32;
                    let fy = y as i32 + oy + biy as i32;

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
                    #[expect(non_upper_case_globals)]
                    let (colors, alpha) = match bitmap.pixel_mode.into() {
                        FT_Pixel_Mode__FT_PIXEL_MODE_GRAY => (
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
                    // eprintln!(
                    //     "{fx} {fy} = [{i}] = {colors:?} =over= {:?}",
                    //     &buffer[i..i + 4]
                    // );
                }
            }

            // eprintln!("advance: {} {}", (position.x_advance as f32) / 64., (position.y_advance as f32) / 64.);
            x = x.checked_add_signed(position.x_advance / 64).unwrap();
            y = y.checked_add_signed(position.y_advance / 64).unwrap();
        }

        (x, y)
    }
}
