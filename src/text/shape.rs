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
                0,
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
        output: &mut dyn ShapingSink,
        font_iterator: FontMatchIterator<'_>,
        fonts: &mut FontDb,
    ) -> Result<(), ShapingError> {
        self.reset_features();

        let properties = unsafe {
            let mut buf = MaybeUninit::uninit();
            hb_buffer_guess_segment_properties(self.raw.0);
            hb_buffer_get_segment_properties(self.raw.0, buf.as_mut_ptr());
            buf.assume_init()
        };

        ShapingPass {
            log,
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
            fonts,
            properties,
            features: &self.features,
        }
        .shape_layer(0..self.cluster_map.len(), font_iterator, false)?;

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
    fonts: &'p mut FontDb,
    properties: hb_segment_properties_t,
    features: &'p [hb_feature_t],
}

impl ShapingPass<'_> {
    fn retry_shaping(
        &mut self,
        range: Range<usize>,
        font_iterator: FontMatchIterator<'_>,
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
        mut font_iterator: FontMatchIterator<'_>,
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
            font_iterator.matcher().tofu()
        } else {
            font_iterator
                .next_with_fallback(self.log, first_codepoint as u32, self.fonts)?
                .unwrap_or_else(|| font_iterator.matcher().tofu())
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

        type ItemIter<'a> = std::iter::Zip<
            std::slice::Iter<'a, hb_glyph_info_t>,
            std::slice::Iter<'a, hb_glyph_position_t>,
        >;

        let (infos, positions) = self.buffer.items();
        if infos.is_empty() {
            return Ok(());
        }

        let first_cluster = infos[0].cluster as usize;
        let is_reverse = first_cluster != cluster_range.start;
        let is_dir_reverse =
            Direction::try_from_hb(self.properties.direction).is_some_and(|d| d.is_reverse());
        // NOTE: `start_cluster` isn't always equal to `first_cluster`!
        //       This is because `first_cluster` is the first cluster that *emitted a glyph*
        //       while `start_cluster` is the directionally-first cluster in this `cluster_range`.
        let start_cluster = if !is_reverse {
            cluster_range.start
        } else {
            cluster_range.end - 1
        };

        let make_glyph = |info: &hb_glyph_info_t, position: &hb_glyph_position_t, it: &ItemIter| {
            let cluster = &self.cluster_map[info.cluster as usize];
            Glyph::from_info_and_position(
                info,
                position,
                if !is_dir_reverse {
                    cluster.utf8_index
                } else {
                    it.clone()
                        .find(|x| x.0.cluster != info.cluster)
                        .map_or_else(
                            || self.cluster_map[cluster_range.start].utf8_index,
                            |c| self.cluster_map[c.0.cluster as usize + 1].utf8_index,
                        )
                },
                &font,
            )
        };

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
        // These ranges are [min, max) if !is_reverse and otherwise they are [max, min].
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
            self.glyph_buffer.push(make_glyph(info, position, &it));
            let cluster_subrange_start = info.cluster as usize;
            let mut cluster_subrange_end = cluster_subrange_start;
            loop {
                match it.next() {
                    Some((info, position)) => {
                        if is_cluster_initial(info.cluster) {
                            pending_glyphs = 0;
                            cluster_subrange_end =
                                (info.cluster as usize).wrapping_add(usize::from(is_reverse));
                        }

                        if info.codepoint != 0 {
                            self.glyph_buffer.push(make_glyph(info, position, &it));
                            len += 1;
                            pending_glyphs += 1;
                        } else {
                            break;
                        }
                    }
                    None => {
                        pending_glyphs = 0;
                        cluster_subrange_end = if !is_reverse {
                            cluster_range.end
                        } else {
                            cluster_range.start
                        };
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
        if !is_reverse {
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
                    self.retry_shaping(
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
                    &font.clone(),
                    &self.glyph_buffer[glyph_buffer_last..glyph_buffer_last + len],
                );
                glyph_buffer_last += len;
            }

            self.glyph_buffer.truncate(truncate_to);
        } else {
            let cluster_range_to_utf8 = |start: usize, end: usize| {
                self.cluster_map[end].utf8_index..self.cluster_map[start].end_utf8_index()
            };

            for (cluster_subrange_start, cluster_subrange_end, len) in
                successful_ranges.into_iter().rev()
            {
                if broken_subrange_start != cluster_subrange_end {
                    self.retry_shaping(
                        broken_subrange_start..cluster_subrange_end,
                        font_iterator.clone(),
                        force_tofu,
                    )?;
                }
                broken_subrange_start = cluster_subrange_start + 1;

                let text_range =
                    cluster_range_to_utf8(cluster_subrange_start, cluster_subrange_end);
                let glyphs_start = self.glyph_buffer.len() - len;
                let glyphs = &mut self.glyph_buffer[glyphs_start..];
                glyphs.reverse();
                self.output.append(text_range, &font.clone(), glyphs);
                self.glyph_buffer.truncate(glyphs_start);
            }
        };

        if broken_subrange_start != cluster_range.end {
            assert!(!force_tofu, "Tofu font failed to shape any characters");

            // This means the font fallback system lied to us and gave us
            // a font that does not, in fact, have the character we asked for.
            let next_force_tofu =
                broken_subrange_start == start_cluster && font_iterator.did_system_fallback();

            let range = broken_subrange_start..cluster_range.end;
            self.retry_shaping(range, font_iterator.clone(), next_force_tofu)?
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
