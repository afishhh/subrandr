use std::{
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
};

use text_sys::*;
use thiserror::Error;
use util::{vec_into_parts, vec_parts};

use crate::text::OpenTypeTag;

use super::{Direction, FontArena, FontDb, FontMatchIterator, Glyph};

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
            hb_buffer_set_cluster_level(self.0, HB_BUFFER_CLUSTER_LEVEL_MONOTONE_CHARACTERS);
            hb_buffer_set_flags(self.0, HB_BUFFER_FLAG_PRODUCE_UNSAFE_TO_CONCAT);
            hb_buffer_set_content_type(self.0, HB_BUFFER_CONTENT_TYPE_UNICODE);
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

#[derive(Debug, Clone)]
pub struct ClusterEntry {
    codepoint: char,
    utf8_index: usize,
    is_grapheme_start: bool,
}

pub struct ShapingBuffer {
    raw: RawShapingBuffer,
    cluster_map: Vec<ClusterEntry>,
    glyph_scratch_buffer_parts: (*mut Glyph<'static>, usize),
    features: Vec<hb_feature_t>,
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
            cluster_map: Vec::new(),
            glyph_scratch_buffer_parts: {
                let (ptr, _, cap) = vec_into_parts(Vec::new());
                (ptr, cap)
            },
            features: Vec::new(),
        }
    }

    pub fn add_text_simple(&mut self, text: &str) {
        self.set_pre_context("");

        let mut last = 0;
        for next in icu_segmenter::GraphemeClusterSegmenter::new().segment_str(text) {
            self.add_grapheme(&text[last..next], last);
            last = next;
        }

        self.set_post_context("");
    }

    pub fn set_pre_context(&mut self, context: &str) {
        unsafe {
            hb_buffer_add_utf8(
                self.raw.0,
                context.as_ptr() as *const _,
                context.len() as i32,
                context.len() as u32,
                0,
            );
        }
    }

    pub fn set_post_context(&mut self, context: &str) {
        unsafe {
            hb_buffer_add_utf8(
                self.raw.0,
                context.as_ptr() as *const _,
                context.len() as i32,
                context.len() as u32,
                0,
            );
        }
    }

    fn add_codepoint(&mut self, codepoint: char, utf8_index: usize, is_grapheme_start: bool) {
        unsafe { hb_buffer_add(self.raw.0, codepoint as u32, self.cluster_map.len() as u32) };
        self.cluster_map.push(ClusterEntry {
            codepoint,
            utf8_index,
            is_grapheme_start,
        });
    }

    pub fn add_grapheme(&mut self, grapheme: &str, mut cluster: usize) {
        debug_assert!(!grapheme.is_empty());

        let mut is_grapheme_start = true;
        for chr in grapheme.chars() {
            self.add_codepoint(chr, cluster, is_grapheme_start);
            cluster += chr.len_utf8();
            is_grapheme_start = false;
        }
    }

    pub fn set_feature(&mut self, tag: OpenTypeTag, value: u32, range: Range<usize>) {
        self.features.push(hb_feature_t {
            tag: tag.0,
            value,
            start: range.start as u32,
            end: range.end as u32,
        });
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
        self.features.clear();
        self.cluster_map.clear();
    }

    pub fn shape<'f>(
        &mut self,
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Vec<Glyph<'f>>, ShapingError> {
        for feature in self.features.iter_mut() {
            feature.start = match self
                .cluster_map
                .binary_search_by_key(&feature.start, |x| x.utf8_index as u32)
            {
                Ok(i) => i,
                Err(i) => i.saturating_sub(1),
            } as u32;
            feature.end = match self
                .cluster_map
                .binary_search_by_key(&feature.end, |x| x.utf8_index as u32)
            {
                Ok(i) | Err(i) => i,
            } as u32;
        }

        let properties = unsafe {
            let mut buf = MaybeUninit::uninit();
            hb_buffer_guess_segment_properties(self.raw.0);
            hb_buffer_get_segment_properties(self.raw.0, buf.as_mut_ptr());
            buf.assume_init()
        };

        let mut result = Vec::with_capacity(self.cluster_map.len());
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
            cluster_map: &self.cluster_map,
            font_arena,
            fonts,
            properties,
            features: &self.features,
        }
        .shape_layer(0..self.cluster_map.len(), font_iterator, false)?;

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
    cluster_map: &'p [ClusterEntry],
    font_arena: &'f FontArena,
    fonts: &'p mut FontDb<'a>,
    properties: hb_segment_properties_t,
    features: &'p [hb_feature_t],
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
        }

        for (entry, i) in std::iter::zip(&self.cluster_map[range.clone()], range.clone()) {
            unsafe {
                hb_buffer_add(self.buffer.0, entry.codepoint as u32, i as u32);
            }
        }

        self.shape_layer(range, font_iterator, force_tofu)?;

        Ok(())
    }

    fn shape_layer(
        &mut self,
        cluster_range: Range<usize>,
        mut font_iterator: FontMatchIterator<'_, 'f>,
        force_tofu: bool,
    ) -> Result<(), ShapingError> {
        let Some(&ClusterEntry {
            codepoint: first_codepoint,
            is_grapheme_start,
            ..
        }) = self.cluster_map.get(cluster_range.start)
        else {
            return Ok(());
        };
        assert!(
            is_grapheme_start,
            "Attempted to reshape substring that starts in the middle of a grapheme"
        );

        let font = if force_tofu {
            font_iterator.matcher().tofu(self.font_arena)
        } else {
            font_iterator
                .next_with_fallback(first_codepoint as u32, self.font_arena, self.fonts)?
                .unwrap_or_else(|| font_iterator.matcher().tofu(self.font_arena))
        };
        let hb_font = font.as_harfbuzz_font()?;

        unsafe {
            hb_shape(
                hb_font,
                self.buffer.0,
                self.features.as_ptr(),
                self.features.len() as u32,
            );
        }
        let (infos, positions) = self.buffer.items_mut();

        if infos.is_empty() {
            return Ok(());
        }

        let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t| {
            Glyph::from_info_and_position(
                info,
                position,
                self.cluster_map[info.cluster as usize].utf8_index,
                font,
            )
        };

        let first_cluster = infos[0].cluster as usize;
        let is_reverse = first_cluster != cluster_range.start;
        // NOTE: `start_cluster` isn't always equal to `first_cluster`!
        //       This is because `first_cluster` is the first cluster that *emitted a glyph*
        //       while `start_cluster` is the directionally-first cluster in this `cluster_range`.
        let start_cluster = if !is_reverse {
            cluster_range.start
        } else {
            cluster_range.end - 1
        };
        // It is, in general, not possible to compute an excluded end bound for clusters here.
        // This is because when working with RTL text such an end bound could be `-1`[1].
        // So we use `usize::MAX` as a sentinel instead and are careful not to
        // compare clusters with non-equality comparisons.
        //
        // [1] Although this placeholder is effectively `-1` :)
        const END_CLUSTER_EXCLUDED: usize = usize::MAX;

        let is_cluster_initial = |cluster: u32| {
            // If `is_reverse` is true then HarfBuzz gave us final-cluster values in reverse order
            // so if the cluster after this one is a grapheme start that must mean this cluster
            // is a grapheme end hence it is an initial cluster in this direction.
            // Otherwise, if `is_reverse` is false, we just check whether this cluster is
            // a grapheme start since we're operating on first-cluster values.
            self.cluster_map
                .get(cluster as usize + usize::from(is_reverse))
                .is_none_or(|c| c.is_grapheme_start)
        };

        let mut glyph_buffer_last = self.glyph_buffer.len();
        let mut successful_ranges = Vec::new();
        let mut it = std::iter::zip(infos, positions);
        while let Some((info, position)) = it.next() {
            if info.codepoint == 0 {
                continue;
            };

            // If the first valid cluster does not start a grapheme, then we have
            // a partially successful grapheme here which we also need to skip.
            //
            // NOTE: There is an edge case here where if a font ligates
            //       part of a grapheme with another grapheme we will never
            //       be able to shape even that other grapheme with this font.
            //       An anologous issue exists for the end trimming.
            //       However this edge case seems pretty improbable/non-sensical
            //       in real-world conditions so it's probably fine?
            if !is_cluster_initial(info.cluster) {
                continue;
            }

            let mut len = 1;
            // Number of glyphs that haven't yet been "flushed" by a grapheme start.
            // If we don't see a grapheme start at the end of the valid subrange then we
            // will roll back `len` by this value to get rid of the partial ending.
            let mut pending_glyphs = 0;
            self.glyph_buffer.push(make_glyph(info, position));
            let cluster_subrange_start = info.cluster as usize;
            let mut cluster_subrange_end = cluster_subrange_start;
            loop {
                match it.next() {
                    Some((info, position)) => {
                        if is_cluster_initial(info.cluster) {
                            pending_glyphs = 0;
                            cluster_subrange_end = info.cluster as usize;
                        }

                        if info.codepoint != 0 {
                            self.glyph_buffer.push(make_glyph(info, position));
                            len += 1;
                            pending_glyphs += 1;
                        } else {
                            break;
                        }
                    }
                    None => {
                        pending_glyphs = 0;
                        cluster_subrange_end = END_CLUSTER_EXCLUDED;
                        break;
                    }
                }
            }

            if pending_glyphs > 0 {
                let new_glyph_buffer_len = self.glyph_buffer.len() - pending_glyphs;
                self.glyph_buffer.truncate(new_glyph_buffer_len);
                len -= pending_glyphs;
            }

            if len == 0 {
                continue;
            }

            successful_ranges.push((cluster_subrange_start, cluster_subrange_end, len));
        }

        let mut broken_subrange_start = first_cluster;
        for (cluster_subrange_start, cluster_subrange_end, len) in successful_ranges {
            if broken_subrange_start != cluster_subrange_start {
                self.retry_shaping(
                    fixup_range(broken_subrange_start, cluster_subrange_start),
                    font_iterator.clone(),
                    force_tofu,
                )?;
            }
            broken_subrange_start = cluster_subrange_end;

            self.result
                .extend_from_slice(&self.glyph_buffer[glyph_buffer_last..glyph_buffer_last + len]);
            glyph_buffer_last += len;
        }

        if broken_subrange_start != END_CLUSTER_EXCLUDED {
            assert!(!force_tofu, "Tofu font failed to shape any characters");

            // This means the font fallback system lied to us and gave us
            // a font that does not, in fact, have the character we asked for.
            let next_force_tofu =
                broken_subrange_start == start_cluster && font_iterator.did_system_fallback();

            let range = if is_reverse {
                cluster_range.start..broken_subrange_start + 1
            } else {
                broken_subrange_start..cluster_range.end
            };
            self.retry_shaping(range, font_iterator.clone(), next_force_tofu)?
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
    buffer.add_text_simple(text);
    buffer.shape(font_iterator, font_arena, fonts)
}
