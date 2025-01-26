use std::rc::Rc;

use crate::{math::I32Fixed, text, util::RcArray, HorizontalAlignment, PixelRect, TextWrapMode};

use super::{FontSelect, TextExtents};

const MULTILINE_SHAPER_DEBUG_PRINT: bool = false;

enum ShaperSegment {
    Text(text::Font),
    Shape(PixelRect),
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
    pub baseline_offset: (I32Fixed<6>, I32Fixed<6>),
    pub paint_rect: PixelRect,
    pub corresponding_input_segment: usize,
    // Implementation details
    max_bearing_y: I32Fixed<6>,
    corresponding_font_boundary: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Size2 {
    w: u32,
    h: u32,
}

#[derive(Debug)]
pub struct ShapedLine {
    pub segments: Vec<ShapedLineSegment>,
    pub paint_size: Size2,
}

#[derive(Debug, Clone)]
pub struct TextWrapParams {
    pub mode: TextWrapMode,
    pub max_width: f32,
    // will be used later for vertical text I guess
    pub max_height: f32,
}

// TODO: Notes on text layout for ASS when time comes
//       Max ascender + max descender = line height for ASS
//       libass/ass_render.c@measure_text

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

    pub fn add_shape(&mut self, dim: PixelRect) {
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
        _wrap: TextWrapParams,
        font_select: &mut FontSelect,
    ) -> (Vec<ShapedLine>, PixelRect) {
        // assert_eq!(wrap.mode, TextWrapMode::None);

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 TEXT {:?}", self.text);
            println!(
                "SHAPING V2 LINE BOUNDARIES {:?}",
                self.explicit_line_bounaries
            );
        }

        let mut lines: Vec<ShapedLine> = vec![];
        let mut total_extents = TextExtents {
            paint_width: I32Fixed::ZERO,
            paint_height: I32Fixed::ZERO,
        };
        let mut total_rect = PixelRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };

        let mut current_segment = 0;
        let mut current_intra_split = 0;
        let mut last = 0;
        for line_boundary in self
            .explicit_line_bounaries
            .iter()
            .copied()
            .chain(std::iter::once(self.text.len()))
        {
            // TODO: Where to get spacing?
            // let spacing = font.horizontal_extents().line_gap as i32 + 10;
            // if let Some(last) = lines.last() {
            //     let x = last
            //         .segments
            //         .iter()
            //         .map(|x| &self.font_boundaries[x.corresponding_font_boundary].0)
            //         .map(|f| f.metrics().height)
            //         .collect::<Vec<_>>();
            //     println!("spacing {x:?}");
            // }
            // if !lines.is_empty() {
            //     total_extents.paint_height += 10 * 64;
            // }

            let mut segments: Vec<ShapedLineSegment> = vec![];
            let mut line_extents = TextExtents {
                paint_height: I32Fixed::ZERO,
                paint_width: I32Fixed::ZERO,
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

            let mut max_bearing_y = I32Fixed::ZERO;

            while self.segment_boundaries[current_segment].1 <= last {
                current_segment += 1;
            }

            loop {
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
                        let segment_fonts = Rc::new(segment_fonts);

                        let (extents, (trailing_x_advance, _)) =
                            text::compute_extents_ex(true, &segment_fonts, &glyphs);

                        let segment_max_bearing_y = I32Fixed::from_ft(font.metrics().ascender);
                        max_bearing_y = std::cmp::max(max_bearing_y, segment_max_bearing_y);

                        let rc_glyphs = RcArray::from_boxed(glyphs.into_boxed_slice());

                        if MULTILINE_SHAPER_DEBUG_PRINT {
                            println!(
                                "last: {last}, end: {end}, intra font splits: {:?}, current intra split: {}",
                                self.intra_font_segment_splits, current_intra_split
                            );
                        }

                        if self
                            .intra_font_segment_splits
                            .get(current_intra_split)
                            .is_none_or(|split| *split >= end)
                        {
                            segments.push(ShapedLineSegment {
                                glyphs_and_fonts: Some((rc_glyphs, segment_fonts)),
                                baseline_offset: (
                                    line_extents.paint_width,
                                    total_extents.paint_height,
                                ),
                                paint_rect: PixelRect {
                                    x: line_extents.paint_width.trunc_to_inner(),
                                    y: total_extents.paint_height.trunc_to_inner(),
                                    w: extents.paint_width.trunc_to_inner() as u32,
                                    h: extents.paint_height.trunc_to_inner() as u32,
                                },
                                max_bearing_y: segment_max_bearing_y,
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
                                    baseline_offset: (x, total_extents.paint_height),
                                    paint_rect: PixelRect {
                                        x: x.trunc_to_inner(),
                                        y: total_extents.paint_height.trunc_to_inner(),
                                        w: extents.paint_width.trunc_to_inner() as u32,
                                        h: extents.paint_height.trunc_to_inner() as u32,
                                    },
                                    max_bearing_y: segment_max_bearing_y,
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
                    }
                    // TODO: Figure out exactly how libass lays out shapes
                    ShaperSegment::Shape(dim) => {
                        let logical_w = dim.w - (-dim.x).min(0) as u32;
                        let logical_h = dim.h - (-dim.y).min(0) as u32;
                        let segment_max_bearing_y = I32Fixed::new(logical_h as i32);
                        segments.push(ShapedLineSegment {
                            glyphs_and_fonts: None,
                            baseline_offset: (line_extents.paint_width, total_extents.paint_height),
                            paint_rect: PixelRect {
                                x: line_extents.paint_width.trunc_to_inner(),
                                y: total_extents.paint_height.trunc_to_inner(),
                                w: logical_w,
                                h: logical_h,
                            },
                            corresponding_input_segment: current_segment + current_intra_split,
                            corresponding_font_boundary: current_segment + current_intra_split,
                            max_bearing_y: segment_max_bearing_y,
                        });
                        line_extents.paint_width += (logical_w * 64) as i32;
                        max_bearing_y = max_bearing_y.max(segment_max_bearing_y);
                        line_extents.paint_height = line_extents
                            .paint_height
                            .max(I32Fixed::new(logical_h as i32));
                    }
                }

                last = end;

                if end == line_boundary {
                    break;
                } else {
                    current_segment += 1;
                }
            }

            debug_assert_eq!(last, line_boundary);

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => 0,
                HorizontalAlignment::Center => -line_extents.paint_width.trunc_to_inner() / 2,
                HorizontalAlignment::Right => -line_extents.paint_width.trunc_to_inner(),
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.0 += aligning_x_offset;
                segment.paint_rect.x += aligning_x_offset;
                if segment.glyphs_and_fonts.is_none() {
                    segment.baseline_offset.1 += max_bearing_y - segment.max_bearing_y;
                } else {
                    segment.baseline_offset.1 += max_bearing_y;
                }
                segment.paint_rect.y += (max_bearing_y - segment.max_bearing_y).trunc_to_inner();
            }

            if !segments.is_empty() {
                total_rect.x = total_rect
                    .x
                    .min(segments.first().map(|x| x.paint_rect.x).unwrap());
                total_rect.w = total_rect.w.max(
                    (segments
                        .last()
                        .map(|x| (x.paint_rect.x + x.paint_rect.w as i32))
                        .unwrap()
                        - segments.first().map(|x| x.paint_rect.x).unwrap())
                        as u32,
                );
            }

            if line_boundary == self.text.len() {
                total_extents.paint_height += line_extents.paint_height;
            } else {
                total_extents.paint_height += segments
                    .iter()
                    .map(
                        |x| match &self.segment_boundaries[x.corresponding_font_boundary].0 {
                            ShaperSegment::Text(f) => I32Fixed::from_ft(f.metrics().height),
                            ShaperSegment::Shape(_) => I32Fixed::new(x.paint_rect.h as i32),
                        },
                    )
                    .max()
                    .unwrap_or(I32Fixed::ZERO)
            }

            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine {
                segments,
                paint_size: Size2 {
                    w: line_extents.paint_width.trunc_to_inner() as u32,
                    h: line_extents.paint_height.trunc_to_inner() as u32,
                },
            });
        }

        total_rect.h = total_extents.paint_height.trunc_to_inner() as u32;

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}
