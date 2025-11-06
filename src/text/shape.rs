use std::{
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Range, RangeFrom, RangeFull},
};

use text_sys::*;
use thiserror::Error;
use util::{vec_into_parts, vec_parts};

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

struct RawShapingBuffer(*mut hb_buffer_t);

impl RawShapingBuffer {
    fn new() -> Self {
        let mut result = Self(unsafe {
            let buffer = hb_buffer_create();
            if hb_buffer_allocation_successful(buffer) == 0 {
                panic!("failed to allocate a harfbuzz buffer")
            }
            buffer
        });
        result.clear();
        result
    }

    fn clear(&mut self) {
        unsafe {
            hb_buffer_clear_contents(self.0);
            hb_buffer_set_flags(self.0, HB_BUFFER_FLAG_PRODUCE_UNSAFE_TO_CONCAT);
        }
    }

    fn items_mut(&mut self) -> (&mut [hb_glyph_info_t], &mut [hb_glyph_position_t]) {
        let infos: &mut [hb_glyph_info_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(self.0, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        let positions: &mut [hb_glyph_position_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(self.0, &mut nglyphs);
            if infos.is_null() {
                &mut []
            } else {
                std::slice::from_raw_parts_mut(infos as *mut _, nglyphs as usize)
            }
        };

        assert_eq!(infos.len(), positions.len());

        (infos, positions)
    }
}

pub struct ShapingBuffer {
    raw: RawShapingBuffer,
    cluster_map_buffer: Vec<(u32, u32)>,
    glyph_scratch_buffer_parts: (*mut Glyph<'static>, usize),
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
            raw: RawShapingBuffer::new(),
            cluster_map_buffer: Vec::new(),
            glyph_scratch_buffer_parts: {
                let (ptr, _, cap) = vec_into_parts(Vec::new());
                (ptr, cap)
            },
        }
    }

    pub fn add(&mut self, text: &str, range: impl ItemRange) {
        range.bounds_check(text.len());

        unsafe {
            hb_buffer_add_utf8(
                self.raw.0,
                text.as_ptr() as *const _,
                text.len() as i32,
                range.start(),
                range.length(),
            );
        }
    }

    pub fn direction(&self) -> Option<Direction> {
        unsafe { Direction::try_from_hb(hb_buffer_get_direction(self.raw.0)) }
    }

    pub fn set_direction(&mut self, direction: Direction) {
        unsafe {
            hb_buffer_set_direction(self.raw.0, direction as hb_direction_t);
        }
    }

    pub fn guess_properties(&mut self) -> Direction {
        unsafe {
            hb_buffer_guess_segment_properties(self.raw.0);
        }
        self.direction().unwrap()
    }

    pub fn clear(&mut self) {
        self.raw.clear();
    }

    pub fn shape<'f>(
        &mut self,
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Vec<Glyph<'f>>, ShapingError> {
        self.cluster_map_buffer.clear();
        self.cluster_map_buffer
            .extend(self.raw.items_mut().0.iter_mut().enumerate().map(|(i, x)| {
                let original_cluster = x.cluster;
                x.cluster = i as u32;
                (x.codepoint, original_cluster)
            }));

        let properties = unsafe {
            let mut buf = MaybeUninit::uninit();
            hb_buffer_guess_segment_properties(self.raw.0);
            hb_buffer_get_segment_properties(self.raw.0, buf.as_mut_ptr());
            buf.assume_init()
        };

        let mut result = Vec::with_capacity(self.cluster_map_buffer.len());
        ShapingPass {
            buffer: RawShapingBuffer(self.raw.0),
            glyph_buffer: unsafe {
                ManuallyDrop::new(Vec::from_raw_parts(
                    self.glyph_scratch_buffer_parts.0.cast(),
                    0,
                    self.glyph_scratch_buffer_parts.1,
                ))
            },
            glyph_buffer_parts: &mut self.glyph_scratch_buffer_parts,
            result: &mut result,
            cluster_map: &self.cluster_map_buffer,
            font_arena,
            fonts,
            properties,
        }
        .shape_layer(0..self.cluster_map_buffer.len(), font_iterator, false)?;

        self.clear();

        Ok(result)
    }
}

impl Drop for ShapingBuffer {
    fn drop(&mut self) {
        unsafe {
            hb_buffer_destroy(self.raw.0);
            drop(Vec::from_raw_parts(
                self.glyph_scratch_buffer_parts.0,
                0,
                self.glyph_scratch_buffer_parts.1,
            ));
        }
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

struct ShapingPass<'p, 'f, 'a> {
    buffer: RawShapingBuffer,
    glyph_buffer: ManuallyDrop<Vec<Glyph<'f>>>,
    glyph_buffer_parts: &'p mut (*mut Glyph<'static>, usize),
    result: &'p mut Vec<Glyph<'f>>,
    cluster_map: &'p [(u32, u32)],
    font_arena: &'f FontArena,
    fonts: &'p mut FontDb<'a>,
    properties: hb_segment_properties_t,
}

impl<'f> ShapingPass<'_, 'f, '_> {
    fn retry_shaping(
        &mut self,
        range: Range<usize>,
        font_iterator: FontMatchIterator<'_, 'f>,
        force_tofu: bool,
    ) -> Result<(), ShapingError> {
        unsafe {
            self.buffer.clear();
            hb_buffer_set_segment_properties(self.buffer.0, &self.properties);
            hb_buffer_set_content_type(self.buffer.0, HB_BUFFER_CONTENT_TYPE_UNICODE);
        }

        for (&(codepoint, _), i) in std::iter::zip(&self.cluster_map[range.clone()], range.clone())
        {
            unsafe {
                hb_buffer_add(self.buffer.0, codepoint, i as u32);
            }
        }

        self.shape_layer(range, font_iterator, force_tofu)?;

        Ok(())
    }

    // TODO: Reshape only on grapheme boundaries?
    fn shape_layer(
        &mut self,
        cluster_range: Range<usize>,
        mut font_iterator: FontMatchIterator<'_, 'f>,
        force_tofu: bool,
    ) -> Result<(), ShapingError> {
        let Some(&(first_codepoint, _)) = self.cluster_map.get(cluster_range.start) else {
            return Ok(());
        };

        let font = if force_tofu {
            font_iterator.matcher().tofu(self.font_arena)
        } else {
            font_iterator
                .next_with_fallback(first_codepoint, self.font_arena, self.fonts)?
                .unwrap_or_else(|| font_iterator.matcher().tofu(self.font_arena))
        };
        let hb_font = font.as_harfbuzz_font()?;

        unsafe {
            hb_shape(hb_font, self.buffer.0, std::ptr::null(), 0);
        }
        let (infos, positions) = self.buffer.items_mut();

        if infos.is_empty() {
            return Ok(());
        }

        let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t| {
            Glyph::from_info_and_position(
                info,
                position,
                self.cluster_map[info.cluster as usize].1 as usize,
                font,
            )
        };

        let first_cluster = infos[0].cluster as usize;
        let last_cluster = infos.last().unwrap().cluster as usize;
        let end_cluster = if first_cluster == cluster_range.start {
            cluster_range.end
        } else {
            cluster_range.start
        };

        let mut glyph_buffer_last = self.glyph_buffer.len();
        let mut successful_ranges = Vec::new();
        let mut it = std::iter::zip(infos, positions);
        while let Some((info, position)) = it.next() {
            if info.codepoint == 0 {
                continue;
            };

            let mut len = 1;
            self.glyph_buffer.push(make_glyph(info, position));
            let cluster_subrange_start = info.cluster as usize;
            let cluster_subrange_end = loop {
                match it.next() {
                    Some((info, position)) if info.codepoint != 0 => {
                        self.glyph_buffer.push(make_glyph(info, position));
                        len += 1;
                    }
                    Some((info, _)) => break info.cluster as usize,
                    None => break end_cluster,
                }
            };

            successful_ranges.push((cluster_subrange_start..cluster_subrange_end, len));
        }

        let mut broken_subrange_start = first_cluster;
        for (cluster_subrange, len) in successful_ranges {
            if broken_subrange_start != cluster_subrange.start {
                self.retry_shaping(
                    fixup_range(broken_subrange_start, cluster_subrange.start),
                    font_iterator.clone(),
                    force_tofu,
                )?;
            }
            broken_subrange_start = cluster_subrange.end;

            self.result
                .extend_from_slice(&self.glyph_buffer[glyph_buffer_last..glyph_buffer_last + len]);
            glyph_buffer_last += len;
        }

        if broken_subrange_start != end_cluster {
            // This means the font fallback system lied to us and gave us
            // a font that does not, in fact, have the character we asked for.
            // Or the tofu font failed to shape any characters but that shouldn't
            // happen, if it does anyway it will just incur an additional shaping pass.
            let next_force_tofu =
                broken_subrange_start == first_cluster && font_iterator.did_system_fallback();

            let left = broken_subrange_start;
            let right = last_cluster;
            self.retry_shaping(
                if left > right {
                    right..left + 1
                } else {
                    left..right + 1
                },
                font_iterator.clone(),
                next_force_tofu,
            )?
        }

        Ok(())
    }
}

impl Drop for ShapingPass<'_, '_, '_> {
    fn drop(&mut self) {
        *self.glyph_buffer_parts = {
            let (ptr, _, cap) = vec_parts(&mut *self.glyph_buffer);
            (ptr.cast(), cap)
        };
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
