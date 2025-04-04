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
    Text(text::Font),
    Shape(Rect2<i32>),
}

pub struct MultilineTextShaper {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segment_boundaries: Vec<(ShaperSegment, /* end of segment i */ usize)>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug)]
pub struct ShapedLineSegment {
    pub glyphs_and_fonts: Option<(RcArray<text::Glyph>, Rc<Vec<text::Font>>)>,
    pub baseline_offset: Point2<I32Fixed<6>>,
    pub logical_rect: Rect2<I32Fixed<6>>,
    pub corresponding_input_segment: usize,
    // Implementation details
    max_ascender: I32Fixed<6>,
    corresponding_font_boundary: usize,
}

#[derive(Debug)]
pub struct ShapedLine {
    pub segments: Vec<ShapedLineSegment>,
}

#[derive(Debug, Clone, Copy)]
pub struct TextWrapParams {
    pub mode: TextWrapMode,
    pub wrap_width: f32,
}

impl MultilineTextShaper {
    pub const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segment_boundaries: Vec::new(),
            intra_font_segment_splits: Vec::new(),
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

        if let Some((ShaperSegment::Text(ref last_font), ref mut last_end)) =
            self.segment_boundaries.last_mut()
        {
            if last_font == font {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segment_boundaries
            .push((ShaperSegment::Text(font.clone()), self.text.len()));
    }

    pub fn add_shape(&mut self, dim: Rect2<i32>) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT SHAPE: {dim:?}",);
        }

        self.text.push('\0');
        self.segment_boundaries
            .push((ShaperSegment::Shape(dim), self.text.len()))
    }

    pub fn shape(
        &self,
        line_alignment: HorizontalAlignment,
        wrap: TextWrapParams,
        font_select: &mut FontSelect,
    ) -> (Vec<ShapedLine>, Rect2<IFixed26Dot6>) {
        // assert_eq!(wrap.mode, TextWrapMode::None);

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
        let mut last = 0;
        let mut post_wrap_skip = 0;
        while current_explicit_line <= self.explicit_line_bounaries.len() {
            let mut line_boundary = self
                .explicit_line_bounaries
                .get(current_explicit_line)
                .copied()
                .unwrap_or(self.text.len());
            let mut segments: Vec<ShapedLineSegment> = vec![];
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

            while self.segment_boundaries[current_segment].1 <= last {
                current_segment += 1;
            }

            let mut did_wrap = false;

            'segment_shaper: loop {
                let (ref segment, font_boundary) = self.segment_boundaries[current_segment];

                let end = font_boundary.min(line_boundary);
                let segment_slice = last..end;

                match segment {
                    ShaperSegment::Text(font) => {
                        let mut segment_fonts = Vec::new();
                        let glyphs = {
                            let mut buffer = text::ShapingBuffer::new();
                            let direction = buffer.guess_properties();
                            if !direction.is_horizontal() {
                                buffer.set_direction(direction.to_horizontal());
                            }
                            buffer.add(&self.text[segment_slice]);
                            buffer.shape(font, &mut segment_fonts, font_select)
                        };

                        let (extents, (trailing_x_advance, _)) =
                            text::compute_extents_ex(true, &segment_fonts, &glyphs);

                        // println!("{:?} {:?}", line_extents, extents);
                        if !did_wrap
                            && wrap.mode == TextWrapMode::Normal
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

                        let mut local_max_ascender = IFixed26Dot6::MIN;
                        let mut local_min_descender = IFixed26Dot6::MAX;

                        for font in segment_fonts.iter() {
                            let ascender = IFixed26Dot6::from_ft(font.metrics().ascender);
                            let descender = IFixed26Dot6::from_ft(font.metrics().descender);

                            local_max_ascender = local_max_ascender.max(ascender);
                            local_min_descender = local_min_descender.min(descender);
                        }

                        if local_max_ascender > line_max_ascender {
                            line_max_ascender = local_max_ascender;
                        }

                        if local_min_descender < line_min_descender {
                            line_min_descender = local_min_descender;
                        }

                        let logical_height = local_max_ascender - local_min_descender;

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            segments.push(ShapedLineSegment {
                                glyphs_and_fonts: Some((rc_glyphs, segment_fonts)),
                                baseline_offset: Point2::new(
                                    line_extents.paint_width,
                                    total_extents.paint_height,
                                ),
                                logical_rect: Rect2::new(
                                    Point2::ZERO,
                                    Point2::new(
                                        extents.paint_width + trailing_x_advance,
                                        logical_height,
                                    ),
                                ),
                                max_ascender: local_max_ascender,
                                corresponding_font_boundary: current_segment,
                                corresponding_input_segment: current_segment + current_intra_split,
                            });
                        } else {
                            let mut last_glyph_idx = 0;
                            let mut x = I32Fixed::ZERO;

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

                                segments.push(ShapedLineSegment {
                                    glyphs_and_fonts: Some((glyph_slice, segment_fonts.clone())),
                                    baseline_offset: Point2::new(x, total_extents.paint_height),
                                    logical_rect: Rect2::new(
                                        Point2::ZERO,
                                        Point2::new(
                                            extents.paint_width + x_advance,
                                            logical_height,
                                        ),
                                    ),
                                    max_ascender: local_max_ascender,
                                    corresponding_font_boundary: current_segment,
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
                        segments.push(ShapedLineSegment {
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
                            corresponding_font_boundary: current_segment + current_intra_split,
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
                    segment.baseline_offset.y - line_max_ascender,
                ));
            }

            if !segments.is_empty() {
                for segment in &segments {
                    total_rect.expand_to_rect(segment.logical_rect);
                }
            }

            if line_boundary == self.text.len() {
                total_extents.paint_height += line_max_ascender - line_min_descender;
            } else {
                total_extents.paint_height += segments
                    .iter()
                    .map(
                        |x| match &self.segment_boundaries[x.corresponding_font_boundary].0 {
                            ShaperSegment::Text(f) => I32Fixed::from_ft(f.metrics().height),
                            ShaperSegment::Shape(_) => x.logical_rect.height(),
                        },
                    )
                    .max()
                    .unwrap_or(I32Fixed::ZERO);
            }

            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine { segments });
        }

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}
