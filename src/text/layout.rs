use std::rc::Rc;

use crate::{
    math::{I32Fixed, Point2, Rect2, Vec2},
    text::{self, ft_utils::IFixed26Dot6},
    util::RcArray,
    HorizontalAlignment, TextWrapMode,
};

use super::{FontSelect, TextExtents};

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
    num_bases: usize,
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

pub struct ShapedSegment {
    // TODO: Make this Rc<[text::Font]>
    pub glyphs_and_fonts: Option<(RcArray<text::Glyph>, Rc<Vec<text::Font>>)>,
    pub baseline_offset: Point2<I32Fixed<6>>,
    pub logical_rect: Rect2<I32Fixed<6>>,
    pub corresponding_input_segment: usize,
    // Implementation details
    max_ascender: I32Fixed<6>,
}

impl std::fmt::Debug for ShapedSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapedSegment")
            .field(
                "start_of_glyphs",
                &self
                    .glyphs_and_fonts
                    .as_ref()
                    .unwrap()
                    .0
                    .first()
                    .unwrap()
                    .index,
            )
            .field("baseline_offset", &self.baseline_offset)
            .field("logical_rect", &self.logical_rect)
            .field(
                "corresponding_input_segment",
                &self.corresponding_input_segment,
            )
            .field("max_ascender", &self.max_ascender)
            .finish()
    }
}

#[derive(Debug)]
pub struct ShapedLine {
    pub segments: Vec<ShapedSegment>,
}

struct SimpleShapedTextSegment {
    glyphs: Vec<text::Glyph>,
    fonts: Vec<text::Font>,
    extents: TextExtents,
    trailing_x_advance: IFixed26Dot6,
}

impl SimpleShapedTextSegment {
    fn shape(font: &text::Font, text: &str, font_select: &mut FontSelect) -> Self {
        let mut fonts = Vec::new();
        let glyphs = {
            let mut buffer = text::ShapingBuffer::new();
            let direction = buffer.guess_properties();
            if !direction.is_horizontal() {
                buffer.set_direction(direction.to_horizontal());
            }
            buffer.add(text);
            buffer.shape(font, &mut fonts, font_select)
        };

        let (extents, (trailing_x_advance, _)) = text::compute_extents_ex(true, &fonts, &glyphs);

        SimpleShapedTextSegment {
            glyphs,
            fonts,
            extents,
            trailing_x_advance,
        }
    }
}

fn calculate_multi_font_metrics(
    fonts: &[text::Font],
) -> (IFixed26Dot6, IFixed26Dot6, IFixed26Dot6) {
    let mut max_ascender = IFixed26Dot6::MIN;
    let mut min_descender = IFixed26Dot6::MAX;
    let mut max_lineskip_descent = IFixed26Dot6::MIN;

    for font in fonts {
        let metrics = font.metrics();
        let ascender = IFixed26Dot6::from_ft(metrics.ascender);
        let descender = IFixed26Dot6::from_ft(metrics.descender);
        let lineskip_descent = IFixed26Dot6::from_ft(metrics.height - metrics.ascender);

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
// TODO: default ruby-align is "space-around", this means justification with extra
//       justification opportunities at the start and end of the text
//       justification is not yet implemented, implement it. (
//         with the generic "justification system" to make it simpler to do this,
//         it should probably work like MultilineTextShaper except it accepts glyphstrings?
//       )
// Chromium seems to lay out ruby text at the top of the current entire line box,
// so we do it like that too.
// TODO: Currently annotations are not taken into account when line wrapping,
//       and ruby bases are not centered when annotations overflow them.

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
        length: usize,
        text: &str,
        font: &text::Font,
        next_continues_container: bool,
    ) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!(
                "SHAPING V2 INPUT RUBY ANNOTATION AT {} SPANNING {length}: {:?} {:?}",
                base.0, text, font
            );
        }

        self.ruby_annotation_text.push_str(text);
        self.ruby_annotations.push(RubyAnnotationSegment {
            font: font.clone(),
            starting_base_segment_index: base.0,
            input_index: self.segment_boundaries.len() + self.intra_font_segment_splits.len(),
            num_bases: length,
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

    fn layout_line_ruby(
        &self,
        segments: &mut Vec<ShapedSegment>,
        annotations: &[RubyAnnotationSegment],
        font_select: &mut FontSelect,
        line_ascender: IFixed26Dot6,
    ) {
        let non_ruby_segment_len = segments.len();
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
                let mut end_x = IFixed26Dot6::MIN;

                // There's a ruby annotation that starts above this segment.
                // and it spans num_bases, figure out how much space we have
                // spanning the bases.
                for base in &segments[current_segment..non_ruby_segment_len] {
                    if base.corresponding_input_segment
                        >= annotation.starting_base_segment_index + annotation.num_bases
                        || base.corresponding_input_segment < annotation.starting_base_segment_index
                    {
                        break;
                    }

                    end_x = base.logical_rect.max.x;
                }

                let baseline_y = first_base.baseline_offset.y - line_ascender;

                let shaped = SimpleShapedTextSegment::shape(
                    &annotation.font,
                    &self.ruby_annotation_text[current_text_cursor..annotation.text_end],
                    font_select,
                );

                let (max_asc, min_desc, _) = calculate_multi_font_metrics(&shaped.fonts);

                segments.push(ShapedSegment {
                    glyphs_and_fonts: Some((
                        RcArray::from_boxed(shaped.glyphs.into_boxed_slice()),
                        shaped.fonts.into(),
                    )),
                    baseline_offset: Point2::new(
                        // TODO: Make this store x extent and fill in during justification
                        (start_x + end_x) / 2 - shaped.extents.paint_width / 2,
                        baseline_y,
                    ),
                    logical_rect: Rect2::new(
                        Point2::new(start_x, baseline_y - max_asc),
                        Point2::new(end_x, baseline_y - min_desc),
                    ),
                    max_ascender: max_asc,
                    corresponding_input_segment: annotation.input_index,
                });

                current_annotation += 1;
                current_text_cursor = annotation.text_end;
            }
            current_segment += 1;
        }
    }

    pub fn shape(
        &self,
        line_alignment: HorizontalAlignment,
        wrap: TextWrapParams,
        font_select: &mut FontSelect,
    ) -> (Vec<ShapedLine>, Rect2<IFixed26Dot6>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 TEXT {:?}", self.text);
            println!(
                "SHAPING V2 LINE BOUNDARIES {:?}",
                self.explicit_line_bounaries
            );
        }

        let wrap_width = I32Fixed::from_f32(wrap.wrap_width);

        let mut lines: Vec<ShapedLine> = vec![];
        let mut total_extents = TextExtents {
            paint_width: I32Fixed::ZERO,
            paint_height: I32Fixed::ZERO,
            max_bearing_y: IFixed26Dot6::ZERO,
        };
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
            let mut line_extents = TextExtents {
                paint_height: I32Fixed::ZERO,
                paint_width: I32Fixed::ZERO,
                max_bearing_y: IFixed26Dot6::ZERO,
            };

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
                        let SimpleShapedTextSegment {
                            glyphs,
                            fonts: segment_fonts,
                            extents,
                            trailing_x_advance,
                        } = SimpleShapedTextSegment::shape(
                            font,
                            &self.text[segment_slice],
                            font_select,
                        );

                        if !did_wrap
                            && wrap.mode == TextWrapMode::Normal
                            && !*line_breaking_forbidden
                            && line_extents.paint_width + extents.paint_width > wrap_width
                        {
                            let mut x = line_extents.paint_width;
                            let mut glyph_it = glyphs.iter();
                            if let Some(first) = glyph_it.next() {
                                x += first.x_advance;
                            }

                            for glyph in glyph_it {
                                let x_after = x
                                    + glyph.x_offset
                                    + segment_fonts[glyph.font_index]
                                        .glyph_extents(glyph.index)
                                        .width;
                                // TODO: also ensure conformance with unicode line breaking
                                //       this would ensure, for example, that no line
                                //       breaks are inserted between a character and
                                //       a combining mark and that word joiners are
                                //       respected.
                                if x_after > wrap_width {
                                    let idx = last + glyph.cluster;
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

                        let segment_fonts = Rc::new(segment_fonts);
                        let rc_glyphs = RcArray::from_boxed(glyphs.into_boxed_slice());

                        if MULTILINE_SHAPER_DEBUG_PRINT {
                            println!(
                                "last: {last}, end: {end}, intra font splits: {:?}, current intra split: {}",
                                self.intra_font_segment_splits, current_intra_split
                            );
                        }

                        let (local_max_ascender, local_min_descender, local_lineskip_descent) =
                            calculate_multi_font_metrics(&segment_fonts);

                        line_max_ascender = line_max_ascender.max(local_max_ascender);
                        line_min_descender = line_min_descender.min(local_min_descender);
                        line_max_lineskip_descent =
                            line_max_lineskip_descent.max(local_lineskip_descent);

                        let logical_height = local_max_ascender - local_min_descender;

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            segments.push(ShapedSegment {
                                glyphs_and_fonts: Some((rc_glyphs, segment_fonts)),
                                baseline_offset: Point2::new(
                                    line_extents.paint_width,
                                    total_extents.paint_height,
                                ),
                                logical_rect: Rect2::from_min_size(
                                    Point2::ZERO,
                                    Vec2::new(
                                        extents.paint_width + trailing_x_advance,
                                        logical_height,
                                    ),
                                ),
                                max_ascender: local_max_ascender,
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                        } else {
                            let mut last_glyph_idx = 0;
                            let mut x = line_extents.paint_width;

                            loop {
                                let split_end = self
                                    .intra_font_segment_splits
                                    .get(current_intra_split)
                                    .copied()
                                    .unwrap_or(end);
                                let end_glyph_idx = rc_glyphs
                                    .iter()
                                    .position(|x| x.cluster >= split_end - last)
                                    .unwrap_or(rc_glyphs.len());
                                let glyph_range = last_glyph_idx..end_glyph_idx;
                                let glyph_slice = RcArray::slice(rc_glyphs.clone(), glyph_range);
                                let (extents, (x_advance, _)) =
                                    text::compute_extents_ex(true, &segment_fonts, &glyph_slice);

                                segments.push(ShapedSegment {
                                    glyphs_and_fonts: Some((glyph_slice, segment_fonts.clone())),
                                    baseline_offset: Point2::new(x, total_extents.paint_height),
                                    logical_rect: Rect2::from_min_size(
                                        Point2::ZERO,
                                        Vec2::new(extents.paint_width + x_advance, logical_height),
                                    ),
                                    max_ascender: local_max_ascender,
                                    corresponding_input_segment: current_segment
                                        + current_intra_split,
                                });
                                last_glyph_idx = end_glyph_idx;
                                x += extents.paint_width + x_advance;

                                if split_end >= end {
                                    break;
                                } else {
                                    current_intra_split += 1;
                                }
                            }
                        }

                        line_extents.paint_width += extents.paint_width;

                        // FIXME: THIS IS WRONG!!
                        //        It will add trailing advance when the last segments contains zero or only invisible glyphs.
                        if end != line_boundary {
                            line_extents.paint_width += trailing_x_advance;
                        }

                        if line_extents.paint_height < extents.paint_height {
                            line_extents.paint_height = extents.paint_height;
                        }

                        line_extents.max_bearing_y =
                            line_extents.max_bearing_y.max(extents.max_bearing_y);
                    }
                    // TODO: Figure out exactly how libass lays out shapes
                    ShaperSegment::Shape(dim) => {
                        let logical_w = dim.width() - (-dim.min.x).min(0);
                        let logical_h = dim.height() - (-dim.min.y).min(0);
                        let segment_max_bearing_y = I32Fixed::new(logical_h);
                        segments.push(ShapedSegment {
                            glyphs_and_fonts: None,
                            baseline_offset: Point2::new(
                                line_extents.paint_width,
                                total_extents.paint_height,
                            ),
                            logical_rect: Rect2::new(
                                Point2::ZERO,
                                Point2::new(
                                    IFixed26Dot6::new(logical_w),
                                    IFixed26Dot6::new(logical_h),
                                ),
                            ),
                            corresponding_input_segment: current_segment + current_intra_split,
                            max_ascender: segment_max_bearing_y,
                        });
                        line_extents.paint_width += logical_w;
                        line_max_ascender = line_max_ascender.max(segment_max_bearing_y);
                        line_extents.paint_height =
                            line_extents.paint_height.max(I32Fixed::new(logical_h));
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

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => IFixed26Dot6::ZERO,
                HorizontalAlignment::Center => -line_extents.paint_width / 2,
                HorizontalAlignment::Right => -line_extents.paint_width,
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.x += aligning_x_offset;
                if segment.glyphs_and_fonts.is_none() {
                    segment.baseline_offset.y += line_max_ascender - segment.max_ascender;
                } else {
                    segment.baseline_offset.y += line_max_ascender;
                }
                segment.logical_rect = segment.logical_rect.translate(Vec2::new(
                    segment.baseline_offset.x,
                    line_max_ascender - segment.max_ascender,
                ));
            }

            {
                let line_ruby_annotations_start = current_ruby_annotation;
                while self
                    .ruby_annotations
                    .get(current_ruby_annotation)
                    .is_some_and(|annotation| {
                        annotation.starting_base_segment_index
                            < current_segment + current_intra_split
                    })
                {
                    current_ruby_annotation += 1;
                }

                self.layout_line_ruby(
                    &mut segments,
                    &self.ruby_annotations[line_ruby_annotations_start..current_ruby_annotation],
                    font_select,
                    line_max_ascender,
                );
            }

            for segment in &segments {
                total_rect.expand_to_rect(segment.logical_rect);
            }

            total_extents.paint_height += line_max_ascender - line_min_descender;
            if line_boundary != self.text.len() {
                total_extents.paint_height += line_max_lineskip_descent;
            }

            total_extents.paint_width = total_extents.paint_width.max(line_extents.paint_width);

            lines.push(ShapedLine { segments });
        }

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}
