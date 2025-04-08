use crate::{
    math::{I26Dot6, I32Fixed, Point2, Rect2, Vec2},
    text::{self},
    util::RcArray,
    HorizontalAlignment, TextWrapMode,
};

use super::{FontArena, FontSelect, TextMetrics};

const MULTILINE_SHAPER_DEBUG_PRINT: bool = false;

enum ShaperSegment {
    Text {
        font: text::Font,
        line_breaking_forbidden: bool,
    },
    Shape(Rect2<i32>),
    Skipped,
}

struct RubyAnnotationSegment {
    starting_base_segment_index: usize,
    font: text::Font,
    input_index: usize,
    text_end: usize,
    next_continues_container: bool,
}

pub struct MultilineTextShaper {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segment_boundaries: Vec<(ShaperSegment, /* end of segment i */ usize)>,
    intra_font_segment_splits: Vec<usize>,

    ruby_annotations: Vec<RubyAnnotationSegment>,
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
        let ascender = I26Dot6::from_ft(metrics.ascender);
        let descender = I26Dot6::from_ft(metrics.descender);
        let lineskip_descent = I26Dot6::from_ft(metrics.height - metrics.ascender);

        max_ascender = max_ascender.max(ascender);
        min_descender = min_descender.min(descender);
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
// TODO: Currently annotations are not taken into account when line wrapping,
//       and ruby bases are not centered when annotations overflow them.
// TODO: Overlapping rubies are currently handled by joining and centering the
//       text above them, this is not the correct solution.
//       Instead the underlying bases should be spaced out, all this means that ruby
//       layout in general should be integrated into the main loop somehow.
//       Possibly by actually having the "we're laying out *multiple rows of text at once*"
//       concept going.
//       Note that the currently solution also works only one way, i.e. annotations are only
//       merged in a single pass towards the right, which misses some cases.

impl MultilineTextShaper {
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segment_boundaries: Vec::new(),
            intra_font_segment_splits: Vec::new(),
            ruby_annotation_text: String::new(),
            ruby_annotations: Vec::new(),
        }
    }

    pub fn add_text(&mut self, mut text: &str, font: &text::Font) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT TEXT: {:?} {:?}", text, font);
        }

        while let Some(nl) = text.find('\n') {
            self.text.push_str(&text[..nl]);
            self.explicit_line_bounaries.push(self.text.len());
            text = &text[nl + 1..];
        }
        self.text.push_str(text);

        if let Some((
            ShaperSegment::Text {
                font: ref last_font,
                line_breaking_forbidden: false,
            },
            ref mut last_end,
        )) = self.segment_boundaries.last_mut()
        {
            if last_font == font {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segment_boundaries.push((
            ShaperSegment::Text {
                font: font.clone(),
                line_breaking_forbidden: false,
            },
            self.text.len(),
        ));
    }

    // TODO: Maybe a better system should be devised than this.
    //       Potentially just track an arbitrary `usize` provided as input for each segment,
    //       would require some restructuring.
    pub fn skip_segment_for_output(&mut self) {
        self.segment_boundaries
            .push((ShaperSegment::Skipped, self.text.len()));
    }

    pub fn add_ruby_base(&mut self, text: &str, font: &text::Font) -> RubyBaseId {
        let id = self.segment_boundaries.len() + self.intra_font_segment_splits.len();
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT RUBY BASE[{id}]: {:?} {:?}", text, font);
        }

        self.text.push_str(text);
        self.segment_boundaries.push((
            ShaperSegment::Text {
                font: font.clone(),
                line_breaking_forbidden: true,
            },
            self.text.len(),
        ));

        RubyBaseId(id)
    }

    pub fn add_ruby_annotation(
        &mut self,
        base: RubyBaseId,
        text: &str,
        font: &text::Font,
        next_continues_container: bool,
    ) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!(
                "SHAPING V2 INPUT RUBY ANNOTATION AT {}: {:?} {:?}",
                base.0, text, font
            );
        }

        self.ruby_annotation_text.push_str(text);
        self.ruby_annotations.push(RubyAnnotationSegment {
            font: font.clone(),
            starting_base_segment_index: base.0,
            input_index: self.segment_boundaries.len() + self.intra_font_segment_splits.len(),
            text_end: self.ruby_annotation_text.len(),
            next_continues_container,
        });
        self.skip_segment_for_output();
    }

    pub fn add_shape(&mut self, dim: Rect2<i32>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT SHAPE: {dim:?}",);
        }

        self.text.push('\0');
        self.segment_boundaries
            .push((ShaperSegment::Shape(dim), self.text.len()))
    }

    fn layout_line_ruby<'f>(
        &self,
        segments: &mut Vec<ShapedSegment<'f>>,
        annotations: &[RubyAnnotationSegment],
        font_arena: &'f FontArena,
        font_select: &mut FontSelect,
    ) {
        let ruby_segments_start = segments.len();

        let mut current_segment = 0;
        let mut current_text_cursor = 0;
        let mut current_annotation = 0;
        while current_segment < segments.len() && current_annotation < annotations.len() {
            let first_base = &segments[current_segment];
            let annotation = &annotations[current_annotation];
            if segments[current_segment].corresponding_input_segment
                == annotation.starting_base_segment_index
            {
                let start_x = first_base.logical_rect.min.x;
                let end_x = first_base.logical_rect.max.x;

                let baseline_y = first_base.baseline_offset.y - first_base.max_ascender;

                let (glyphs, extents) = shape_simple_segment(
                    &annotation.font,
                    &self.ruby_annotation_text[current_text_cursor..annotation.text_end],
                    ..,
                    font_arena,
                    font_select,
                );

                segments.push(ShapedSegment {
                    glyphs: Some(RcArray::from_boxed(glyphs.into_boxed_slice())),
                    baseline_offset: Point2::new(
                        // This is only temporarily stored here, and read during the
                        // second pass below.
                        extents.paint_size.x + extents.trailing_advance,
                        baseline_y,
                    ),
                    logical_rect: Rect2::new(
                        Point2::new(start_x, baseline_y - extents.max_ascender),
                        Point2::new(end_x, baseline_y - extents.min_descender),
                    ),
                    max_ascender: extents.max_ascender,
                    corresponding_input_segment: annotation.input_index,
                });

                current_annotation += 1;
                current_text_cursor = annotation.text_end;
            }
            current_segment += 1;
        }

        let mut current = ruby_segments_start;
        while let Some(first) = segments.get(current) {
            let start_x = first.logical_rect.min.x;
            let mut total_width = I26Dot6::ZERO;

            let merged_start = current;
            loop {
                let segment = &segments[current];
                total_width += segment.baseline_offset.x;
                let centered_end = (start_x + segment.logical_rect.max.x) / 2 + total_width / 2;
                current += 1;
                if current >= segments.len() || centered_end < segments[current].logical_rect.min.x
                {
                    break;
                }
            }

            let merged = &mut segments[merged_start..current];
            let total_space = merged.last().unwrap().logical_rect.max.x - start_x;
            let centering_pad = (total_space - total_width) / 2;

            let mut last_end = start_x + centering_pad;
            for next in merged {
                let width = next.baseline_offset.x;
                next.baseline_offset.x = last_end;
                next.logical_rect.min.x = next.logical_rect.min.x.min(last_end);
                last_end += width;
                next.logical_rect.max.x = next.logical_rect.max.x.max(last_end);
            }
        }
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
        let mut current_ruby_annotation = 0;
        let mut last = 0;
        let mut post_wrap_skip = 0;
        while current_explicit_line <= self.explicit_line_bounaries.len() {
            let mut line_boundary = self
                .explicit_line_bounaries
                .get(current_explicit_line)
                .copied()
                .unwrap_or(self.text.len());
            let mut segments: Vec<ShapedSegment> = vec![];
            let mut current_x = I26Dot6::ZERO;

            if MULTILINE_SHAPER_DEBUG_PRINT {
                println!(
                    "last: {last}, font boundaries: {:?}, current segment: {}",
                    self.segment_boundaries
                        .iter()
                        .map(|(_, s)| s)
                        .collect::<Vec<_>>(),
                    current_segment
                );
            }

            let mut line_max_ascender = I32Fixed::ZERO;
            let mut line_min_descender = I32Fixed::ZERO;
            let mut line_max_lineskip_descent = I32Fixed::ZERO;

            while self.segment_boundaries[current_segment].1 <= last {
                current_segment += 1;
            }

            let mut did_wrap = false;

            'segment_shaper: loop {
                let (ref segment, font_boundary) = self.segment_boundaries[current_segment];

                let end = font_boundary.min(line_boundary);
                let segment_slice = last..end;

                match segment {
                    ShaperSegment::Skipped => {}
                    ShaperSegment::Text {
                        font,
                        line_breaking_forbidden,
                    } => {
                        let (glyphs, extents) = shape_simple_segment(
                            font,
                            &self.text,
                            segment_slice,
                            &font_arena,
                            font_select,
                        );

                        if !did_wrap
                            && wrap.mode == TextWrapMode::Normal
                            && !*line_breaking_forbidden
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

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            segments.push(ShapedSegment {
                                glyphs: Some(rc_glyphs),
                                baseline_offset: Point2::new(current_x, current_line_y),
                                logical_rect: Rect2::from_min_size(
                                    Point2::ZERO,
                                    Vec2::new(
                                        extents.paint_size.x + extents.trailing_advance,
                                        logical_height,
                                    ),
                                ),
                                max_ascender: extents.max_ascender,
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                            current_x += extents.paint_size.x + extents.trailing_advance;
                        } else {
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
                    // TODO: Figure out exactly how libass lays out shapes
                    ShaperSegment::Shape(dim) => {
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

            {
                let line_ruby_annotations_start = current_ruby_annotation;
                while self
                    .ruby_annotations
                    .get(current_ruby_annotation)
                    .is_some_and(|annotation| {
                        annotation.starting_base_segment_index
                            <= current_segment + current_intra_split
                    })
                {
                    current_ruby_annotation += 1;
                }

                self.layout_line_ruby(
                    &mut segments,
                    &self.ruby_annotations[line_ruby_annotations_start..current_ruby_annotation],
                    &font_arena,
                    font_select,
                );
            }

            let mut line_rect = Rect2::NOTHING;
            for segment in &segments {
                total_rect.expand_to_rect(segment.logical_rect);
                line_rect.expand_to_rect(segment.logical_rect);
            }

            current_line_y += line_max_ascender - line_min_descender + line_max_lineskip_descent;

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
