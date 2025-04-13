use crate::{
    math::{I26Dot6, I32Fixed, Point2, Rect2, Vec2},
    text::{self},
    util::RcArray,
    HorizontalAlignment, TextWrapMode,
};

use super::{FontArena, FontSelect, TextMetrics};

const MULTILINE_SHAPER_DEBUG_PRINT: bool = false;

enum Content<'a> {
    Text(TextContent<'a>),
    Shape(Rect2<i32>),
    None,
}

struct ShaperSegment<'a> {
    content: Content<'a>,
    end: usize,
}

struct TextContent<'a> {
    font: &'a text::Font,
    internal_breaks_allowed: bool,
    ruby_annotation: Option<Box<RubyAnnotation<'a>>>,
}

struct RubyAnnotation<'a> {
    font: &'a text::Font,
    input_index: usize,
    // Note: Text does not shape or form ligatures across ruby annotations or bases, even merged ones, due to bidi isolation. See § 3.5 Bidi Reordering and CSS Text 3 § 7.3 Shaping Across Element Boundaries.
    // ^^ This means we can treat all ruby annotation as completely separate pieces of text.
    text: &'a str,
}

pub struct MultilineTextShaper<'a> {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segments: Vec<ShaperSegment<'a>>,
    intra_font_segment_splits: Vec<usize>,

    ruby_annotations: Vec<RubyAnnotation<'a>>,
    ruby_annotation_text: String,
}

#[derive(Debug, Clone)]
pub struct ShapedSegment<'f> {
    pub glyphs: Option<RcArray<text::Glyph<'f>>>,
    pub baseline_offset: Point2<I26Dot6>,
    pub logical_rect: Rect2<I26Dot6>,
    pub corresponding_input_segment: usize,
    // Implementation details
    max_ascender: I26Dot6,
}

#[derive(Debug, Clone)]
pub struct ShapedLine<'f> {
    pub segments: Vec<ShapedSegment<'f>>,
    pub bounding_rect: Rect2<I26Dot6>,
}

fn shape_simple_segment<'f>(
    font: &text::Font,
    text: &str,
    range: impl text::ItemRange,
    font_arena: &'f FontArena,
    font_select: &mut FontSelect,
) -> (Vec<text::Glyph<'f>>, TextMetrics) {
    let glyphs = {
        let mut buffer = text::ShapingBuffer::new();
        buffer.reset();
        buffer.add(text, range);
        let direction = buffer.guess_properties();
        if !direction.is_horizontal() {
            buffer.set_direction(direction.to_horizontal());
        }
        buffer.shape(font, font_arena, font_select)
    };

    let metrics = text::compute_extents_ex(true, &glyphs);

    (glyphs, metrics)
}

fn calculate_multi_font_metrics(fonts: &[text::Font]) -> (I26Dot6, I26Dot6, I26Dot6) {
    let mut max_ascender = I26Dot6::MIN;
    let mut min_descender = I26Dot6::MAX;
    let mut max_lineskip_descent = I26Dot6::MIN;

    for font in fonts {
        let metrics = font.metrics();
        let lineskip_descent = metrics.height - metrics.ascender;

        max_ascender = max_ascender.max(metrics.ascender);
        min_descender = min_descender.min(metrics.descender);
        max_lineskip_descent = max_lineskip_descent.max(lineskip_descent);
    }

    (max_ascender, min_descender, max_lineskip_descent)
}

#[derive(Debug, Clone, Copy)]
pub struct TextWrapParams {
    pub mode: TextWrapMode,
    pub wrap_width: f32,
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

impl<'a> MultilineTextShaper<'a> {
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segments: Vec::new(),
            intra_font_segment_splits: Vec::new(),
            ruby_annotation_text: String::new(),
            ruby_annotations: Vec::new(),
        }
    }

    pub fn add_text(&mut self, mut text: &str, font: &'a text::Font) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT TEXT: {:?} {:?}", text, font);
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
                    font: last_font,
                    internal_breaks_allowed: true,
                    ruby_annotation: None,
                }),
            end: ref mut last_end,
        }) = self.segments.last_mut()
        {
            if last_font == font {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font,
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

    pub fn add_ruby_base(&mut self, text: &str, font: &'a text::Font) -> RubyBaseId {
        let id = self.segments.len();

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT RUBY BASE[{id}]: {:?} {:?}", text, font);
        }

        self.text.push_str(text);
        self.segments.push(ShaperSegment {
            content: Content::Text(TextContent {
                font,
                internal_breaks_allowed: false,
                ruby_annotation: None,
            }),
            end: self.text.len(),
        });

        RubyBaseId(id)
    }

    pub fn add_ruby_annotation(&mut self, base: RubyBaseId, text: &'a str, font: &'a text::Font) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!(
                "SHAPING V2 INPUT RUBY ANNOTATION FOR {}: {:?} {:?}",
                base.0, text, font
            );
        }

        let index = self.segments.len() + self.intra_font_segment_splits.len();

        let ShaperSegment {
            content:
                Content::Text(TextContent {
                    font: _,
                    internal_breaks_allowed: false,
                    ruby_annotation: ref mut ruby_annotation @ None,
                }),
            ..
        } = self.segments[base.0]
        else {
            panic!("ruby annotation placed on non-ruby base segment or one that already has an annotation in multiline shaper");
        };

        *ruby_annotation = Some(Box::new(RubyAnnotation {
            font,
            input_index: index,
            text,
        }));
        self.skip_segment_for_output();
    }

    pub fn add_shape(&mut self, dim: Rect2<i32>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT SHAPE: {dim:?}",);
        }

        self.text.push('\0');
        self.segments.push(ShaperSegment {
            content: Content::Shape(dim),
            end: self.text.len(),
        })
    }

    pub fn shape<'f>(
        &self,
        line_alignment: HorizontalAlignment,
        wrap: TextWrapParams,
        font_arena: &'f FontArena,
        font_select: &mut FontSelect,
    ) -> (Vec<ShapedLine<'f>>, Rect2<I26Dot6>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 TEXT {:?}", self.text);
            println!(
                "SHAPING V2 LINE BOUNDARIES {:?}",
                self.explicit_line_bounaries
            );
        }

        let wrap_width = I32Fixed::from_f32(wrap.wrap_width);

        let mut lines: Vec<ShapedLine> = vec![];
        let mut current_line_y = I26Dot6::ZERO;
        let mut total_rect = Rect2::NOTHING;

        let mut current_explicit_line = 0;
        let mut current_segment = 0;
        let mut current_intra_split = 0;
        let mut last = 0;
        let mut post_wrap_skip = 0;
        while current_explicit_line <= self.explicit_line_bounaries.len() {
            let mut line_boundary = self
                .explicit_line_bounaries
                .get(current_explicit_line)
                .copied()
                .unwrap_or(self.text.len());
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

            let mut line_max_ascender = I32Fixed::ZERO;
            let mut line_min_descender = I32Fixed::ZERO;
            let mut line_max_lineskip_descent = I32Fixed::ZERO;

            while self.segments[current_segment].end <= last {
                current_segment += 1;
            }

            let mut did_wrap = false;

            'segment_shaper: loop {
                let ShaperSegment {
                    content: ref segment,
                    end: font_boundary,
                } = self.segments[current_segment];

                let end = font_boundary.min(line_boundary);
                let segment_slice = last..end;

                match segment {
                    Content::None => {}
                    &Content::Text(TextContent {
                        font,
                        internal_breaks_allowed,
                        ref ruby_annotation,
                    }) => {
                        let (glyphs, extents) = shape_simple_segment(
                            font,
                            &self.text,
                            segment_slice,
                            &font_arena,
                            font_select,
                        );

                        if !did_wrap
                            && wrap.mode == TextWrapMode::Normal
                            // TODO: breaking before a box is always legal
                            && internal_breaks_allowed
                            && current_x + extents.paint_size.x > wrap_width
                        {
                            let mut x = extents.paint_size.x;
                            let mut glyph_it = glyphs.iter();
                            if let Some(first) = glyph_it.next() {
                                x += first.x_advance;
                            }

                            for glyph in glyph_it {
                                let x_after = x
                                    + glyph.x_offset
                                    + glyph.font.glyph_extents(glyph.index).width;
                                // TODO: also ensure conformance with unicode line breaking
                                //       this would ensure, for example, that no line
                                //       breaks are inserted between a character and
                                //       a combining mark and that word joiners are
                                //       respected.
                                if x_after > wrap_width {
                                    let idx = glyph.cluster;
                                    let break_at = {
                                        // FIXME: What to do about words that are partially rendered with
                                        //        different fonts? This will not handle those properly because
                                        //        they will be in different segments...
                                        //        This would basically require backtracking and reshaping across
                                        //        segment boundaries.
                                        match self.text[last..idx].rfind(' ') {
                                            Some(off) => {
                                                post_wrap_skip = 1;
                                                last + off
                                            }
                                            None => {
                                                // No spaces available for breaking, render the whole word.
                                                match self.text[idx..end].find(' ') {
                                                    Some(off) => {
                                                        post_wrap_skip = 1;
                                                        idx + off
                                                    }
                                                    None => break,
                                                }
                                            }
                                        }
                                    };

                                    did_wrap = true;
                                    line_boundary = break_at;

                                    if line_boundary == last {
                                        break 'segment_shaper;
                                    }

                                    // Reshape this segment with the new line boundary
                                    continue 'segment_shaper;
                                }
                                x += glyph.x_advance;
                            }
                            // the text is made up of one glyph and we can't
                            // split it up, go through with rendering it.
                        }

                        let rc_glyphs = RcArray::from_boxed(glyphs.into_boxed_slice());

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
                                annotation.font,
                                annotation.text,
                                ..,
                                font_arena,
                                font_select,
                            );

                            let base_width = extents.paint_size.x + extents.trailing_advance;
                            let ruby_width =
                                ruby_metrics.paint_size.x + ruby_metrics.trailing_advance;
                            let (base_padding, ruby_padding) = if ruby_width > base_width {
                                ((ruby_width - base_width) / 2, I26Dot6::ZERO)
                            } else {
                                (I26Dot6::ZERO, (base_width - ruby_width) / 2)
                            };

                            // FIXME: Annotations seem to be slightly above where they should and
                            //        the logical rects also appear to be slightly to high.
                            annotation_segments.push(ShapedSegment {
                                glyphs: Some(RcArray::from_boxed(glyphs.into_boxed_slice())),
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
                                max_ascender: ruby_metrics.max_ascender,
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
                                glyphs: Some(rc_glyphs),
                                baseline_offset: Point2::new(
                                    current_x + ruby_padding,
                                    current_line_y,
                                ),
                                logical_rect: Rect2::from_min_size(
                                    Point2::new(-ruby_padding, I26Dot6::ZERO),
                                    Vec2::new(logical_width, logical_height),
                                ),
                                max_ascender: extents.max_ascender,
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                            current_x += logical_width;
                        } else {
                            assert_eq!(
                                ruby_padding,
                                I26Dot6::ZERO,
                                "ruby bases cannot have internal segment splits"
                            );

                            let mut last_glyph_idx = 0;

                            loop {
                                let split_end = self
                                    .intra_font_segment_splits
                                    .get(current_intra_split)
                                    .copied()
                                    .unwrap_or(end);
                                let end_glyph_idx = rc_glyphs
                                    .iter()
                                    .position(|x| x.cluster >= split_end)
                                    .unwrap_or(rc_glyphs.len());
                                let glyph_range = last_glyph_idx..end_glyph_idx;
                                let glyph_slice = RcArray::slice(rc_glyphs.clone(), glyph_range);
                                let local_max_ascender = extents.max_ascender;
                                let extents = text::compute_extents_ex(true, &glyph_slice);

                                segments.push(ShapedSegment {
                                    glyphs: Some(glyph_slice),
                                    baseline_offset: Point2::new(current_x, current_line_y),
                                    logical_rect: Rect2::from_min_size(
                                        Point2::ZERO,
                                        Vec2::new(
                                            extents.paint_size.x + extents.trailing_advance,
                                            logical_height,
                                        ),
                                    ),
                                    max_ascender: local_max_ascender,
                                    corresponding_input_segment: current_segment
                                        + current_intra_split,
                                });
                                last_glyph_idx = end_glyph_idx;
                                current_x += extents.paint_size.x + extents.trailing_advance;

                                if split_end >= end {
                                    break;
                                } else {
                                    current_intra_split += 1;
                                }
                            }
                        }
                    }
                    // TODO: Figure out exactly how ass shape layout should work
                    Content::Shape(dim) => {
                        let logical_w = dim.width() - (-dim.min.x).min(0);
                        let logical_h = dim.height() - (-dim.min.y).min(0);
                        let segment_max_bearing_y = I32Fixed::new(logical_h);
                        segments.push(ShapedSegment {
                            glyphs: None,
                            baseline_offset: Point2::new(current_x, current_line_y),
                            logical_rect: Rect2::new(
                                Point2::ZERO,
                                Point2::new(I26Dot6::new(logical_w), I26Dot6::new(logical_h)),
                            ),
                            corresponding_input_segment: current_segment + current_intra_split,
                            max_ascender: segment_max_bearing_y,
                        });
                        current_x += logical_w;
                        line_max_ascender = line_max_ascender.max(segment_max_bearing_y);
                    }
                }

                last = end;

                if end == line_boundary {
                    if !did_wrap {
                        current_explicit_line += 1;
                    }
                    break;
                } else {
                    current_segment += 1;
                }
            }

            debug_assert_eq!(last, line_boundary);

            last += post_wrap_skip;
            post_wrap_skip = 0;

            for segment in segments.iter_mut().rev() {
                match &segment.glyphs {
                    Some(glyphs) => {
                        if let Some(last) = glyphs.last() {
                            let extents = last.font.glyph_extents(last.index);
                            let trailing_advance = last.x_advance - extents.width;
                            current_x -= trailing_advance;
                            segment.logical_rect.max.x -= trailing_advance;
                            break;
                        }
                    }
                    None => break,
                }
            }

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => I26Dot6::ZERO,
                HorizontalAlignment::Center => -current_x / 2,
                HorizontalAlignment::Right => -current_x,
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                if segment.glyphs.is_none() {
                    segment.baseline_offset.y += line_max_ascender - segment.max_ascender;
                } else {
                    segment.baseline_offset.y += line_max_ascender;
                }
                segment.logical_rect = segment.logical_rect.translate(Vec2::new(
                    segment.baseline_offset.x,
                    current_line_y + line_max_ascender - segment.max_ascender,
                ));
            }

            for segment in annotation_segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                segment.baseline_offset.y += line_max_ascender;
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
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}
