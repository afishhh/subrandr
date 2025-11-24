use std::{
    collections::LinkedList,
    fmt::Debug,
    ops::{Add as _, Deref, Range},
    rc::Rc,
};

use util::{math::I26Dot6, rev_if::RevIf};

use super::FontFeatureEvent;
use crate::text::{
    Direction, FontArena, FontDb, FontMatchIterator, Glyph, ShapingBuffer, ShapingError,
};

#[derive(Clone)]
pub struct GlyphString<'f> {
    /// Always refers to the original string that contains the whole context
    /// of this string.
    text: Rc<str>,
    segments: LinkedList<GlyphStringSegment<'f>>,
    direction: Direction,
}

impl Debug for GlyphString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GlyphString(direction: {:?}) ", self.direction)?;
        let mut list = f.debug_list();
        for segment in &self.segments {
            list.entry(&util::fmt_from_fn(|f| {
                f.debug_struct("GlyphStringSegment")
                    .field("text", &&self.text[segment.text_range.clone()])
                    .field(
                        "glyphs",
                        &util::fmt_from_fn(|f| f.debug_list().finish_non_exhaustive()),
                    )
                    .finish()
            }));
        }
        list.finish()
    }
}

#[derive(Debug, Clone)]
struct GlyphStringSegment<'f> {
    /// Some glyph slice which resulted from shaping the entirety or some subslice
    /// of the text in `text`. [`Glyph::cluster`] will be a valid index into `text`.
    storage: Rc<[Glyph<'f>]>,
    /// The subslice of `storage` this segment actually represents.
    glyph_range: Range<usize>,
    /// The subslice of [`GlyphString::text`] this segment was shaped from.
    text_range: Range<usize>,
}

impl<'f> Deref for GlyphStringSegment<'f> {
    type Target = [Glyph<'f>];

    fn deref(&self) -> &Self::Target {
        &self.storage[self.glyph_range.clone()]
    }
}

impl<'s, 'f> IntoIterator for &'s GlyphStringSegment<'f> {
    type Item = &'s Glyph<'f>;
    type IntoIter = std::slice::Iter<'s, Glyph<'f>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'f> GlyphStringSegment<'f> {
    fn from_glyphs(text_range: Range<usize>, glyphs: Vec<Glyph<'f>>) -> Self {
        Self {
            glyph_range: 0..glyphs.len(),
            text_range,
            storage: glyphs.into(),
        }
    }

    fn subslice(&self, range: Range<usize>, direction: Direction) -> Self {
        assert!(
            range.start < range.end,
            "Invalid glyph string segment subslice ({} > {})",
            range.start,
            range.end
        );

        Self {
            storage: self.storage.clone(),
            glyph_range: self.glyph_range.start + range.start..self.glyph_range.start + range.end,
            text_range: {
                if !direction.is_reverse() {
                    self[range.start].cluster
                        ..self
                            .get(range.end)
                            .map_or(self.text_range.end, |g| g.cluster)
                } else {
                    self.get(range.end)
                        .map_or(self.text_range.start, |g| g.cluster + 1)
                        ..self[range.start].cluster + 1
                }
            },
        }
    }

    // [`Self::subslice`] does not allow creating an empty subslice so that "no empty segments"
    // can be an invariant upheld by [`GlyphString`].
    // This function allows for empty subslices but returns them as an empty [`LinkedList`].
    fn subslice_into_list(&self, range: Range<usize>, direction: Direction) -> LinkedList<Self> {
        if range.start == range.end {
            return LinkedList::new();
        }

        LinkedList::from([self.subslice(range, direction)])
    }

    fn split_off_visual_start(&mut self, pivot: usize, direction: Direction) -> Self {
        let result = self.subslice(0..pivot, direction);
        self.glyph_range.start += pivot;
        if !direction.is_reverse() {
            self.text_range.start = result.text_range.end;
        } else {
            self.text_range.end = result.text_range.start;
        }
        result
    }

    fn first_byte_of_glyph(&self, index: usize, direction: Direction) -> usize {
        self.get(index + usize::from(direction.is_reverse()))
            .map_or(self.text_range.start, |g| {
                g.cluster + usize::from(direction.is_reverse())
            })
    }

    fn iter_indices_half_exclusive(
        &self,
        pivot: usize,
        forward: bool,
    ) -> impl Iterator<Item = usize> {
        let mut i;
        let end;
        let step: isize;
        if !forward {
            end = 0;
            i = pivot.saturating_sub(1);
            step = -1;
        } else {
            end = self.len() - 1;
            i = (pivot + 1).min(end);
            step = 1;
        };

        std::iter::from_fn(move || {
            if i == end {
                None
            } else {
                let value = i;
                i = i.wrapping_add_signed(step);
                Some(value)
            }
        })
    }

    fn try_concat_with_half(
        &self,
        pivot: usize,
        other: Self,
        direction: Direction,
        forward: bool,
    ) -> Option<LinkedList<Self>> {
        match direction.is_reverse() ^ forward {
            false if other.first().is_none_or(|first| !first.unsafe_to_concat()) => {
                Some(LinkedList::from([
                    self.subslice(0..pivot + 1, direction),
                    other,
                ]))
            }
            true if other.last().is_none_or(|last| !last.unsafe_to_concat()) => {
                Some(LinkedList::from([
                    other,
                    self.subslice(pivot..self.len(), direction),
                ]))
            }
            _ => None,
        }
    }

    fn break_until(
        &self,
        text: Rc<str>,
        glyph_index: usize,
        break_index: usize,
        buffer: &mut ShapingBuffer,
        font_feature_events: &[FontFeatureEvent],
        grapheme_cluster_boundaries: &[usize],
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
        direction: Direction,
    ) -> Result<LinkedList<Self>, ShapingError> {
        // If the break is within a glyph (like a long ligature), we must
        // use the slow reshaping path.
        let can_reuse_split_glyph = self.first_byte_of_glyph(glyph_index, direction) == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            // Easy case, we can just split the glyph string right here
            if !direction.is_reverse() {
                return Ok(self.subslice_into_list(0..glyph_index, direction));
            } else {
                return Ok(self.subslice_into_list(glyph_index + 1..self.len(), direction));
            }
        } else if self.len() > 1 {
            // The hard case, we have to find the closest glyph on the left which
            // has the UNSAFE_TO_CONCAT flag unset, then try reshaping after such glyphs
            // until the first glyph of the result also has the UNSAFE_TO_CONCAT flag unset.
            for i in self.iter_indices_half_exclusive(glyph_index, direction.is_reverse()) {
                if !self[i].unsafe_to_concat() {
                    let reshape_cluster = self[i + usize::from(!direction.is_reverse())].cluster
                        + usize::from(direction.is_reverse());
                    let reshape_range = reshape_cluster..break_index;

                    buffer.clear();
                    buffer.set_direction(direction);
                    super::set_buffer_content_from_range(
                        buffer,
                        text.as_ref(),
                        reshape_range.clone(),
                        font_feature_events,
                        grapheme_cluster_boundaries,
                    );
                    let other = GlyphStringSegment::from_glyphs(
                        reshape_range,
                        buffer.shape(font_iterator.clone(), font_arena, fonts)?,
                    );

                    if let Some(result) = self.try_concat_with_half(i, other, direction, false) {
                        return Ok(result);
                    }
                }
            }
        }

        // We have to reshape the whole segment, there's no place where we can safely concat.
        let reshape_range = self.text_range.start..break_index;
        buffer.clear();
        buffer.set_direction(direction);
        super::set_buffer_content_from_range(
            buffer,
            text.as_ref(),
            reshape_range.clone(),
            font_feature_events,
            grapheme_cluster_boundaries,
        );
        Ok(LinkedList::from([GlyphStringSegment::from_glyphs(
            reshape_range,
            buffer.shape(font_iterator, font_arena, fonts)?,
        )]))
    }

    // Analogous to `break_after` but returns the part logically after `break_index` (inclusive).
    fn break_after(
        &self,
        text: Rc<str>,
        glyph_index: usize,
        break_index: usize,
        buffer: &mut ShapingBuffer,
        font_feature_events: &[FontFeatureEvent],
        grapheme_cluster_boundaries: &[usize],
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
        direction: Direction,
    ) -> Result<LinkedList<GlyphStringSegment<'f>>, ShapingError> {
        let can_reuse_split_glyph = self.first_byte_of_glyph(glyph_index, direction) == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            if !direction.is_reverse() {
                return Ok(self.subslice_into_list(glyph_index..self.len(), direction));
            } else {
                return Ok(self.subslice_into_list(0..glyph_index + 1, direction));
            }
        } else {
            for i in self.iter_indices_half_exclusive(glyph_index, !direction.is_reverse()) {
                if !self[i].unsafe_to_concat() {
                    let reshape_cluster = self[i + usize::from(direction.is_reverse())].cluster
                        + usize::from(direction.is_reverse());
                    let reshape_range = break_index..reshape_cluster;

                    buffer.clear();
                    buffer.set_direction(direction);
                    super::set_buffer_content_from_range(
                        buffer,
                        text.as_ref(),
                        reshape_range.clone(),
                        font_feature_events,
                        grapheme_cluster_boundaries,
                    );
                    let other = GlyphStringSegment::from_glyphs(
                        reshape_range,
                        buffer.shape(font_iterator.clone(), font_arena, fonts)?,
                    );

                    if let Some(result) = self.try_concat_with_half(i, other, direction, true) {
                        return Ok(result);
                    }
                }
            }
        }

        // We have to reshape the whole segment, there's no place where we can safely concat.
        let reshape_range = break_index..self.text_range.end;
        buffer.clear();
        buffer.set_direction(direction);
        super::set_buffer_content_from_range(
            buffer,
            text.as_ref(),
            reshape_range.clone(),
            font_feature_events,
            grapheme_cluster_boundaries,
        );
        Ok(LinkedList::from([GlyphStringSegment::from_glyphs(
            reshape_range,
            buffer.shape(font_iterator, font_arena, fonts)?,
        )]))
    }

    fn glyph_at_utf8_index(&self, index: usize, direction: Direction) -> Option<usize> {
        if !self.text_range.contains(&index) {
            return None;
        }

        Some(if !direction.is_reverse() {
            self.iter()
                .enumerate()
                .find_map(|(i, g)| match g.cluster.cmp(&index) {
                    std::cmp::Ordering::Equal => Some(i),
                    std::cmp::Ordering::Greater => i.checked_sub(1),
                    std::cmp::Ordering::Less => None,
                })
                .unwrap_or(self.len() - 1)
        } else {
            self.iter()
                .enumerate()
                .find_map(|(i, g)| match g.cluster.cmp(&index) {
                    std::cmp::Ordering::Equal => Some(i),
                    std::cmp::Ordering::Less => i.checked_sub(1),
                    std::cmp::Ordering::Greater => None,
                })
                .unwrap_or(self.len() - 1)
        })
    }
}

// TODO: linked_list_cursors feature would improve some of this code significantly
impl<'f> GlyphString<'f> {
    pub fn from_glyphs(
        text: Rc<str>,
        text_range: Range<usize>,
        glyphs: Vec<Glyph<'f>>,
        direction: Direction,
    ) -> Self {
        assert!(text_range.start <= text_range.end);
        GlyphString::from_array(
            text,
            [GlyphStringSegment::from_glyphs(text_range, glyphs)],
            direction,
        )
    }

    fn from_array<const N: usize>(
        text: Rc<str>,
        segments: [GlyphStringSegment<'f>; N],
        direction: Direction,
    ) -> GlyphString<'f> {
        GlyphString {
            text,
            segments: LinkedList::from_iter(
                segments.into_iter().filter(|segment| !segment.is_empty()),
            ),
            direction,
        }
    }

    pub fn iter_glyphs_visual(&self) -> impl DoubleEndedIterator<Item = &Glyph<'f>> {
        self.segments.iter().flat_map(|s| s.iter())
    }

    pub fn iter_glyphs_logical(&self) -> impl DoubleEndedIterator<Item = &Glyph<'f>> {
        RevIf::new(
            self.segments.iter().flat_map(|s| s.iter()),
            self.direction.is_reverse(),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn text(&self) -> &Rc<str> {
        &self.text
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    pub(super) fn split_off_visual_start(&mut self, end_utf8_index: usize) -> Option<Self> {
        let mut result = LinkedList::new();
        let target_utf8_index = match self.direction.is_reverse() {
            false => end_utf8_index.checked_sub(1)?,
            true => end_utf8_index,
        };

        while let Some(segment) = self.segments.front() {
            // Make sure we're not already past the passed index which can happen
            // due to whitespace collapsing during line-breaking.
            if !self.direction.is_reverse() {
                if segment.text_range.start >= end_utf8_index {
                    break;
                }
            } else if segment.text_range.end < end_utf8_index {
                // TODO: Is this case right?
                break;
            }
            let mut segment = self.segments.pop_front().unwrap();

            match segment.glyph_at_utf8_index(target_utf8_index, self.direction) {
                Some(last_glyph_index) => {
                    result.push_back(
                        segment.split_off_visual_start(last_glyph_index + 1, self.direction),
                    );
                    if !segment.is_empty() {
                        self.segments.push_front(segment);
                    }

                    return Some(Self {
                        text: self.text.clone(),
                        segments: result,
                        direction: self.direction,
                    });
                }
                None => result.push_back(segment),
            }
        }

        if !result.is_empty() {
            Some(GlyphString {
                text: self.text.clone(),
                segments: result,
                direction: self.direction,
            })
        } else {
            None
        }
    }

    // NOTE: Unlike [`Glyph::cluster`] the break range indices here always point to the first byte
    //       of a codepoint so the segment breaking methods must take this into account.
    pub(super) fn break_around(
        &self,
        break_range: Range<usize>,
        max_width: I26Dot6,
        buffer: &mut ShapingBuffer,
        font_feature_events: &[FontFeatureEvent],
        grapheme_cluster_boundaries: &[usize],
        font_iter: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Option<(Self, Self)>, ShapingError> {
        assert!(!self.is_empty());

        let mut left_segments = LinkedList::new();
        let mut current_x = I26Dot6::ZERO;
        let mut it = RevIf::new(self.segments.iter(), self.direction.is_reverse());
        let mut next = it.next();
        let push = |list: &mut LinkedList<GlyphStringSegment<'f>>,
                    segment: GlyphStringSegment<'f>| {
            if self.direction.is_reverse() {
                list.push_front(segment);
            } else {
                list.push_back(segment);
            }
        };
        let append = |list: &mut LinkedList<GlyphStringSegment<'f>>,
                      other: &mut LinkedList<GlyphStringSegment<'f>>| {
            if !self.direction.is_reverse() {
                list.append(other);
            } else {
                other.append(list);
                std::mem::swap(list, other);
            }
        };

        loop {
            let Some(segment) = next else {
                return Ok(None);
            };

            if let Some(left_candidate_glyph) =
                segment.glyph_at_utf8_index(break_range.start, self.direction)
            {
                let mut left_candidate = segment.break_until(
                    self.text.clone(),
                    left_candidate_glyph,
                    break_range.start,
                    buffer,
                    font_feature_events,
                    grapheme_cluster_boundaries,
                    font_iter.clone(),
                    font_arena,
                    fonts,
                    self.direction,
                )?;

                if left_candidate
                    .iter()
                    .flat_map(|s| &s[..])
                    .map(|g| g.x_advance)
                    .fold(current_x, I26Dot6::add)
                    <= max_width
                {
                    append(&mut left_segments, &mut left_candidate);
                    break;
                } else {
                    // This wasn't a valid break because the width ended up being too large.
                    return Ok(None);
                }
            }

            for glyph in segment {
                current_x += glyph.x_advance;
            }
            push(&mut left_segments, segment.clone());

            next = it.next();
        }

        let left = Self {
            text: self.text.clone(),
            segments: left_segments,
            direction: self.direction,
        };
        while let Some(segment) = next {
            if let Some(right_candidate_glyph) =
                segment.glyph_at_utf8_index(break_range.end, self.direction)
            {
                let mut right_segments = segment.break_after(
                    self.text.clone(),
                    right_candidate_glyph,
                    break_range.end,
                    buffer,
                    font_feature_events,
                    grapheme_cluster_boundaries,
                    font_iter,
                    font_arena,
                    fonts,
                    self.direction,
                )?;

                append(
                    &mut right_segments,
                    &mut it.cloned().collect::<LinkedList<_>>(),
                );

                return Ok(Some((
                    left,
                    Self {
                        text: self.text.clone(),
                        segments: right_segments,
                        direction: self.direction,
                    },
                )));
            }

            next = it.next();
        }

        Ok(Some((
            left,
            Self::from_array(self.text.clone(), [], self.direction),
        )))
    }
}
