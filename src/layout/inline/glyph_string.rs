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
                    .field("text_range", &&segment.text_range)
                    .field(
                        "glyphs",
                        &util::fmt_from_fn(|f| {
                            let mut list = f.debug_list();
                            for glyph in segment {
                                list.entry(glyph);
                            }
                            list.finish()
                        }),
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
    #[track_caller]
    fn subslice(&self, range: Range<usize>) -> Self {
        assert!(
            range.start < range.end,
            "Invalid glyph string segment subslice ({} >= {})",
            range.start,
            range.end
        );

        Self {
            storage: self.storage.clone(),
            glyph_range: self.glyph_range.start + range.start..self.glyph_range.start + range.end,
            text_range: self[range.start].cluster
                ..self
                    .get(range.end)
                    .map_or(self.text_range.end, |g| g.cluster),
            font: self.font.clone(),
        }
    }

    // [`Self::subslice`] does not allow creating an empty subslice so that "no empty segments"
    // can be an invariant upheld by [`GlyphString`].
    // This function allows for empty subslices but returns them as an empty [`LinkedList`].
    fn subslice_into_vec(&self, range: Range<usize>) -> Vec<Self> {
        if range.start == range.end {
            return Vec::new();
        }

        Vec::from([self.subslice(range)])
    }

    fn split_off_logical_start(&mut self, pivot: usize) -> Self {
        let result = self.subslice(0..pivot);
        self.glyph_range.start += pivot;
        self.text_range.start = result.text_range.end;
        debug_assert!(!self.glyph_range.is_empty());
        result
    }

    fn split_off_logical_end(&mut self, pivot: usize) -> Self {
        let result = self.subslice(pivot..self.len());
        self.glyph_range.end = self.glyph_range.start + pivot;
        self.text_range.end = result.text_range.start;
        debug_assert!(!self.glyph_range.is_empty());
        result
    }

    fn try_concat_with_reshaped_end(
        &self,
        pivot: usize,
        shaper: &mut RunShaper,
        font_iterator: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
    ) -> Result<Option<Vec<Self>>, ShapingError> {
        let mut other = GlyphStringSegmentSink(vec![self.subslice(0..pivot + 1)]);
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
        let can_reuse_split_glyph = self[glyph_index].cluster == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            // Easy case, we can just split the glyph string right here
            return Ok(self.subslice_into_vec(0..glyph_index));
        } else if glyph_index > 1 {
            // The hard case, we have to find the closest glyph on the left which
            // has the UNSAFE_TO_CONCAT flag unset, then try reshaping after such glyphs
            // until the first glyph of the result also has the UNSAFE_TO_CONCAT flag unset.
            for i in (1..glyph_index).rev() {
                if !self[i].unsafe_to_concat() {
                    let reshape_cluster = self[i + 1].cluster;
                    let reshape_range = reshape_cluster..break_index;

                    shaper.set_buffer_content(&text, reshape_range, direction);
                    if let Some(result) =
                        self.try_concat_with_reshaped_end(i, shaper, font_iterator.clone(), lctx)?
                    {
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

    fn try_concat_with_reshaped_start(
        &self,
        pivot: usize,
        shaper: &mut RunShaper,
        font_iterator: FontMatchIterator<'_>,
        lctx: &mut LayoutContext,
    ) -> Result<Option<Vec<Self>>, ShapingError> {
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

        other.0.push(self.subslice(pivot..self.len()));
        Ok(Some(other.0))
    }

    // Analogous to `break_until` but returns the part logically after `break_index` (inclusive).
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
        let can_reuse_split_glyph = self[glyph_index].cluster == break_index;
        if !self[glyph_index].unsafe_to_break() && can_reuse_split_glyph {
            return Ok(self.subslice_into_vec(glyph_index..self.len()));
        } else if glyph_index < self.len() - 2 {
            for i in glyph_index + 1..self.len() - 1 {
                if !self[i].unsafe_to_concat() {
                    let reshape_cluster = self[i].cluster;
                    let reshape_range = break_index..reshape_cluster;

                    shaper.set_buffer_content(text.as_ref(), reshape_range, direction);
                    if let Some(result) =
                        self.try_concat_with_reshaped_start(i, shaper, font_iterator.clone(), lctx)?
                    {
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

    fn glyph_at_utf8_index(&self, index: usize) -> Option<usize> {
        if !self.text_range.contains(&index) {
            return None;
        }

        Some(self.iter().position(|g| g.cluster >= index).unwrap())
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

    pub fn iter_fonts_logical(&self) -> impl DoubleEndedIterator<Item = &Font> {
        self.segments.iter().map(|s| &s.font)
    }

    pub fn iter_glyphs_visual(&self) -> impl DoubleEndedIterator<Item = (&Font, &Glyph)> {
        RevIf::new(
            self.segments
                .iter()
                .flat_map(|s| s.iter().map(|g| (&s.font, g))),
            self.direction.is_reverse(),
        )
    }

    pub fn iter_glyphs_logical(&self) -> impl DoubleEndedIterator<Item = (&Font, &Glyph)> {
        self.segments
            .iter()
            .flat_map(|s| s.iter().map(|g| (&s.font, g)))
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

    fn split_off_logical_half(&mut self, utf8_pivot: usize, forward: bool) -> Option<Self> {
        let mut i = 0;
        for segment in self.segments.iter_mut() {
            if segment.text_range.start >= utf8_pivot {
                break;
            }

            if segment.text_range.end <= utf8_pivot {
                i += 1;
                continue;
            }

            let Some(pivot) = segment.iter().position(|g| g.cluster >= utf8_pivot) else {
                i += 1;
                continue;
            };
            debug_assert!(pivot > 0);

            // TODO: Replace the below `.chain()` calls with `std::iter::chain` (needs MSRV >= 1.91)
            if forward {
                let last = segment.split_off_logical_start(pivot);
                return Some(Self {
                    text: self.text.clone(),
                    segments: self
                        .segments
                        .drain(..i)
                        .chain(std::iter::once(last))
                        .collect(),
                    direction: self.direction,
                });
            } else {
                let first = segment.split_off_logical_end(pivot);
                return Some(Self {
                    text: self.text.clone(),
                    segments: std::iter::once(first)
                        .chain(self.segments.drain(i + 1..))
                        .collect(),
                    direction: self.direction,
                });
            };
        }

        let segments = if forward {
            if i == 0 {
                return None;
            }

            self.segments.drain(..i).collect()
        } else {
            if i == self.segments.len() {
                return None;
            }

            self.segments.drain(i..).collect()
        };

        Some(GlyphString {
            text: self.text.clone(),
            segments,
            direction: self.direction,
        })
    }

    pub(super) fn split_off_logical_start(&mut self, end_utf8_index: usize) -> Option<Self> {
        self.split_off_logical_half(end_utf8_index, true)
    }

    pub(super) fn split_off_logical_end(&mut self, start_utf8_index: usize) -> Option<Self> {
        self.split_off_logical_half(start_utf8_index, false)
    }

    pub(super) fn split_off_visual_start(&mut self, end_utf8_index: usize) -> Option<Self> {
        self.split_off_logical_half(end_utf8_index, !self.direction.is_reverse())
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
        let mut it = self.segments.iter().enumerate();
        let mut next = it.next();

        loop {
            let Some((i, segment)) = next else {
                return Ok(None);
            };

            if let Some(left_candidate_glyph) = segment.glyph_at_utf8_index(break_range.start) {
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
                    left_candidate.splice(0..0, self.segments[..i].iter().cloned());
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
            if let Some(right_candidate_glyph) = segment.glyph_at_utf8_index(break_range.end) {
                let mut right_segments = segment.break_after(
                    self.text.clone(),
                    right_candidate_glyph,
                    break_range.end,
                    shaper,
                    font_iter,
                    lctx,
                    self.direction,
                )?;

                right_segments.extend_from_slice(&self.segments[i + 1..]);

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

#[cfg(test)]
mod test {
    use std::rc::Rc;

    use util::math::I26Dot6;

    use super::GlyphString;
    use crate::text::{Direction, Face, Glyph};

    #[test]
    fn split_off_in_glyph() {
        let original = {
            let font = Face::tofu().with_size(I26Dot6::new(16), 72).unwrap();
            let mut result = GlyphString::new(Rc::from("abcde"), Direction::Ltr);
            result.segments.extend([
                super::GlyphStringSegment {
                    storage: Rc::from([Glyph::test_new(1, 0), Glyph::test_new(2, 1)]),
                    glyph_range: 0..2,
                    text_range: 0..3,
                    font: font.clone(),
                },
                super::GlyphStringSegment {
                    storage: Rc::from([Glyph::test_new(3, 3)]),
                    glyph_range: 0..2,
                    text_range: 3..5,
                    font,
                },
            ]);
            result
        };

        let start = original.clone().split_off_logical_start(2).unwrap();
        let end = original.clone().split_off_logical_end(2).unwrap();
        assert_eq!(start.segments.len(), 1);
        assert_eq!(start.segments[0].text_range, 0..3);
        assert_eq!(end.segments.len(), 1);
        assert_eq!(end.segments[0].text_range, 3..5);
    }
}
