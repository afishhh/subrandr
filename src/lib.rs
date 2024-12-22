// The library is still under active development
#![allow(dead_code)]
// #![cfg_attr(test, feature(test))]
#![warn(clippy::nursery)]
#![allow(clippy::too_many_arguments)]
// FIXME: enable this when suboptimal_flops stops recommending you mul_add
#![allow(clippy::suboptimal_flops)]
#![allow(clippy::missing_transmute_annotations)]
// .as_ptr() is not expensive.
#![allow(clippy::or_fun_call)]

use std::rc::Rc;

use color::BGRA8;
use math::Point2;
use outline::{CurveDegree, OutlineBuilder};
use rasterize::NonZeroPolygonRasterizer;
use text::{FontManager, TextExtents};

pub mod ass;
mod capi;
mod color;
mod math;
mod outline;
mod painter;
mod polyline;
mod rasterize;
pub mod srv3;
mod text;
mod util;

pub use painter::*;
use util::{ref_to_slice, RcArray};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Alignment {
    pub const fn into_parts(self) -> (HorizontalAlignment, VerticalAlignment) {
        match self {
            Self::TopLeft => (HorizontalAlignment::Left, VerticalAlignment::Top),
            Self::Top => (HorizontalAlignment::Center, VerticalAlignment::Top),
            Self::TopRight => (HorizontalAlignment::Right, VerticalAlignment::Top),
            Self::Left => (
                HorizontalAlignment::Left,
                VerticalAlignment::BaselineCentered,
            ),
            Self::Center => (
                HorizontalAlignment::Center,
                VerticalAlignment::BaselineCentered,
            ),
            Self::Right => (
                HorizontalAlignment::Right,
                VerticalAlignment::BaselineCentered,
            ),
            Self::BottomLeft => (HorizontalAlignment::Left, VerticalAlignment::Bottom),
            Self::Bottom => (HorizontalAlignment::Center, VerticalAlignment::Bottom),
            Self::BottomRight => (HorizontalAlignment::Right, VerticalAlignment::Bottom),
        }
    }
}

enum VerticalAlignment {
    Top,
    BaselineCentered,
    Bottom,
}

enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextWrappingMode {
    None,
}

#[derive(Debug, Clone)]
struct Event {
    start: u32,
    end: u32,
    x: f32,
    y: f32,
    alignment: Alignment,
    text_wrap: TextWrappingMode,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
enum Segment {
    Text(TextSegment),
    Shape(ShapeSegment),
}

#[derive(Debug, Clone)]
struct TextSegment {
    font: String,
    font_size: f32,
    font_weight: u32,
    italic: bool,
    underline: bool,
    strike_out: bool,
    color: BGRA8,
    text: String,
}

// Shape segment behaviour:
// Treated as constant-sized block during text layout
// Size does not take into account negative coordinates
#[derive(Debug, Clone)]
pub struct ShapeSegment {
    outline: outline::Outline,
    bounding_box: math::Rect2,
    stroke_width: f32,
    stroke_color: BGRA8,
    fill_color: BGRA8,
}

impl ShapeSegment {
    pub fn new(
        outline: outline::Outline,
        stroke_width: f32,
        stroke_color: BGRA8,
        fill_color: BGRA8,
    ) -> Self {
        Self {
            bounding_box: { outline.bounding_box().clamp_to_positive() },
            outline,
            stroke_width,
            stroke_color,
            fill_color,
        }
    }
}

#[derive(Debug)]
pub struct Subtitles {
    events: Vec<Event>,
}

impl Subtitles {
    pub const fn empty() -> Self {
        Self { events: vec![] }
    }

    #[doc(hidden)]
    pub fn test_new() -> Self {
        Self {
            events: vec![
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.2,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "this ".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0x0000FFFF),
                            text: "is\n".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "mu".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 48.0,
                            font_weight: 700,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "ltil".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "Arial".to_string(),
                            font_size: 80.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "iね❌".to_string(),
                        }),
                        Segment::Shape(ShapeSegment::new(
                            {
                                let mut b = OutlineBuilder::new();
                                b.add_point(Point2::new(0.0, 0.0));
                                b.add_point(Point2::new(30.0, 120.));
                                b.add_point(Point2::new(120.0, 120.));
                                b.add_segment(CurveDegree::Linear);
                                b.add_segment(CurveDegree::Linear);
                                b.add_segment(CurveDegree::Linear);
                                b.close_contour();
                                b.build()
                            },
                            5.0,
                            BGRA8::from_rgba32(0x00FF00FF),
                            BGRA8::from_rgba32(0x00FFFFFF),
                        )),
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.1,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: BGRA8::from_rgba32(0x00FF00AA),
                        text: "this is for comparison".to_string(),
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.8,
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: BGRA8::from_rgba32(0xFFFFFFFF),
                        text: "this is bold..".to_string(),
                    })],
                },
                // FIXME: Doesn't work, scaling emoji font fails
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.6,
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: "emoji".to_string(),
                            font_size: 32.,
                            font_weight: 700,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            text: "😭".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "emoji".to_string(),
                            font_size: 64.,
                            font_weight: 700,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            text: "😭".to_string(),
                        }),
                    ],
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Copy)]
struct PixelRect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

const MULTILINE_SHAPER_DEBUG_PRINT: bool = false;

enum ShaperSegment {
    Text(text::Font),
    Shape(PixelRect),
}

struct MultilineTextShaper {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segment_boundaries: Vec<(ShaperSegment, /* end of segment i */ usize)>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug)]
struct ShapedLineSegment {
    glyphs_and_fonts: Option<(RcArray<text::Glyph>, Rc<Vec<text::Font>>)>,
    baseline_offset: (i32, i32),
    paint_rect: PixelRect,
    corresponding_input_segment: usize,
    // Implementation details
    max_bearing_y: i64,
    corresponding_font_boundary: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Size2 {
    w: u32,
    h: u32,
}

#[derive(Debug)]
struct ShapedLine {
    segments: Vec<ShapedLineSegment>,
    paint_size: Size2,
}

impl MultilineTextShaper {
    const fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            segment_boundaries: Vec::new(),
            intra_font_segment_splits: Vec::new(),
        }
    }

    fn add_text(&mut self, mut text: &str, font: &text::Font) {
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

    fn add_shape(&mut self, dim: PixelRect) {
        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 INPUT SHAPE: {dim:?}",);
        }

        self.text.push('\0');
        self.segment_boundaries
            .push((ShaperSegment::Shape(dim), self.text.len()))
    }

    fn shape(
        &self,
        line_alignment: HorizontalAlignment,
        wrapping: TextWrappingMode,
        font_manager: &mut FontManager,
    ) -> (Vec<ShapedLine>, PixelRect) {
        assert_eq!(wrapping, TextWrappingMode::None);

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 TEXT {:?}", self.text);
            println!(
                "SHAPING V2 LINE BOUNDARIES {:?}",
                self.explicit_line_bounaries
            );
        }

        let mut lines: Vec<ShapedLine> = vec![];
        let mut total_extents = TextExtents {
            paint_width: 0,
            paint_height: 0,
        };
        let mut total_rect = PixelRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        };

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
                paint_height: 0,
                paint_width: 0,
            };

            // TODO: These binary searches can be replaced by a pointer
            let starting_font_segment = match self
                .segment_boundaries
                .binary_search_by_key(&last, |(_, b)| *b)
            {
                Ok(idx) => idx + 1,
                Err(idx) => idx,
            };

            if MULTILINE_SHAPER_DEBUG_PRINT {
                println!(
                    "last: {last}, font boundaries: {:?}, binary search result: {}",
                    self.segment_boundaries
                        .iter()
                        .map(|(_, s)| s)
                        .collect::<Vec<_>>(),
                    starting_font_segment
                );
            }

            let mut max_bearing_y = 0;

            for current_segment in starting_font_segment..self.segment_boundaries.len() {
                let (segment, font_boundary) = &self.segment_boundaries[current_segment];

                let end = (*font_boundary).min(line_boundary);
                let segment_slice = last..end;

                let first_internal_split_idx =
                    match self.intra_font_segment_splits.binary_search(&last) {
                        Ok(idx) => idx + 1,
                        Err(idx) => idx,
                    };

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
                            buffer.shape(font, &mut segment_fonts, font_manager)
                        };
                        let segment_fonts = Rc::new(segment_fonts);

                        let (extents, (trailing_x_advance, _)) =
                            text::compute_extents_ex(true, &segment_fonts, &glyphs);

                        let segment_max_bearing_y = glyphs
                            .iter()
                            .map(|x| {
                                segment_fonts[x.font_index]
                                    .glyph_extents(x.index)
                                    .horiBearingY
                            })
                            .max()
                            .unwrap_or(0);

                        max_bearing_y = std::cmp::max(max_bearing_y, segment_max_bearing_y);

                        let rc_glyphs = RcArray::from_boxed(glyphs.into_boxed_slice());

                        if MULTILINE_SHAPER_DEBUG_PRINT {
                            println!(
                            "last: {last}, end: {end}, intra font splits: {:?}, binary search result: {}",
                            self.intra_font_segment_splits, first_internal_split_idx
                        );
                        }

                        if self
                            .intra_font_segment_splits
                            .get(first_internal_split_idx)
                            .is_none_or(|split| *split >= end)
                        {
                            segments.push(ShapedLineSegment {
                                glyphs_and_fonts: Some((rc_glyphs, segment_fonts)),
                                baseline_offset: (
                                    line_extents.paint_width / 64,
                                    total_extents.paint_height / 64,
                                ),
                                paint_rect: PixelRect {
                                    x: line_extents.paint_width / 64,
                                    y: total_extents.paint_height / 64,
                                    w: extents.paint_width as u32 / 64,
                                    h: extents.paint_height as u32 / 64,
                                },
                                max_bearing_y: segment_max_bearing_y,
                                corresponding_font_boundary: current_segment,
                                corresponding_input_segment: current_segment
                                    + first_internal_split_idx,
                            });
                        } else {
                            let mut last_glyph_idx = 0;
                            let mut x = 0;
                            for (i, split_end) in self.intra_font_segment_splits
                                [first_internal_split_idx..]
                                .iter()
                                .copied()
                                .take_while(|idx| *idx < end)
                                .chain(std::iter::once(end))
                                .enumerate()
                            {
                                let end_glyph_idx = split_end - last;
                                let glyph_range = last_glyph_idx..end_glyph_idx;
                                let glyph_slice = RcArray::slice(rc_glyphs.clone(), glyph_range);
                                let (extents, (x_advance, _)) =
                                    text::compute_extents_ex(true, &segment_fonts, &glyph_slice);
                                segments.push(ShapedLineSegment {
                                    glyphs_and_fonts: Some((glyph_slice, segment_fonts.clone())),
                                    baseline_offset: (x / 64, total_extents.paint_height / 64),
                                    paint_rect: PixelRect {
                                        x: x / 64,
                                        y: total_extents.paint_height / 64,
                                        w: extents.paint_width as u32 / 64,
                                        h: extents.paint_height as u32 / 64,
                                    },
                                    max_bearing_y: segment_max_bearing_y,
                                    corresponding_font_boundary: current_segment,
                                    corresponding_input_segment: current_segment
                                        + first_internal_split_idx
                                        + i,
                                });
                                last_glyph_idx = end_glyph_idx;
                                x += extents.paint_width + x_advance;
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
                        let segment_max_bearing_y = (logical_h * 64) as i64;
                        let y = dim.y.max(0);
                        // let x = dim.x.max(0);
                        segments.push(ShapedLineSegment {
                            glyphs_and_fonts: None,
                            baseline_offset: (
                                line_extents.paint_width / 64,
                                total_extents.paint_height / 64 - y,
                            ),
                            paint_rect: PixelRect {
                                x: line_extents.paint_width / 64,
                                y: total_extents.paint_height / 64 - y,
                                w: logical_w,
                                h: logical_h,
                            },
                            corresponding_input_segment: current_segment + first_internal_split_idx,
                            corresponding_font_boundary: current_segment + first_internal_split_idx,
                            max_bearing_y: segment_max_bearing_y,
                        });
                        line_extents.paint_width += (logical_w * 64) as i32;
                        max_bearing_y = max_bearing_y.max(segment_max_bearing_y);
                        line_extents.paint_height =
                            line_extents.paint_height.max((logical_h * 64) as i32);
                    }
                }

                last = end;

                if end == line_boundary {
                    break;
                }
            }

            debug_assert_eq!(last, line_boundary);

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => 0,
                HorizontalAlignment::Center => -line_extents.paint_width / 128,
                HorizontalAlignment::Right => -line_extents.paint_width / 64,
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.0 += aligning_x_offset;
                segment.paint_rect.x += aligning_x_offset;
                if segment.glyphs_and_fonts.is_none() {
                    segment.baseline_offset.1 +=
                        ((max_bearing_y - segment.max_bearing_y) / 64) as i32;
                } else {
                    segment.baseline_offset.1 += (max_bearing_y / 64) as i32;
                }
                segment.paint_rect.y += ((max_bearing_y - segment.max_bearing_y) / 64) as i32;
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
                            ShaperSegment::Text(f) => {
                                x.paint_rect.y as i64 * 64 + f.metrics().height
                            }
                            ShaperSegment::Shape(_) => (x.paint_rect.h * 64) as i64,
                        },
                    )
                    .max()
                    .unwrap_or(0) as i32;
            }

            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine {
                segments,
                paint_size: Size2 {
                    w: (line_extents.paint_width / 64) as u32,
                    h: (line_extents.paint_height / 64) as u32,
                },
            });
        }

        total_rect.h = (total_extents.paint_height / 64) as u32;

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}

pub struct Renderer<'a> {
    fonts: text::FontManager,
    // always should be 72 for ass?
    dpi: u32,
    subs: &'a Subtitles,
}

impl<'a> Renderer<'a> {
    pub fn new(subs: &'a Subtitles, dpi: u32) -> Self {
        Self {
            fonts: text::FontManager::new(text::font_backend::platform_default().unwrap()),
            dpi,
            subs,
        }
    }

    pub fn resize(&mut self, _width: u32, _height: u32) {
    }

    fn debug_text(
        &mut self,
        x: i32,
        y: i32,
        text: &str,
        alignment: Alignment,
        size: f32,
        color: BGRA8,
        painter: &mut Painter,
    ) {
        let font = self
            .fonts
            .get_or_load("monospace", 400., false)
            .unwrap()
            .with_size(size, self.dpi);
        let shaped = text::shape_text(&font, text);
        let (ox, oy) = Self::translate_for_aligned_text(
            &font,
            true,
            &text::compute_extents(true, ref_to_slice(&font), &shaped.glyphs),
            alignment,
        );
        painter.text(x + ox, y + oy, ref_to_slice(&font), &shaped.glyphs, color);
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextExtents,
        alignment: Alignment,
    ) -> (i32, i32) {
        assert!(horizontal);

        let (horizontal, vertical) = alignment.into_parts();

        // TODO: Numbers chosen arbitrarily
        let ox = match horizontal {
            HorizontalAlignment::Left => -font.horizontal_extents().descender / 64 / 2,
            HorizontalAlignment::Center => -extents.paint_width / 128,
            HorizontalAlignment::Right => {
                (-extents.paint_width + font.horizontal_extents().descender) / 64
            }
        };

        let oy = match vertical {
            VerticalAlignment::Top => font.horizontal_extents().ascender / 64,
            VerticalAlignment::BaselineCentered => 0,
            VerticalAlignment::Bottom => font.horizontal_extents().descender / 64,
        };

        (ox, oy)
    }

    pub fn render(&mut self, painter: &mut Painter, t: u32) {
        if painter.height() == 0 || painter.height() == 0 {
            return;
        }

        painter.clear(BGRA8::ZERO);

        self.debug_text(
            painter.width() as i32,
            0,
            &format!("{}x{} dpi:{}", painter.width(), painter.height(), self.dpi),
            Alignment::TopRight,
            16.0,
            BGRA8::from_rgba32(0xFFFFFFFF),
            painter,
        );

        let shape_scale = self.dpi as f32 / 72.0;

        {
            for event in self
                .subs
                .events
                .iter()
                .filter(|ev| ev.start <= t && ev.end > t)
            {
                let x = (painter.width() as f32 * event.x) as u32;
                let y = (painter.height() as f32 * event.y) as u32;

                let mut shaper = MultilineTextShaper::new();
                for segment in event.segments.iter() {
                    match segment {
                        Segment::Text(segment) => {
                            let font = self
                                .fonts
                                .get_or_load(
                                    &segment.font,
                                    segment.font_weight as f32,
                                    segment.italic,
                                )
                                .unwrap()
                                .with_size(segment.font_size, self.dpi);

                            shaper.add_text(&segment.text, &font);
                        }
                        Segment::Shape(shape) => {
                            shaper.add_shape(PixelRect {
                                x: (shape.bounding_box.min.x * shape_scale).floor() as i32,
                                y: (shape.bounding_box.min.y * shape_scale).floor() as i32,
                                w: ((shape.bounding_box.size().x + shape.stroke_width / 2.0)
                                    * shape_scale)
                                    .ceil() as u32,
                                h: ((shape.bounding_box.size().y + shape.stroke_width / 2.0)
                                    * shape_scale)
                                    .ceil() as u32,
                            });
                        }
                    }
                }

                let (horizontal_alignment, vertical_alignment) = event.alignment.into_parts();
                let (lines, total_rect) = shaper.shape(
                    horizontal_alignment,
                    TextWrappingMode::None,
                    &mut self.fonts,
                );

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => -(total_rect.h as i32) / 2,
                        VerticalAlignment::Bottom => -(total_rect.h as i32),
                    };

                painter.stroke_whrect(
                    x + total_rect.x - 1,
                    y + total_rect.y - 1,
                    total_rect.w + 2,
                    total_rect.h + 2,
                    BGRA8::from_rgba32(0xFF00FFFF),
                );

                let total_position_debug_pos = match vertical_alignment {
                    VerticalAlignment::Top => (total_rect.h as i32 + 20, Alignment::Top),
                    VerticalAlignment::BaselineCentered => {
                        (total_rect.h as i32 + 20, Alignment::Top)
                    }
                    VerticalAlignment::Bottom => (-32, Alignment::Bottom),
                };

                self.debug_text(
                    x + total_rect.x + total_rect.w as i32 / 2,
                    y + total_rect.y + total_position_debug_pos.0,
                    &format!(
                        "x:{} y:{} w:{} h:{}",
                        x + total_rect.x,
                        y + total_rect.y,
                        total_rect.w,
                        total_rect.h
                    ),
                    total_position_debug_pos.1,
                    16.0,
                    BGRA8::from_rgba32(0xFF00FFFF),
                    painter,
                );

                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];

                    let paint_box = (
                        x + shaped_segment.paint_rect.x,
                        y + shaped_segment.paint_rect.y,
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1,
                        &format!(
                            "{},{}",
                            x + shaped_segment.paint_rect.x,
                            y + shaped_segment.paint_rect.y
                        ),
                        Alignment::BottomLeft,
                        16.0,
                        BGRA8::from_rgba32(0xFF0000FF),
                        painter,
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1 + shaped_segment.paint_rect.h as i32,
                        &format!(
                            "{},{}",
                            x + shaped_segment.baseline_offset.0,
                            y + shaped_segment.baseline_offset.1
                        ),
                        Alignment::TopLeft,
                        16.0,
                        BGRA8::from_rgba32(0xFF0000FF),
                        painter,
                    );

                    self.debug_text(
                        paint_box.0 + shaped_segment.paint_rect.w as i32,
                        paint_box.1,
                        &if let Segment::Text(segment) = segment {
                            format!("{:.0}pt", segment.font_size)
                        } else {
                            "shape".to_owned()
                        },
                        Alignment::BottomRight,
                        16.0,
                        BGRA8::from_rgba32(0xFFFFFFFF),
                        painter,
                    );

                    painter.stroke_whrect(
                        paint_box.0,
                        paint_box.1,
                        shaped_segment.paint_rect.w,
                        shaped_segment.paint_rect.h,
                        BGRA8::from_rgba32(0x0000FFFF),
                    );

                    painter.horizontal_line(
                        y + shaped_segment.baseline_offset.1,
                        paint_box.0,
                        paint_box.0 + shaped_segment.paint_rect.w as i32,
                        BGRA8::from_rgba32(0x00FF00FF),
                    );

                    let x = x + shaped_segment.baseline_offset.0;
                    let y = y + shaped_segment.baseline_offset.1;

                    match segment {
                        Segment::Text(t) => {
                            let (glyphs, fonts) = shaped_segment.glyphs_and_fonts.as_ref().unwrap();
                            painter.text(x, y, fonts, glyphs, t.color);
                        }
                        Segment::Shape(s) => {
                            let mut outline = s.outline.clone();
                            outline.scale(shape_scale);

                            let (x, y) = ((x as f32) as i32, (y as f32) as i32);

                            let stroked = outline::stroke(
                                &outline,
                                s.stroke_width * shape_scale / 2.0,
                                s.stroke_width * shape_scale / 2.0,
                                0.01,
                            );

                            let mut rasterizer = NonZeroPolygonRasterizer::new();
                            for c in outline.iter_contours() {
                                rasterizer.append_polyline(
                                    (x, y),
                                    &outline.flatten_contour(c),
                                    false,
                                );
                                rasterizer.render_fill(painter, s.fill_color);
                            }

                            for (a, b) in stroked.0.iter_contours().zip(stroked.1.iter_contours()) {
                                rasterizer.reset();
                                rasterizer.append_polyline(
                                    (x, y),
                                    &stroked.0.flatten_contour(a),
                                    false,
                                );
                                rasterizer.append_polyline(
                                    (x, y),
                                    &stroked.1.flatten_contour(b),
                                    true,
                                );
                                rasterizer.render_fill(painter, s.stroke_color);
                            }

                            painter.debug_stroke_outline(
                                x,
                                y,
                                &stroked.0,
                                BGRA8::from_rgba32(0xFF0000FF),
                                false,
                            );
                            painter.debug_stroke_outline(
                                x,
                                y,
                                &stroked.1,
                                BGRA8::from_rgba32(0x0000FFFF),
                                true,
                            );
                        }
                    }
                }
            }
        }
    }
}
