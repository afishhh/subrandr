use std::{
    collections::LinkedList,
    fmt::Debug,
    ops::{Add as _, Range},
    rc::Rc,
};

use util::math::I26Dot6;

use crate::text::{
    Direction, FontArena, FontDb, FontMatchIterator, Glyph, ShapingBuffer, ShapingError,
};

pub trait GlyphStringText: AsRef<str> + Clone {}

impl GlyphStringText for &str {}
impl GlyphStringText for Rc<str> {}

#[derive(Clone)]
pub struct GlyphString<'f, T: GlyphStringText> {
    segments: LinkedList<GlyphStringSegment<'f, T>>,
    direction: Direction,
}

impl<T: GlyphStringText> Debug for GlyphString<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GlyphString ")?;
        f.debug_list().entries(self.iter_glyphs()).finish()
    }
}

#[derive(Debug, Clone)]
struct GlyphStringSegment<'f, T: GlyphStringText> {
    /// Always refers to the original string that contains the whole context
    /// of this segment, not only the text of the glyphs themselves.
    text: T,
    /// Some glyph slice which resulted from shaping the entirety or some subslice
    /// of the text in `text`. [`Glyph::cluster`] will be a valid index into `text`.
    storage: Rc<[Glyph<'f>]>,
    /// The subslice of `storage` this segment actually represents.
    range: Range<usize>,
}

impl<'f, T: GlyphStringText> GlyphStringSegment<'f, T> {
    fn from_glyphs(text: T, glyphs: Vec<Glyph<'f>>) -> Self {
        Self {
            text,
            range: 0..glyphs.len(),
            storage: glyphs.into(),
        }
    }

    fn glyphs(&self) -> &[Glyph<'f>] {
        &self.storage[self.range.clone()]
    }

    fn break_unsafe_subslice(&self, range: Range<usize>) -> Self {
        Self {
            text: self.text.clone(),
            storage: self.storage.clone(),
            range: self.range.start + range.start..self.range.start + range.end,
        }
    }

    fn split_off_start(&mut self, count: usize) -> Self {
        let result = self.break_unsafe_subslice(0..count);
        self.range.start += count;
        result
    }

    // These breaking functions *seem to* work, although I haven't tested them that extensively.
    fn break_until(
        &self,
        glyph_index: usize,
        cluster: usize,
        buffer: &mut ShapingBuffer,
        grapheme_cluster_boundaries: &[usize],
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
        direction: Direction,
    ) -> Result<GlyphString<'f, T>, ShapingError> {
        let split_glyph = self.glyphs()[glyph_index];
        // If the break is within a glyph (like a long ligature), we must
        // use the slow reshaping path.
        let can_reuse_split_glyph = split_glyph.cluster == cluster;
        if !split_glyph.unsafe_to_break() && can_reuse_split_glyph {
            // Easy case, we can just split the glyph string right here
            Ok(GlyphString::from_array(
                [self.break_unsafe_subslice(0..glyph_index)],
                direction,
            ))
        } else {
            // Harder case, we have to find the closest glyph on the left which
            // has the UNSAFE_TO_CONCAT flag unset, then try reshaping after such glyphs
            // until the first glyph of the result also has the UNSAFE_TO_CONCAT flag unset.
            let left = 'left: {
                for i in (self.range.start + 1..glyph_index - 1).rev() {
                    if !self.storage[i].unsafe_to_concat() {
                        let concat_glyph = &self.storage[i];
                        buffer.clear();
                        super::set_buffer_content_from_range(
                            buffer,
                            self.text.as_ref(),
                            if concat_glyph.cluster < cluster {
                                // This is left-to-right text
                                concat_glyph.cluster..cluster
                            } else {
                                // This is right-to-left text
                                cluster..concat_glyph.cluster
                            },
                            grapheme_cluster_boundaries,
                        );
                        let glyphs = buffer.shape(font_iterator.clone(), font_arena, fonts)?;
                        if glyphs.first().is_none_or(|first| !first.unsafe_to_concat()) {
                            break 'left GlyphString::from_array(
                                [
                                    self.break_unsafe_subslice(0..i),
                                    GlyphStringSegment::from_glyphs(self.text.clone(), glyphs),
                                ],
                                direction,
                            );
                        } else {
                            // The result cannot be concatenated with the other part,
                            // we have to try again in another position.
                        }
                    }
                }

                // We have to reshape the whole segment, there's no place where we can safely concat.
                buffer.clear();
                super::set_buffer_content_from_range(
                    buffer,
                    self.text.as_ref(),
                    self.glyphs().first().unwrap().cluster..cluster,
                    grapheme_cluster_boundaries,
                );
                GlyphString::from_glyphs(
                    self.text.clone(),
                    buffer.shape(font_iterator, font_arena, fonts)?,
                    direction,
                )
            };

            Ok(left)
        }
    }

    fn break_after(
        &self,
        glyph_index: usize,
        cluster: usize,
        buffer: &mut ShapingBuffer,
        grapheme_cluster_boundaries: &[usize],
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
        direction: Direction,
    ) -> Result<GlyphString<'f, T>, ShapingError> {
        let split_glyph = self.glyphs()[glyph_index];
        let can_reuse_split_glyph = split_glyph.cluster == cluster;
        if !split_glyph.unsafe_to_break() && can_reuse_split_glyph {
            let right = Self {
                text: self.text.clone(),
                storage: self.storage.clone(),
                range: self.range.start + glyph_index..self.range.end,
            };

            Ok(GlyphString::from_array([right], direction))
        } else {
            // Analogous to the process in `split_until`, but performed on the right searching forward.
            let right = 'right: {
                for i in glyph_index + 1..self.range.end - 1 {
                    if !self.storage[i].unsafe_to_concat() {
                        let concat_glyph = &self.storage[i];
                        buffer.clear();
                        super::set_buffer_content_from_range(
                            buffer,
                            self.text.as_ref(),
                            if concat_glyph.cluster > cluster {
                                // This is left-to-right text
                                cluster..concat_glyph.cluster
                            } else {
                                // This is right-to-left text
                                concat_glyph.cluster..cluster
                            },
                            grapheme_cluster_boundaries,
                        );
                        let glyphs = buffer.shape(font_iterator.clone(), font_arena, fonts)?;
                        if glyphs.last().is_none_or(|last| !last.unsafe_to_concat()) {
                            break 'right GlyphString::from_array(
                                [
                                    GlyphStringSegment::from_glyphs(self.text.clone(), glyphs),
                                    self.break_unsafe_subslice(i..self.range.len()),
                                ],
                                direction,
                            );
                        } else {
                            // The result cannot be concatenated with the other part,
                            // we have to try again in another position.
                        }
                    }
                }

                // We have to reshape the whole segment, there's no place where we can safely concat.
                buffer.clear();
                super::set_buffer_content_from_range(
                    buffer,
                    self.text.as_ref(),
                    cluster..self.glyphs().last().unwrap().cluster,
                    grapheme_cluster_boundaries,
                );
                GlyphString::from_glyphs(
                    self.text.clone(),
                    buffer.shape(font_iterator, font_arena, fonts)?,
                    direction,
                )
            };

            Ok(right)
        }
    }
}

// TODO: linked_list_cursors feature would improve some of this code significantly
impl<'f, T: GlyphStringText> GlyphString<'f, T> {
    pub fn from_glyphs(text: T, glyphs: Vec<Glyph<'f>>, direction: Direction) -> Self {
        GlyphString {
            segments: LinkedList::from([GlyphStringSegment::from_glyphs(text, glyphs)]),
            direction,
        }
    }

    fn from_array<const N: usize>(
        segments: [GlyphStringSegment<'f, T>; N],
        direction: Direction,
    ) -> GlyphString<'f, T> {
        GlyphString {
            segments: LinkedList::from(segments),
            direction,
        }
    }

    pub fn iter_glyphs(&self) -> impl DoubleEndedIterator<Item = &Glyph<'f>> {
        self.segments.iter().flat_map(|s| s.glyphs().iter())
    }

    pub fn is_empty(&self) -> bool {
        // TODO: just make empty segments an invariant of `GlyphString`
        self.segments.is_empty() || self.segments.iter().all(|s| s.glyphs().is_empty())
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }

    // TODO: Maybe this could be gotten rid of by reworking how glyphs are styled?
    //       Don't know whether that's a good idea
    pub fn split_off_until_cluster(&mut self, cluster: usize) -> Option<Self> {
        let mut result = LinkedList::new();

        while let Some(mut segment) = self.segments.pop_front() {
            // TODO: linked_list_cursors feature would avoid unnecessary re-allocations here
            match segment.glyphs().iter().position(|glyph| {
                if self.direction.is_reverse() {
                    glyph.cluster <= cluster
                } else {
                    glyph.cluster >= cluster
                }
            }) {
                Some(end) => {
                    if end != 0 {
                        result.push_back(segment.split_off_start(end));
                    }
                    self.segments.push_front(segment);
                    return Some(Self {
                        segments: result,
                        direction: self.direction,
                    });
                }
                None => result.push_back(segment),
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(GlyphString {
                segments: result,
                direction: self.direction,
            })
        }
    }

    // This also seems to work although it's a little bit arcane
    pub fn break_at_if_less_or_eq(
        &self,
        cluster: usize,
        max_width: I26Dot6,
        buffer: &mut ShapingBuffer,
        grapheme_cluster_boundaries: &[usize],
        font_iter: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Option<(Self, Self)>, ShapingError> {
        let mut left = LinkedList::new();
        let mut it = self.segments.iter();
        let mut x = I26Dot6::ZERO;
        for segment in &mut it {
            // TODO: if g.cluster > cluster then use previous glyph
            //       ^^^^ RTL?
            if let Some(split_at) = segment.glyphs().iter().position(|g| g.cluster == cluster) {
                let mut left_suff = segment.break_until(
                    split_at,
                    cluster,
                    buffer,
                    grapheme_cluster_boundaries,
                    font_iter.clone(),
                    font_arena,
                    fonts,
                    self.direction,
                )?;

                if left_suff
                    .iter_glyphs()
                    .map(|g| g.x_advance)
                    .fold(x, I26Dot6::add)
                    <= max_width
                {
                    left.append(&mut left_suff.segments);
                    let mut right = segment.break_after(
                        split_at,
                        cluster,
                        buffer,
                        grapheme_cluster_boundaries,
                        font_iter,
                        font_arena,
                        fonts,
                        self.direction,
                    )?;

                    for segment in it {
                        right.segments.push_back(segment.clone());
                    }

                    return Ok(Some((
                        Self {
                            segments: left,
                            direction: self.direction,
                        },
                        right,
                    )));
                }
            }

            for glyph in segment.glyphs() {
                x += glyph.x_advance;
            }
        }

        Ok(None)
    }
}
