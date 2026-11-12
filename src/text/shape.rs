use std::{
    collections::HashMap,
    mem::{ManuallyDrop, MaybeUninit},
    ops::Range,
};

use log::LogContext;
use text_sys::*;
use thiserror::Error;
use util::{vec_into_parts, vec_parts};

use crate::text::{Font, OpenTypeTag};

use super::{Direction, FontDb, FontMatchIterator, Glyph};

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

    fn guess_properties(&mut self) -> hb_segment_properties_t {
        unsafe {
            let mut buf = MaybeUninit::uninit();
            hb_buffer_guess_segment_properties(self.0);
            hb_buffer_get_segment_properties(self.0, buf.as_mut_ptr());
            buf.assume_init()
        }
    }

    fn set_segment_properties(&mut self, properties: &hb_segment_properties_t) {
        unsafe { hb_buffer_set_segment_properties(self.0, properties) };
    }

    fn set_pre_context(&mut self, context: &str) {
        unsafe {
            hb_buffer_add_utf8(
                self.0,
                context.as_ptr() as *const _,
                context.len() as i32,
                context.len() as u32,
                0,
            );
        }
    }

    fn set_post_context(&mut self, context: &str) {
        unsafe {
            hb_buffer_add_utf8(
                self.0,
                context.as_ptr() as *const _,
                context.len() as i32,
                0,
                0,
            );
        }
    }

    fn add_codepoint(&mut self, codepoint: char, cluster: u32) {
        unsafe { hb_buffer_add(self.0, codepoint as u32, cluster) };
    }

    fn reverse(&mut self) {
        unsafe { hb_buffer_reverse(self.0) };
    }

    fn items(&self) -> (&[hb_glyph_info_t], &[hb_glyph_position_t]) {
        let infos: &[hb_glyph_info_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_infos(self.0, &mut nglyphs);
            if infos.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(infos as *const _, nglyphs as usize)
            }
        };

        let positions: &[hb_glyph_position_t] = unsafe {
            let mut nglyphs = 0;
            let infos = hb_buffer_get_glyph_positions(self.0, &mut nglyphs);
            if infos.is_null() {
                &[]
            } else {
                std::slice::from_raw_parts(infos as *const _, nglyphs as usize)
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

impl ClusterEntry {
    fn end_utf8_index(&self) -> usize {
        self.utf8_index + self.codepoint.len_utf8()
    }
}

#[derive(Debug, Clone, Copy)]
struct FeatureState {
    start_cluster: u32,
    value: u32,
}

impl FeatureState {
    fn flush(self, tag: OpenTypeTag, end_cluster: u32) -> hb_feature_t {
        hb_feature_t {
            tag: tag.0,
            value: self.value,
            start: self.start_cluster,
            end: end_cluster,
        }
    }
}

pub struct ShapingBuffer {
    active_features: HashMap<OpenTypeTag, FeatureState>,
    raw: RawShapingBuffer,
    features: Vec<hb_feature_t>,
    cluster_map: Vec<ClusterEntry>,
    glyph_scratch_buffer_parts: (*mut Glyph, usize),
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
            active_features: HashMap::new(),
            raw: RawShapingBuffer::new(),
            features: Vec::new(),
            cluster_map: Vec::new(),
            glyph_scratch_buffer_parts: {
                let (ptr, _, cap) = vec_into_parts(Vec::new());
                (ptr, cap)
            },
        }
    }

    fn add_codepoint(&mut self, codepoint: char, utf8_index: usize, is_grapheme_start: bool) {
        self.raw
            .add_codepoint(codepoint, self.cluster_map.len() as u32);
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

    pub fn set_feature(&mut self, tag: OpenTypeTag, value: u32) {
        let new_state = FeatureState {
            start_cluster: self.cluster_map.len() as u32,
            value,
        };

        match self.active_features.entry(tag) {
            std::collections::hash_map::Entry::Occupied(mut occupied) => {
                self.features
                    .push(occupied.get().flush(tag, new_state.start_cluster));
                *occupied.get_mut() = new_state;
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(new_state);
            }
        }
    }

    pub fn reset_features(&mut self) {
        for (tag, state) in self.active_features.drain() {
            if state.start_cluster != self.cluster_map.len() as u32 {
                self.features
                    .push(state.flush(tag, self.cluster_map.len() as u32))
            }
        }
    }

    pub fn set_direction(&mut self, direction: Direction) {
        unsafe {
            hb_buffer_set_direction(self.raw.0, direction as hb_direction_t);
        }
    }

    pub fn clear(&mut self) {
        self.active_features.clear();
        self.raw.clear();
        self.features.clear();
        self.cluster_map.clear();
    }
}

pub trait ShapingSink {
    fn append(&mut self, text_range: Range<usize>, font: &Font, glyphs: &[Glyph]);
}

impl ShapingBuffer {
    pub fn shape(
        &mut self,
        log: &LogContext,
        // Used for supplying context to HarfBuzz.
        // Clusters in the cluster map are assumed to be UTF-8 indicies into this string
        // and context is always computed by taking open-ended subslices of this string.
        text: &str,
        output: &mut dyn ShapingSink,
        font_iterator: FontMatchIterator<'_>,
        fonts: &mut FontDb,
    ) -> Result<(), ShapingError> {
        self.reset_features();

        ShapingPass {
            log,
            properties: self.raw.guess_properties(),
            buffer: RawShapingBuffer(self.raw.0),
            glyph_buffer: unsafe {
                ManuallyDrop::new(Vec::from_raw_parts(
                    self.glyph_scratch_buffer_parts.0.cast(),
                    0,
                    self.glyph_scratch_buffer_parts.1,
                ))
            },
            glyph_buffer_parts: &mut self.glyph_scratch_buffer_parts,
            output,
            cluster_map: &self.cluster_map,
            text,
            fonts,
            features: &self.features,
        }
        .shape_range(0..self.cluster_map.len(), font_iterator, false)?;

        self.clear();

        Ok(())
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

struct ShapingPass<'p> {
    log: &'p LogContext<'p>,
    buffer: RawShapingBuffer,
    glyph_buffer: ManuallyDrop<Vec<Glyph>>,
    glyph_buffer_parts: &'p mut (*mut Glyph, usize),
    output: &'p mut dyn ShapingSink,
    cluster_map: &'p [ClusterEntry],
    text: &'p str,
    fonts: &'p mut FontDb,
    properties: hb_segment_properties_t,
    features: &'p [hb_feature_t],
}

impl ShapingPass<'_> {
    fn shape_range(
        &mut self,
        cluster_range: Range<usize>,
        mut font_iterator: FontMatchIterator<'_>,
        force_tofu: bool,
    ) -> Result<(), ShapingError> {
        let cluster_map_slice = &self.cluster_map[cluster_range.clone()];
        let (Some(first_cluster), Some(last_cluster)) =
            (cluster_map_slice.first(), cluster_map_slice.last())
        else {
            return Ok(());
        };
        assert!(
            first_cluster.is_grapheme_start,
            "Attempted to shape range that starts in the middle of a grapheme"
        );

        self.buffer.clear();
        self.buffer.set_segment_properties(&self.properties);

        self.buffer
            .set_pre_context(&self.text[..first_cluster.utf8_index]);
        for (entry, i) in std::iter::zip(cluster_map_slice, cluster_range.clone()) {
            self.buffer.add_codepoint(entry.codepoint, i as u32);
        }
        self.buffer
            .set_post_context(&self.text[last_cluster.end_utf8_index()..]);

        let font = if force_tofu {
            font_iterator.matcher().tofu()
        } else {
            font_iterator
                .next_with_fallback(self.log, first_cluster.codepoint as u32, self.fonts)?
                .unwrap_or_else(|| font_iterator.matcher().tofu())
        };
        let hb_font = font.as_harfbuzz_font()?;

        unsafe {
            hb_shape(
                hb_font,
                self.buffer.0,
                self.features.as_ptr(),
                self.features.len() as u32,
            )
        };

        let is_reverse = Direction::try_from_hb(self.properties.direction)
            .unwrap()
            .is_reverse();
        if is_reverse {
            self.buffer.reverse();
        }

        let (infos, positions) = self.buffer.items();
        if infos.is_empty() {
            return Ok(());
        }

        let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t| {
            let cluster = &self.cluster_map[info.cluster as usize];
            Glyph::from_info_and_position(info, position, cluster.utf8_index, &font)
        };

        let is_cluster_initial =
            |cluster: u32| self.cluster_map[cluster as usize].is_grapheme_start;

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
            let mut pending_glyphs = 1;
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
                        cluster_subrange_end = cluster_range.end;
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

        let mut broken_subrange_start = cluster_range.start;
        {
            let text_utf8_end = self.cluster_map.get(cluster_range.end).map_or_else(
                || self.cluster_map.last().unwrap().end_utf8_index(),
                |x| x.utf8_index,
            );
            let cluster_range_to_utf8 = |range: Range<usize>| {
                self.cluster_map[range.start].utf8_index
                    ..self
                        .cluster_map
                        .get(range.end)
                        .map_or(text_utf8_end, |c| c.utf8_index)
            };
            let truncate_to = glyph_buffer_last;

            for (cluster_subrange_start, cluster_subrange_end, len) in successful_ranges {
                if broken_subrange_start != cluster_subrange_start {
                    self.shape_range(
                        broken_subrange_start..cluster_subrange_start,
                        font_iterator.clone(),
                        force_tofu,
                    )?;
                }
                broken_subrange_start = cluster_subrange_end;

                let text_range =
                    cluster_range_to_utf8(cluster_subrange_start..cluster_subrange_end);
                self.output.append(
                    text_range,
                    &font,
                    &self.glyph_buffer[glyph_buffer_last..glyph_buffer_last + len],
                );
                glyph_buffer_last += len;
            }

            self.glyph_buffer.truncate(truncate_to);
        }

        if broken_subrange_start != cluster_range.end {
            assert!(!force_tofu, "Tofu font failed to shape any characters");

            // This means the font fallback system lied to us and gave us
            // a font that does not, in fact, have the character we asked for.
            let next_force_tofu =
                broken_subrange_start == cluster_range.start && font_iterator.did_system_fallback();

            let range = broken_subrange_start..cluster_range.end;
            self.shape_range(range, font_iterator.clone(), next_force_tofu)?
        }

        Ok(())
    }
}

impl Drop for ShapingPass<'_> {
    fn drop(&mut self) {
        *self.glyph_buffer_parts = {
            let (ptr, _, cap) = vec_parts(&mut *self.glyph_buffer);
            (ptr.cast(), cap)
        };
    }
}
