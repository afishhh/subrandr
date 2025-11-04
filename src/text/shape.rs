use std::{
    mem::MaybeUninit,
    ops::{Range, RangeFrom, RangeFull},
};

use text_sys::*;
use thiserror::Error;

use super::{Direction, FontArena, FontDb, FontMatchIterator, Glyph};

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
    FreeType(#[from] super::FreeTypeError),
    #[error("font selection: {0}")]
    FontSelect(#[from] super::font_db::SelectError),
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
                hb_buffer_set_segment_properties(sub_buffer.buffer, properties);
                hb_buffer_set_content_type(sub_buffer.buffer, HB_BUFFER_CONTENT_TYPE_UNICODE);
                for ((codepoint, _), i) in
                    codepoints[range.clone()].iter().copied().zip(range.clone())
                {
                    hb_buffer_add(sub_buffer.buffer, codepoint, i as u32);
                }

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

                let left = infos[start].cluster as usize;
                let right = infos.last().unwrap().cluster as usize;
                retry_shaping(
                    if left > right {
                        right..left + 1
                    } else {
                        left..right + 1
                    },
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
