use std::{
    collections::LinkedList,
    fmt::Debug,
    ops::{Add as _, Range},
    rc::Rc,
};

use crate::math::I26Dot6;

use super::{font_match::FontMatchIterator, FontArena, FontDb, Glyph, ShapingBuffer, ShapingError};

pub trait GlyphStringText: AsRef<str> + Clone {}

impl GlyphStringText for &str {}
impl GlyphStringText for Rc<str> {}

#[derive(Clone)]
pub struct GlyphString<'f, T: GlyphStringText> {
    pub segments: LinkedList<GlyphStringSegment<'f, T>>,
}

impl<T: GlyphStringText> Debug for GlyphString<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphString").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct GlyphStringSegment<'f, T: GlyphStringText> {
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
    pub fn from_glyphs(text: T, glyphs: Vec<Glyph<'f>>) -> Self {
        Self {
            text,
            range: 0..glyphs.len(),
            storage: glyphs.into(),
        }
    }

    pub fn glyphs(&self) -> &[Glyph<'f>] {
        &self.storage[self.range.clone()]
    }

    fn break_unsafe_subslice(&self, range: Range<usize>) -> Self {
        Self {
            text: self.text.clone(),
            storage: self.storage.clone(),
            range: self.range.start + range.start..self.range.start + range.end,
        }
    }

    pub fn split_off_start(&mut self, count: usize) -> Self {
        let result = self.break_unsafe_subslice(0..count);
        self.range.start += count;
        result
    }

    // These breaking functions *seem to* work, although I haven't tested them that extensively.
    pub fn break_until(
        &self,
        glyph_index: usize,
        cluster: usize,
        buffer: &mut ShapingBuffer,
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<GlyphString<'f, T>, ShapingError> {
        let split_glyph = self.glyphs()[glyph_index];
        // If the break is within a glyph (like a long ligature), we must
        // use the slow reshaping path.
        let can_reuse_split_glyph = split_glyph.cluster == cluster;
        if !split_glyph.unsafe_to_break() && can_reuse_split_glyph {
            // Easy case, we can just split the glyph string right here
            Ok(GlyphString::from_array([
                self.break_unsafe_subslice(0..glyph_index)
            ]))
        } else {
            // Harder case, we have to find the closest glyph on the left which
            // has the UNSAFE_TO_CONCAT flag unset, then try reshaping after such glyphs
            // until the first glyph of the result also has the UNSAFE_TO_CONCAT flag unset.
            let left = 'left: {
                for i in (self.range.start + 1..glyph_index - 1).rev() {
                    if !self.storage[i].unsafe_to_concat() {
                        let concat_glyph = &self.storage[i];
                        buffer.clear();
                        buffer.add(
                            self.text.as_ref(),
                            if concat_glyph.cluster < cluster {
                                // This is left-to-right text
                                concat_glyph.cluster..cluster
                            } else {
                                // This is right-to-left text
                                cluster..concat_glyph.cluster
                            },
                        );
                        let glyphs = buffer.shape(font_iterator.clone(), font_arena, fonts)?;
                        if glyphs.first().is_none_or(|first| !first.unsafe_to_concat()) {
                            break 'left GlyphString::from_array([
                                self.break_unsafe_subslice(0..i),
                                GlyphStringSegment::from_glyphs(self.text.clone(), glyphs),
                            ]);
                        } else {
                            // The result cannot be concatenated with the other part,
                            // we have to try again in another position.
                        }
                    }
                }

                // We have to reshape the whole segment, there's no place where we can safely concat.
                buffer.clear();
                buffer.add(
                    self.text.as_ref(),
                    self.glyphs().first().unwrap().cluster..cluster,
                );
                GlyphString::from_glyphs(
                    self.text.clone(),
                    buffer.shape(font_iterator, font_arena, fonts)?,
                )
            };

            Ok(left)
        }
    }

    pub fn break_after(
        &self,
        glyph_index: usize,
        cluster: usize,
        buffer: &mut ShapingBuffer,
        font_iterator: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<GlyphString<'f, T>, ShapingError> {
        let split_glyph = self.glyphs()[glyph_index];
        let can_reuse_split_glyph = split_glyph.cluster == cluster;
        if !split_glyph.unsafe_to_break() && can_reuse_split_glyph {
            let right = Self {
                text: self.text.clone(),
                storage: self.storage.clone(),
                range: self.range.start + glyph_index..self.range.end,
            };

            Ok(GlyphString::from_array([right]))
        } else {
            // Analogous to the process in `split_until`, but performed on the right searching forward.
            let right = 'right: {
                for i in glyph_index + 1..self.range.end - 1 {
                    if !self.storage[i].unsafe_to_concat() {
                        let concat_glyph = &self.storage[i];
                        buffer.clear();
                        buffer.add(
                            self.text.as_ref(),
                            if concat_glyph.cluster > cluster {
                                // This is left-to-right text
                                cluster..concat_glyph.cluster
                            } else {
                                // This is right-to-left text
                                concat_glyph.cluster..cluster
                            },
                        );
                        let glyphs = buffer.shape(font_iterator.clone(), font_arena, fonts)?;
                        if glyphs.last().is_none_or(|last| !last.unsafe_to_concat()) {
                            break 'right GlyphString::from_array([
                                GlyphStringSegment::from_glyphs(self.text.clone(), glyphs),
                                self.break_unsafe_subslice(i..self.range.len()),
                            ]);
                        } else {
                            // The result cannot be concatenated with the other part,
                            // we have to try again in another position.
                        }
                    }
                }

                // We have to reshape the whole segment, there's no place where we can safely concat.
                buffer.clear();
                buffer.add(
                    self.text.as_ref(),
                    cluster..self.glyphs().last().unwrap().cluster,
                );
                GlyphString::from_glyphs(
                    self.text.clone(),
                    buffer.shape(font_iterator, font_arena, fonts)?,
                )
            };

            Ok(right)
        }
    }
}

// TODO: linked_list_cursors feature would improve some of this code significantly
impl<'f, T: GlyphStringText> GlyphString<'f, T> {
    pub fn from_glyphs(text: T, glyphs: Vec<Glyph<'f>>) -> Self {
        GlyphString {
            segments: LinkedList::from([GlyphStringSegment::from_glyphs(text, glyphs)]),
        }
    }

    pub fn from_array<const N: usize>(
        segments: [GlyphStringSegment<'f, T>; N],
    ) -> GlyphString<'f, T> {
        GlyphString {
            segments: LinkedList::from(segments),
        }
    }

    pub fn iter_glyphs(&self) -> impl DoubleEndedIterator<Item = &Glyph<'f>> {
        self.segments.iter().flat_map(|s| s.glyphs().iter())
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty() || self.segments.iter().all(|s| s.glyphs().is_empty())
    }

    // TODO: Maybe this could be gotten rid of by reworking how glyphs are styled?
    //       Don't know whether that's a good idea
    pub fn split_off_until_cluster(&mut self, cluster: usize) -> Option<Self> {
        let mut result = LinkedList::new();

        while let Some(mut segment) = self.segments.pop_front() {
            // TODO: linked_list_cursors feature would avoid unnecessary re-allocations here
            match segment
                .glyphs()
                .iter()
                .position(|glyph| glyph.cluster >= cluster)
            {
                Some(end) => {
                    if end != 0 {
                        result.push_back(segment.split_off_start(end));
                    }
                    self.segments.push_front(segment);
                    return Some(Self { segments: result });
                }
                None => result.push_back(segment),
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(GlyphString { segments: result })
        }
    }

    // This also seems to work although it's a little bit arcane
    pub fn break_at_if_less_or_eq(
        &self,
        cluster: usize,
        max_width: I26Dot6,
        buffer: &mut ShapingBuffer,
        font_iter: FontMatchIterator<'_, 'f>,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<Option<(Self, Self)>, ShapingError> {
        let mut left = LinkedList::new();
        let mut it = self.segments.iter();
        let mut x = I26Dot6::ZERO;
        for segment in &mut it {
            // TODO: if g.cluster > cluster then use previous glyph
            if let Some(split_at) = segment.glyphs().iter().position(|g| g.cluster == cluster) {
                let mut left_suff = segment.break_until(
                    split_at,
                    cluster,
                    buffer,
                    font_iter.clone(),
                    font_arena,
                    fonts,
                )?;

                if left_suff
                    .iter_glyphs()
                    .map(|g| g.x_advance)
                    .fold(x, I26Dot6::add)
                    <= max_width
                {
                    left.append(&mut left_suff.segments);
                    let mut right = segment
                        .break_after(split_at, cluster, buffer, font_iter, font_arena, fonts)?;

                    for segment in it {
                        right.segments.push_back(segment.clone());
                    }

                    return Ok(Some((GlyphString { segments: left }, right)));
                }
            }

            for glyph in segment.glyphs() {
                x += glyph.x_advance;
            }
        }

        Ok(None)
    }
}
