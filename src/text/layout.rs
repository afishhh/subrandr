use std::rc::Rc;

use icu_segmenter::{LineBreakOptions, LineBreakStrictness, LineBreakWordOption};
use thiserror::Error;

use crate::{
    math::{I26Dot6, Point2, Rect2, Vec2},
    style::types::HorizontalAlignment,
    text::{self, FontArena, FontDb, FontMatcher, GlyphString, TextMetrics},
};

const MULTILINE_SHAPER_DEBUG_PRINT: bool = false;

struct ShaperSegment<'f> {
    content: Content<'f>,
    end: usize,
}

enum Content<'f> {
    Text(TextContent<'f>),
    None,
}

struct TextContent<'f> {
    font_matcher: FontMatcher<'f>,
    internal_breaks_allowed: bool,
    ruby_annotation: Option<Box<RubyAnnotation<'f>>>,
}

struct RubyAnnotation<'f> {
    font_matcher: FontMatcher<'f>,
    input_index: usize,
    // Note: Text does not shape or form ligatures across ruby annotations or bases, even merged ones, due to bidi isolation. See § 3.5 Bidi Reordering and CSS Text 3 § 7.3 Shaping Across Element Boundaries.
    // ^^ This means we can treat all ruby annotation as completely separate pieces of text.
    text: Rc<str>,
}

pub struct MultilineTextShaper<'f> {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segments: Vec<ShaperSegment<'f>>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug, Clone)]
pub struct ShapedSegment<'f> {
    pub glyphs: GlyphString<'f, Rc<str>>,
    pub baseline_offset: Point2<I26Dot6>,
    pub logical_rect: Rect2<I26Dot6>,
    pub corresponding_input_segment: usize,
}

#[derive(Debug, Clone)]
pub struct ShapedLine<'f> {
    pub segments: Vec<ShapedSegment<'f>>,
    pub bounding_rect: Rect2<I26Dot6>,
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error(transparent)]
    Metrics(#[from] text::FreeTypeError),
}

fn shape_simple_segment<'f>(
    text: Rc<str>,
    range: impl text::ItemRange,
    font_iterator: text::FontMatchIterator<'_, 'f>,
    font_arena: &'f FontArena,
    fonts: &mut FontDb,
) -> Result<(Vec<text::Glyph<'f>>, TextMetrics), LayoutError> {
    let primary = font_iterator
        .matcher()
        .primary(font_arena, fonts)
        .map_err(text::ShapingError::FontSelect)?;

    let glyphs = {
        let mut buffer = text::ShapingBuffer::new();
        buffer.reset();
        buffer.add(&text, range);
        let direction = buffer.guess_properties();
        if !direction.is_horizontal() {
            buffer.set_direction(direction.to_horizontal());
        }
        buffer.shape(font_iterator, font_arena, fonts)?
    };

    let mut metrics = text::compute_extents_ex(true, &glyphs)?;
    if let Some(font) = primary {
        metrics.extend_by_font(font);
    }

    Ok((glyphs, metrics))
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWrapMode {
    /// Greedy line breaking.
    #[default]
    Normal,
}

#[derive(Debug, Clone, Copy)]
pub struct TextWrapOptions {
    pub mode: TextWrapMode,
    pub strictness: LineBreakStrictness,
    pub word_break: LineBreakWordOption,
}

impl Default for TextWrapOptions {
    fn default() -> Self {
        Self {
            mode: TextWrapMode::Normal,
            strictness: LineBreakStrictness::Normal,
            word_break: LineBreakWordOption::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RubyBaseId(usize);

// Notes on ruby support:
// Follows https://www.w3.org/TR/css-ruby-1/
// Only a single annotation level is (currently) supported.
// Whitespace should be handled according to the spec before it is passed here.
// (I do not believe this is currently done correctly as of now though)
// Annotations are passed into the shaper already paired with appriopriate bases.
// Ruby bases and annotations forbid internal line wrapping.
// All ruby annotations have exactly one base.
// TODO: default ruby-align is "space-around", this means justification with extra
//       justification opportunities at the start and end of the text
//       justification is not yet implemented, implement it. (
//         with the generic "justification system" to make it simpler to do this,
//         it should probably work like MultilineTextShaper except it accepts glyphstrings?
//       )
// Chromium seems to lay out ruby text at the top of the current entire line box,
// *when the whole thing is in one block* but youtube uses inline-block so the sane
// layout is correct.

impl<'f> MultilineTextShaper<'f> {
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segments: Vec::new(),
            intra_font_segment_splits: Vec::new(),
        }
    }

    pub fn add_text(&mut self, mut text: &str, font_matcher: FontMatcher<'f>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT TEXT: {text:?} {font_matcher:?}");
        }

        while let Some(nl) = text.find('\n') {
            self.text.push_str(&text[..nl]);
            self.explicit_line_bounaries.push(self.text.len());
            text = &text[nl + 1..];
        }
        self.text.push_str(text);

        if let Some(&mut ShaperSegment {
            content:
                Content::Text(TextContent {
                    font_matcher: ref last_matcher,
                    internal_breaks_allowed: true,
                    ruby_annotation: None,
                }),
            end: ref mut last_end,
        }) = self.segments.last_mut()
        {
            if last_matcher == &font_matcher {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font_matcher,
                internal_breaks_allowed: true,
                ruby_annotation: None,
            }),
            end: self.text.len(),
        });
    }

    // TODO: Maybe a better system should be devised than this.
    //       Potentially just track an arbitrary `usize` provided as input for each segment,
    //       would require some restructuring.
    pub fn skip_segment_for_output(&mut self) {
        self.segments.push(ShaperSegment {
            content: Content::None,
            end: self.text.len(),
        });
    }

    pub fn add_ruby_base(&mut self, text: &str, font_matcher: FontMatcher<'f>) -> RubyBaseId {
        let id = self.segments.len();

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT RUBY BASE[{id}]: {font_matcher:?}");
        }

        self.text.push_str(text);
        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font_matcher,
                internal_breaks_allowed: false,
                ruby_annotation: None,
            }),
            end: self.text.len(),
        });

        RubyBaseId(id)
    }

    pub fn add_ruby_annotation(
        &mut self,
        base: RubyBaseId,
        text: impl Into<Rc<str>> + std::fmt::Debug,
        font_matcher: FontMatcher<'f>,
    ) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!(
                "SHAPING V2 INPUT RUBY ANNOTATION FOR {}: {text:?} {font_matcher:?}",
                base.0
            );
        }

        let index = self.segments.len() + self.intra_font_segment_splits.len();

        let ShaperSegment {
            content:
                Content::Text(TextContent {
                    internal_breaks_allowed: false,
                    ruby_annotation: ref mut ruby_annotation @ None,
                    ..
                }),
            ..
        } = self.segments[base.0]
        else {
            panic!("ruby annotation placed on non-ruby base segment or one that already has an annotation in multiline shaper");
        };

        *ruby_annotation = Some(Box::new(RubyAnnotation {
            font_matcher,
            input_index: index,
            text: text.into(),
        }));
        self.skip_segment_for_output();
    }

    pub fn shape(
        &mut self,
        line_alignment: HorizontalAlignment,
        wrap: TextWrapOptions,
        wrap_width: I26Dot6,
        font_arena: &'f FontArena,
        fonts: &mut FontDb,
    ) -> Result<(Vec<ShapedLine<'f>>, Rect2<I26Dot6>), LayoutError> {
        while self
            .explicit_line_bounaries
            .pop_if(|i| *i == self.text.len())
            .is_some()
        {}

        if self.text.is_empty() {
            return Ok((Vec::new(), Rect2::ZERO));
        }

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 TEXT {:?}", self.text);
            println!(
                "SHAPING V2 LINE BOUNDARIES {:?}",
                self.explicit_line_bounaries
            );
        }

        if self.segments.is_empty() {
            return Ok((Vec::new(), Rect2::ZERO));
        }

        let segmenter = icu_segmenter::LineSegmenter::new_auto_with_options({
            let mut options = LineBreakOptions::default();
            options.strictness = wrap.strictness;
            options.word_option = wrap.word_break;
            options
        });

        let text: Rc<str> = std::mem::take(&mut self.text).into();
        let mut lines: Vec<ShapedLine> = vec![];
        let mut current_line_y = I26Dot6::ZERO;
        let mut total_rect = Rect2::NOTHING;

        let mut current_explicit_line = 0;
        let mut current_segment = 0;
        let mut current_intra_split = 0;
        let mut last = 0;
        while current_explicit_line <= self.explicit_line_bounaries.len() {
            let mut line_boundary = self
                .explicit_line_bounaries
                .get(current_explicit_line)
                .copied()
                .unwrap_or(text.len());
            let mut annotation_segments: Vec<ShapedSegment> = Vec::new();
            let mut segments: Vec<ShapedSegment> = Vec::new();
            let mut current_x = I26Dot6::ZERO;

            if MULTILINE_SHAPER_DEBUG_PRINT {
                println!(
                    "last: {last}, segment boundaries: {:?}, current segment: {}",
                    self.segments
                        .iter()
                        .map(|ShaperSegment { end, .. }| end)
                        .collect::<Vec<_>>(),
                    current_segment
                );
            }

            let mut line_max_ascender = I26Dot6::ZERO;
            let mut line_min_descender = I26Dot6::ZERO;
            // TODO: Line height should actually be calculated with respect to the
            //       whole *inline box*!!! Not its fragments like we currently do.
            //       See <https://www.w3.org/TR/css-inline-3/#inline-height> which refers
            //       purely to "inline box"es and not their constituent fragments.
            let mut line_max_lineskip_descent = I26Dot6::ZERO;
            let mut annotations_max_ascender = I26Dot6::ZERO;

            while self.segments[current_segment].end <= last {
                current_segment += 1;
            }

            let mut post_wrap_glyphs: Option<GlyphString<'f, Rc<str>>> = None;

            loop {
                let ShaperSegment {
                    content: ref segment,
                    end: font_boundary,
                } = self.segments[current_segment];

                let mut end = font_boundary.min(line_boundary);
                let segment_slice = last..end;

                match segment {
                    Content::None => {}
                    &Content::Text(TextContent {
                        ref font_matcher,
                        internal_breaks_allowed,
                        ref ruby_annotation,
                    }) => {
                        let primary = font_matcher
                            .primary(font_arena, fonts)
                            .map_err(text::ShapingError::FontSelect)?
                            .ok_or(text::ShapingError::FontSelect(
                                text::font_db::SelectError::NotFound,
                            ))?;

                        let (mut glyphs, mut extents) = match post_wrap_glyphs.take() {
                            Some(glyphs) => {
                                let mut metrics =
                                    text::compute_extents_ex(true, glyphs.iter_glyphs())?;
                                metrics.extend_by_font(primary);

                                (glyphs, metrics)
                            }
                            None => {
                                let (vec, metrics) = shape_simple_segment(
                                    text.clone(),
                                    segment_slice.clone(),
                                    font_matcher.iterator(),
                                    font_arena,
                                    fonts,
                                )?;
                                (GlyphString::from_glyphs(text.clone(), vec), metrics)
                            }
                        };

                        // TODO: Inter-inline-block line breaking.
                        if wrap.mode == TextWrapMode::Normal
                            && internal_breaks_allowed
                            && current_x + extents.paint_size.x > wrap_width
                        {
                            const MAX_TRIES: usize = 3;

                            let max_width = wrap_width - current_x;
                            // A MAX_TRIES-wide ring buffer for breaking opportunities.
                            let mut candidate_breaks = [last; MAX_TRIES];
                            let breaks = segmenter.segment_str(&text[segment_slice.clone()]);
                            let mut glyph_it = glyphs.iter_glyphs().peekable();

                            let mut pos = I26Dot6::ZERO;
                            for offset in breaks {
                                let cluster = offset + segment_slice.start;

                                while let Some(glyph) =
                                    glyph_it.next_if(|glyph| glyph.cluster < cluster)
                                {
                                    pos += glyph.x_advance;
                                }

                                if pos > max_width {
                                    break;
                                } else {
                                    for i in (1..MAX_TRIES).rev() {
                                        candidate_breaks[i] = candidate_breaks[i - 1];
                                    }
                                    candidate_breaks[0] = cluster;
                                }
                            }

                            for candidate in candidate_breaks {
                                if candidate == last {
                                    continue;
                                }

                                if let Some((broken, remaining)) = glyphs.break_at_if_less_or_eq(
                                    candidate,
                                    max_width,
                                    &mut text::ShapingBuffer::new(),
                                    font_matcher.iterator(),
                                    font_arena,
                                    fonts,
                                )? {
                                    drop(glyph_it);
                                    glyphs = broken;
                                    post_wrap_glyphs = Some(remaining);
                                    end = candidate;
                                    line_boundary = candidate;

                                    extents = text::compute_extents_ex(true, glyphs.iter_glyphs())?;
                                    extents.extend_by_font(primary);

                                    break;
                                }
                            }
                        }

                        if MULTILINE_SHAPER_DEBUG_PRINT {
                            println!(
                                "last: {last}, end: {end}, intra font splits: {:?}, current intra split: {}",
                                self.intra_font_segment_splits, current_intra_split
                            );
                        }

                        line_max_ascender = line_max_ascender.max(extents.max_ascender);
                        line_min_descender = line_min_descender.min(extents.min_descender);
                        line_max_lineskip_descent =
                            line_max_lineskip_descent.max(extents.max_lineskip_descent);

                        let logical_height = extents.max_ascender - extents.min_descender;

                        let ruby_padding = if let Some(annotation) = ruby_annotation {
                            let (glyphs, ruby_metrics) = shape_simple_segment(
                                annotation.text.clone(),
                                ..,
                                annotation.font_matcher.iterator(),
                                font_arena,
                                fonts,
                            )?;

                            let base_width = extents.paint_size.x + extents.trailing_advance;
                            let ruby_width =
                                ruby_metrics.paint_size.x + ruby_metrics.trailing_advance;
                            let (base_padding, ruby_padding) = if ruby_width > base_width {
                                ((ruby_width - base_width) / 2, I26Dot6::ZERO)
                            } else {
                                (I26Dot6::ZERO, (base_width - ruby_width) / 2)
                            };

                            annotations_max_ascender =
                                annotations_max_ascender.max(ruby_metrics.max_ascender);

                            // FIXME: Annotations seem to be slightly above where they should and
                            //        the logical rects also appear to be slightly too high.
                            annotation_segments.push(ShapedSegment {
                                glyphs: GlyphString::from_glyphs(text.clone(), glyphs),
                                baseline_offset: Point2::new(
                                    current_x + ruby_padding,
                                    current_line_y - extents.max_ascender,
                                ),
                                logical_rect: Rect2::new(
                                    Point2::new(-ruby_padding, -ruby_metrics.max_ascender),
                                    Point2::new(
                                        ruby_metrics.paint_size.x
                                            + ruby_metrics.trailing_advance
                                            + ruby_padding,
                                        -ruby_metrics.min_descender,
                                    ),
                                ),
                                corresponding_input_segment: annotation.input_index,
                            });

                            base_padding
                        } else {
                            I26Dot6::ZERO
                        };

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            let logical_width =
                                extents.paint_size.x + extents.trailing_advance + ruby_padding * 2;
                            segments.push(ShapedSegment {
                                glyphs,
                                baseline_offset: Point2::new(
                                    current_x + ruby_padding,
                                    current_line_y,
                                ),
                                logical_rect: Rect2::from_min_size(
                                    Point2::new(-ruby_padding, -extents.max_ascender),
                                    Vec2::new(logical_width, logical_height),
                                ),
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                            current_x += logical_width;
                        } else {
                            assert_eq!(
                                ruby_padding,
                                I26Dot6::ZERO,
                                "ruby bases cannot have internal segment splits"
                            );

                            loop {
                                let split_end = self
                                    .intra_font_segment_splits
                                    .get(current_intra_split)
                                    .copied()
                                    .unwrap_or(end);
                                let glyph_slice = match glyphs.split_off_until_cluster(split_end) {
                                    Some(string) => string,
                                    None => break,
                                };
                                let local_max_ascender = extents.max_ascender;
                                let extents =
                                    text::compute_extents_ex(true, glyph_slice.iter_glyphs())?;

                                segments.push(ShapedSegment {
                                    glyphs: glyph_slice,
                                    baseline_offset: Point2::new(current_x, current_line_y),
                                    logical_rect: Rect2::from_min_size(
                                        Point2::new(I26Dot6::ZERO, -local_max_ascender),
                                        Vec2::new(
                                            extents.paint_size.x + extents.trailing_advance,
                                            logical_height,
                                        ),
                                    ),
                                    corresponding_input_segment: current_segment
                                        + current_intra_split,
                                });
                                current_x += extents.paint_size.x + extents.trailing_advance;

                                if split_end >= end {
                                    break;
                                } else {
                                    current_intra_split += 1;
                                }
                            }
                        }
                    }
                }

                last = end;

                if end == line_boundary {
                    if post_wrap_glyphs.is_none() {
                        current_explicit_line += 1;
                    }
                    break;
                } else {
                    current_segment += 1;
                }
            }

            debug_assert_eq!(last, line_boundary);

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => I26Dot6::ZERO,
                HorizontalAlignment::Center => -current_x / 2,
                HorizontalAlignment::Right => -current_x,
            };

            let annotation_y_adjustment = if current_line_y == I26Dot6::ZERO {
                I26Dot6::ZERO
            } else {
                annotations_max_ascender
            };

            current_line_y += annotation_y_adjustment;

            for segment in segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                segment.baseline_offset.y += line_max_ascender + annotation_y_adjustment;
                segment.logical_rect = segment.logical_rect.translate(Vec2::new(
                    segment.baseline_offset.x,
                    current_line_y + line_max_ascender,
                ));
            }

            for segment in annotation_segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                segment.baseline_offset.y += line_max_ascender + annotation_y_adjustment;
                segment.logical_rect = segment
                    .logical_rect
                    .translate(segment.baseline_offset.to_vec());
            }

            let mut line_rect = Rect2::NOTHING;
            for segment in &segments {
                total_rect.expand_to_rect(segment.logical_rect);
                line_rect.expand_to_rect(segment.logical_rect);
            }

            current_line_y += line_max_ascender - line_min_descender + line_max_lineskip_descent;

            segments.append(&mut annotation_segments);

            lines.push(ShapedLine {
                segments,
                bounding_rect: line_rect,
            });
        }

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {total_rect:?} {lines:#?}");
        }

        Ok((lines, total_rect))
    }
}
