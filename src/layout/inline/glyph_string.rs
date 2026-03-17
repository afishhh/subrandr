use std::{
    fmt::Debug,
    ops::{Add as _, Deref, Range},
    rc::Rc,
};

use util::{math::I26Dot6, rev_if::RevIf};

use super::RunShaper;
use crate::{
    layout::LayoutContext,
    text::{Direction, Font, FontMatchIterator, Glyph, ShapingError, ShapingSink},
};

#[derive(Clone)]
pub struct GlyphString {
    /// Always refers to the original string that contains the whole context
    /// of this string.
    text: Rc<str>,
    segments: Vec<GlyphStringSegment>,
    direction: Direction,
}

impl Debug for GlyphString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GlyphString(direction: {:?}) ", self.direction)?;
        let mut list = f.debug_list();
        for segment in &self.segments {
            list.entry(&util::fmt_from_fn(|f| {
                f.debug_struct("GlyphStringSegment")
                    .field("text", &&self.text[segment.text_range.clone()])
                    .field("font", &&segment.font)
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
struct GlyphStringSegment {
    /// Some glyph slice which resulted from shaping the entirety or some subslice
    /// of the text in `text`. [`Glyph::cluster`] will be a valid index into `text`.
    storage: Rc<[Glyph]>,
    /// The subslice of `storage` this segment actually represents.
    glyph_range: Range<usize>,
    /// The subslice of [`GlyphString::text`] this segment was shaped from.
    text_range: Range<usize>,
    /// The font this segment was shaped with.
    font: Font,
}

impl Deref for GlyphStringSegment {
    type Target = [Glyph];

    fn deref(&self) -> &Self::Target {
        &self.storage[self.glyph_range.clone()]
    }
}

impl<'s> IntoIterator for &'s GlyphStringSegment {
    type Item = &'s Glyph;
    type IntoIter = std::slice::Iter<'s, Glyph>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl GlyphStringSegment {
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
            font: self.font.clone(),
        }
    }

    // [`Self::subslice`] does not allow creating an empty subslice so that "no empty segments"
    // can be an invariant upheld by [`GlyphString`].
    // This function allows for empty subslices but returns them as an empty [`LinkedList`].
    fn subslice_into_vec(&self, range: Range<usize>, direction: Direction) -> Vec<Self> {
        if range.start == range.end {
            return Vec::new();
        }

        Vec::from([self.subslice(range, direction)])
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

    fn try_concat_with_reshaped_half(
        &self,
        pivot: usize,
        direction: Direction,
        forward: bool,
        shaper: &mut RunShaper,
        font_iterator: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
    ) -> Result<Option<Vec<Self>>, ShapingError> {
        match direction.is_reverse() ^ forward {
            false => {
                let mut other =
                    GlyphStringSegmentSink(vec![self.subslice(0..pivot + 1, direction)]);
                shaper.shape(&mut other, font_iterator, lctx)?;
                if other.0[1..]
                    .first()
                    .and_then(|x| x.first())
                    .is_some_and(|first| first.unsafe_to_concat())
                {
                    return Ok(None);
                }

                Ok(Some(other.0))
            }
            true => {
                let mut other = GlyphStringSegmentSink(Vec::new());
                shaper.shape(&mut other, font_iterator, lctx)?;
                if other
                    .0
                    .last()
                    .and_then(|x| x.last())
                    .is_some_and(|last| last.unsafe_to_concat())
                {
                    return Ok(None);
                }

                other.0.push(self.subslice(pivot..self.len(), direction));
                Ok(Some(other.0))
            }
        }
    }

    fn break_until(
        &self,
        text: Rc<str>,
        glyph_index: usize,
        break_index: usize,
        shaper: &mut RunShaper,
        font_iterator: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
        direction: Direction,
    ) -> Result<Vec<Self>, ShapingError> {
        // If the break is within a glyph (like a long ligature), we must
        // use the slow reshaping path.
        let can_reuse_split_glyph = self.first_byte_of_glyph(glyph_index, direction) == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            // Easy case, we can just split the glyph string right here
            if !direction.is_reverse() {
                return Ok(self.subslice_into_vec(0..glyph_index, direction));
            } else {
                return Ok(self.subslice_into_vec(glyph_index + 1..self.len(), direction));
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

                    shaper.set_buffer_content(&text, reshape_range, direction);
                    if let Some(result) = self.try_concat_with_reshaped_half(
                        i,
                        direction,
                        false,
                        shaper,
                        font_iterator.clone(),
                        lctx,
                    )? {
                        return Ok(result);
                    }
                }
            }
        }

        // We have to reshape the whole segment, there's no place where we can safely concat.
        let reshape_range = self.text_range.start..break_index;
        shaper.set_buffer_content(text.as_ref(), reshape_range, direction);
        let mut result = GlyphStringSegmentSink(Vec::new());
        shaper.shape(&mut result, font_iterator, lctx)?;
        Ok(result.0)
    }

    // Analogous to `break_after` but returns the part logically after `break_index` (inclusive).
    fn break_after(
        &self,
        text: Rc<str>,
        glyph_index: usize,
        break_index: usize,
        shaper: &mut RunShaper,
        font_iterator: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
        direction: Direction,
    ) -> Result<Vec<GlyphStringSegment>, ShapingError> {
        let can_reuse_split_glyph = self.first_byte_of_glyph(glyph_index, direction) == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            if !direction.is_reverse() {
                return Ok(self.subslice_into_vec(glyph_index..self.len(), direction));
            } else {
                return Ok(self.subslice_into_vec(0..glyph_index + 1, direction));
            }
        } else {
            for i in self.iter_indices_half_exclusive(glyph_index, !direction.is_reverse()) {
                if !self[i].unsafe_to_concat() {
                    let reshape_cluster = self[i + usize::from(direction.is_reverse())].cluster
                        + usize::from(direction.is_reverse());
                    let reshape_range = break_index..reshape_cluster;

                    shaper.set_buffer_content(text.as_ref(), reshape_range, direction);
                    if let Some(result) = self.try_concat_with_reshaped_half(
                        i,
                        direction,
                        true,
                        shaper,
                        font_iterator.clone(),
                        lctx,
                    )? {
                        return Ok(result);
                    }
                }
            }
        }

        // We have to reshape the whole segment, there's no place where we can safely concat.
        let reshape_range = break_index..self.text_range.end;
        shaper.set_buffer_content(text.as_ref(), reshape_range, direction);
        let mut result = GlyphStringSegmentSink(Vec::new());
        shaper.shape(&mut result, font_iterator, lctx)?;
        Ok(result.0)
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

struct GlyphStringSegmentSink(Vec<GlyphStringSegment>);

impl ShapingSink for GlyphStringSegmentSink {
    fn append(&mut self, text_range: Range<usize>, font: &Font, glyphs: &[Glyph]) {
        self.0.push(GlyphStringSegment {
            storage: glyphs.into(),
            glyph_range: 0..glyphs.len(),
            text_range,
            font: font.clone(),
        });
    }
}

impl ShapingSink for GlyphString {
    fn append(&mut self, text_range: Range<usize>, font: &Font, glyphs: &[Glyph]) {
        let mut sink = GlyphStringSegmentSink(std::mem::take(&mut self.segments));
        ShapingSink::append(&mut sink, text_range, font, glyphs);
        self.segments = sink.0;
    }
}

impl GlyphString {
    pub fn new(text: Rc<str>, direction: Direction) -> Self {
        Self {
            text,
            segments: Vec::new(),
            direction,
        }
    }

    pub fn iter_fonts_visual(&self) -> impl DoubleEndedIterator<Item = &Font> {
        self.segments.iter().map(|s| &s.font)
    }

    pub fn iter_glyphs_visual(&self) -> impl DoubleEndedIterator<Item = (&Font, &Glyph)> {
        self.segments
            .iter()
            .flat_map(|s| s.iter().map(|g| (&s.font, g)))
    }

    pub fn iter_glyphs_logical(&self) -> impl DoubleEndedIterator<Item = (&Font, &Glyph)> {
        RevIf::new(
            self.segments
                .iter()
                .flat_map(|s| s.iter().map(|g| (&s.font, g))),
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
        let target_utf8_index = match self.direction.is_reverse() {
            false => end_utf8_index.checked_sub(1)?,
            true => end_utf8_index,
        };

        let mut i = 0;
        while i < self.segments.len() {
            let segment = &mut self.segments[i];
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

            if let Some(last_glyph_index) =
                segment.glyph_at_utf8_index(target_utf8_index, self.direction)
            {
                let last = segment.split_off_visual_start(last_glyph_index + 1, self.direction);
                let result = self.segments.drain(..i);

                return Some(Self {
                    text: self.text.clone(),
                    segments: result.chain(std::iter::once(last)).collect(),
                    direction: self.direction,
                });
            }

            i += 1;
        }

        if i > 0 {
            Some(GlyphString {
                text: self.text.clone(),
                segments: self.segments.drain(..i).collect(),
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
        shaper: &mut RunShaper,
        font_iter: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
    ) -> Result<Option<(Self, Self)>, ShapingError> {
        assert!(!self.is_empty());

        let left_segments;
        let mut current_x = I26Dot6::ZERO;
        let mut it = RevIf::new(
            self.segments.iter().enumerate(),
            self.direction.is_reverse(),
        );
        let mut next = it.next();
        let slice_segments = |pivot: usize, after: bool| {
            if !self.direction.is_reverse() ^ after {
                &self.segments[..pivot]
            } else {
                &self.segments[pivot + 1..]
            }
        };
        let append =
            |list: &[GlyphStringSegment], other: &mut Vec<GlyphStringSegment>, invert: bool| {
                if !self.direction.is_reverse() ^ invert {
                    other.splice(0..0, list.iter().cloned());
                } else {
                    other.extend_from_slice(list);
                }
            };

        loop {
            let Some((i, segment)) = next else {
                return Ok(None);
            };

            if let Some(left_candidate_glyph) =
                segment.glyph_at_utf8_index(break_range.start, self.direction)
            {
                let mut left_candidate = segment.break_until(
                    self.text.clone(),
                    left_candidate_glyph,
                    break_range.start,
                    shaper,
                    font_iter.clone(),
                    lctx,
                    self.direction,
                )?;

                if left_candidate
                    .iter()
                    .flat_map(|s| &s[..])
                    .map(|g| g.x_advance)
                    .fold(current_x, I26Dot6::add)
                    <= max_width
                {
                    append(slice_segments(i, false), &mut left_candidate, false);
                    left_segments = left_candidate;
                    break;
                } else {
                    // This wasn't a valid break because the width ended up being too large.
                    return Ok(None);
                }
            }

            for glyph in segment {
                current_x += glyph.x_advance;
            }

            next = it.next();
        }

        let left = Self {
            text: self.text.clone(),
            segments: left_segments,
            direction: self.direction,
        };
        while let Some((i, segment)) = next {
            if let Some(right_candidate_glyph) =
                segment.glyph_at_utf8_index(break_range.end, self.direction)
            {
                let mut right_segments = segment.break_after(
                    self.text.clone(),
                    right_candidate_glyph,
                    break_range.end,
                    shaper,
                    font_iter,
                    lctx,
                    self.direction,
                )?;

                append(slice_segments(i, true), &mut right_segments, true);

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

        Ok(Some((left, Self::new(self.text.clone(), self.direction))))
    }
}
