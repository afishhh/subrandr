// The library is still under active development
#![allow(dead_code)]
// #![cfg_attr(test, feature(test))]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::missing_transmute_annotations)]

use std::{cell::Cell, collections::VecDeque, fmt::Debug, rc::Rc};

use color::{BlendMode, BGRA8};
use log::{info, trace, Logger};
use math::{Fixed, Point2, Vec2};
use outline::{OutlineBuilder, SegmentDegree};
use rasterize::NonZeroPolygonRasterizer;
use srv3::{Srv3Event, Srv3TextShadow};
use text::{FontRequest, FontSelect, TextExtents};

pub mod ass;
mod capi;
mod color;
mod log;
mod math;
mod outline;
mod painter;
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
enum TextWrapMode {
    Normal, // the css one? greedy I think
    None,
}

#[derive(Debug, Clone)]
enum EventExtra {
    Srv3(Srv3Event),
    Test { x: f32, y: f32 },
}

impl EventExtra {
    fn compute_layout(&self, ctx: &SubtitleContext, event: &Event) -> EventLayout {
        match self {
            EventExtra::Srv3(srv3) => srv3.compute_layout(ctx, event),
            &Self::Test { x, y } => EventLayout {
                x: ctx.padding_left + (x * ctx.video_width),
                y: ctx.padding_top + (y * ctx.video_height),
                max_width: f32::INFINITY,
                max_height: f32::INFINITY,
            },
        }
    }
}

struct EventLayout {
    x: f32,
    y: f32,
    max_width: f32,
    max_height: f32,
}

#[derive(Debug, Clone)]
struct Event {
    start: u32,
    end: u32,
    alignment: Alignment,
    text_wrap: TextWrapMode,
    segments: Vec<Segment>,
    extra: EventExtra,
}

#[derive(Debug, Clone)]
enum Segment {
    Text(TextSegment),
    Shape(ShapeSegment),
}

#[derive(Debug, Clone)]
struct TextSegment {
    font: Vec<String>,
    font_size: f32,
    font_weight: u32,
    italic: bool,
    decorations: TextDecorations,
    color: BGRA8,
    text: String,
    shadows: Vec<TextShadow>,
}

#[derive(Debug, Clone)]
struct TextDecorations {
    border: Vec2,
    border_color: BGRA8,
    // TODO: f32 for size
    underline: bool,
    underline_color: BGRA8,
    strike_out: bool,
    strike_out_color: BGRA8,
}

#[derive(Debug, Clone)]
enum TextShadow {
    Css(CssTextShadow),
    Srv3(Srv3TextShadow),
}

#[derive(Debug, Clone)]
struct CssTextShadow {
    offset: Vec2,
    blur_radius: f32,
    color: BGRA8,
}

impl TextDecorations {
    pub const fn none() -> Self {
        Self {
            border: Vec2::ZERO,
            border_color: BGRA8::ZERO,
            underline: false,
            underline_color: BGRA8::ZERO,
            strike_out: false,
            strike_out_color: BGRA8::ZERO,
        }
    }
}

impl Default for TextDecorations {
    fn default() -> Self {
        Self::none()
    }
}

// Shape segment behaviour:
// Treated as constant-sized block during text layout
// Size does not take into account negative coordinates
#[derive(Debug, Clone)]
pub struct ShapeSegment {
    outline: outline::Outline,
    bounding_box: math::Rect2,
    stroke_x: f32,
    stroke_y: f32,
    stroke_color: BGRA8,
    fill_color: BGRA8,
}

impl ShapeSegment {
    pub fn new(
        outline: outline::Outline,
        stroke_x: f32,
        stroke_y: f32,
        stroke_color: BGRA8,
        fill_color: BGRA8,
    ) -> Self {
        Self {
            bounding_box: { outline.control_box().clamp_to_positive() },
            outline,
            stroke_x,
            stroke_y,
            stroke_color,
            fill_color,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
// TODO: Maybe call this a viewport or have a field called "viewport"
pub struct SubtitleContext {
    pub dpi: u32,
    pub video_width: f32,
    pub video_height: f32,
    pub padding_left: f32,
    pub padding_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,
}

impl SubtitleContext {
    pub fn ppi(&self) -> u32 {
        self.dpi * 96 / 72
    }

    pub fn pixel_scale(&self) -> f32 {
        self.dpi as f32 / 72.0
    }

    pub fn padding_width(&self) -> f32 {
        self.padding_left + self.padding_right
    }

    pub fn padding_height(&self) -> f32 {
        self.padding_top + self.padding_bottom
    }

    pub fn player_width(&self) -> f32 {
        self.video_width + self.padding_width()
    }

    pub fn player_height(&self) -> f32 {
        self.video_height + self.padding_height()
    }
}

trait SubtitleClass: Debug {
    fn get_name(&self) -> &'static str;
    fn get_font_size(&self, ctx: &SubtitleContext, event: &Event, segment: &TextSegment) -> f32;
}

// Font size passed through directly.
// Coordinate system 0.0-1.0 percentages
#[derive(Debug)]
struct TestSubtitleClass;
impl SubtitleClass for TestSubtitleClass {
    fn get_name(&self) -> &'static str {
        "<test>"
    }

    fn get_font_size(&self, _ctx: &SubtitleContext, _event: &Event, segment: &TextSegment) -> f32 {
        segment.font_size
    }
}

#[derive(Debug)]
pub struct Subtitles {
    class: &'static dyn SubtitleClass,
    events: Vec<Event>,
}

impl Subtitles {
    pub const fn empty() -> Self {
        Self {
            class: &TestSubtitleClass,
            events: vec![],
        }
    }

    #[doc(hidden)]
    pub fn test_new() -> Self {
        Self {
            class: &TestSubtitleClass,
            events: vec![
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.5, y: 0.2 },
                    alignment: Alignment::Top,
                    text_wrap: TextWrapMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "this ".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0x0000FFFF),
                            text: "is\n".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "mu".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: 48.0,
                            font_weight: 700,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "ltil".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["Arial".to_string()],
                            font_size: 80.0,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            text: "iね❌".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Shape(ShapeSegment::new(
                            {
                                let mut b = OutlineBuilder::new();
                                b.add_point(Point2::new(0.0, 0.0));
                                b.add_point(Point2::new(30.0, 120.));
                                b.add_point(Point2::new(120.0, 120.));
                                b.add_segment(SegmentDegree::Linear);
                                b.add_segment(SegmentDegree::Linear);
                                b.add_segment(SegmentDegree::Linear);
                                b.close_contour();
                                b.build()
                            },
                            2.0,
                            5.0,
                            BGRA8::from_rgba32(0x00FF00FF),
                            BGRA8::from_rgba32(0x00FFFFFF),
                        )),
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.5, y: 0.1 },
                    alignment: Alignment::Top,
                    text_wrap: TextWrapMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: vec!["monospace".to_string()],
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        decorations: TextDecorations {
                            border: Vec2::new(2.0, 2.0),
                            border_color: BGRA8::new(255, 0, 0, 255),
                            underline: true,
                            underline_color: BGRA8::new(255, 255, 255, 255),
                            strike_out: true,
                            strike_out_color: BGRA8::new(255, 255, 255, 255),
                        },
                        color: BGRA8::from_rgba32(0x00FF00AA),
                        text: "this is for comparison".to_string(),
                        shadows: Vec::new(),
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.8, y: 0.9 },
                    alignment: Alignment::Top,
                    text_wrap: TextWrapMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: vec!["sans-serif".to_string()],
                            font_size: 26.6,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0x00FF00AA),
                            text: "mo".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["sans-serif".to_string()],
                            font_size: 26.6,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFF00AA),
                            text: "ment".to_string(),
                            shadows: Vec::new(),
                        }),
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.8, y: 0.8 },
                    alignment: Alignment::Top,
                    text_wrap: TextWrapMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: vec!["sans-serif".to_string()],
                        font_size: 26.6,
                        font_weight: 400,
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0x0000FFAA),
                        text: "moment".to_string(),
                        shadows: Vec::new(),
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.2, y: 0.9 },
                    alignment: Alignment::BottomLeft,
                    text_wrap: TextWrapMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: vec!["sans-serif".to_string()],
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0x00FF0099),
                        text: "with shadows".to_string(),
                        shadows: vec![
                            TextShadow::Css(CssTextShadow {
                                offset: Vec2::new(80.0, 80.0),
                                blur_radius: 0.0,
                                color: BGRA8::new(0, 0, 255, 40),
                            }),
                            TextShadow::Css(CssTextShadow {
                                offset: Vec2::new(48.0, 48.0),
                                blur_radius: 20.0,
                                color: BGRA8::new(255, 0, 0, 255),
                            }),
                        ],
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.5, y: 0.8 },
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrapMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: vec!["monospace".to_string()],
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0xFFFFFFFF),
                        text: "this is bold..".to_string(),
                        shadows: Vec::new(),
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.5, y: 0.6 },
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrapMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: vec!["emoji".to_string()],
                            font_size: 32.,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            text: "😭".to_string(),
                            shadows: Vec::new(),
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["emoji".to_string()],
                            font_size: 64.,
                            font_weight: 400,
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            text: "😭".to_string(),
                            shadows: Vec::new(),
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
    baseline_offset: (Fixed<6>, Fixed<6>),
    paint_rect: PixelRect,
    corresponding_input_segment: usize,
    // Implementation details
    max_bearing_y: Fixed<6>,
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

#[derive(Debug, Clone)]
struct TextWrapParams {
    mode: TextWrapMode,
    max_width: f32,
    // will be used later for vertical text I guess
    max_height: f32,
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
            paint_width: Fixed::ZERO,
            paint_height: Fixed::ZERO,
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
                paint_height: Fixed::ZERO,
                paint_width: Fixed::ZERO,
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

            let mut max_bearing_y = Fixed::ZERO;

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
                            buffer.shape(font, &mut segment_fonts, font_select)
                        };
                        let segment_fonts = Rc::new(segment_fonts);

                        let (extents, (trailing_x_advance, _)) =
                            text::compute_extents_ex(true, &segment_fonts, &glyphs);

                        let segment_max_bearing_y = glyphs
                            .iter()
                            .map(|x| {
                                segment_fonts[x.font_index]
                                    .glyph_extents(x.index)
                                    .hori_bearing_y
                            })
                            .max()
                            .unwrap_or(Fixed::ZERO);

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
                                    line_extents.paint_width,
                                    total_extents.paint_height,
                                ),
                                paint_rect: PixelRect {
                                    x: line_extents.paint_width.trunc_to_i32(),
                                    y: total_extents.paint_height.trunc_to_i32(),
                                    w: extents.paint_width.trunc_to_i32() as u32,
                                    h: extents.paint_height.trunc_to_i32() as u32,
                                },
                                max_bearing_y: segment_max_bearing_y,
                                corresponding_font_boundary: current_segment,
                                corresponding_input_segment: current_segment
                                    + first_internal_split_idx,
                            });
                        } else {
                            let mut last_glyph_idx = 0;
                            let mut x = Fixed::ZERO;
                            for (i, split_end) in self.intra_font_segment_splits
                                [first_internal_split_idx..]
                                .iter()
                                .copied()
                                .take_while(|idx| *idx < end)
                                .chain(std::iter::once(end))
                                .enumerate()
                            {
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
                                        x: x.trunc_to_i32(),
                                        y: total_extents.paint_height.trunc_to_i32(),
                                        w: extents.paint_width.trunc_to_i32() as u32,
                                        h: extents.paint_height.trunc_to_i32() as u32,
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
                        let segment_max_bearing_y = Fixed::new(logical_h as i32);
                        segments.push(ShapedLineSegment {
                            glyphs_and_fonts: None,
                            baseline_offset: (line_extents.paint_width, total_extents.paint_height),
                            paint_rect: PixelRect {
                                x: line_extents.paint_width.trunc_to_i32(),
                                y: total_extents.paint_height.trunc_to_i32(),
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
                            line_extents.paint_height.max(Fixed::new(logical_h as i32));
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
                HorizontalAlignment::Center => -line_extents.paint_width.trunc_to_i32() / 2,
                HorizontalAlignment::Right => -line_extents.paint_width.trunc_to_i32(),
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.0 += aligning_x_offset;
                segment.paint_rect.x += aligning_x_offset;
                if segment.glyphs_and_fonts.is_none() {
                    segment.baseline_offset.1 += max_bearing_y - segment.max_bearing_y;
                } else {
                    segment.baseline_offset.1 += max_bearing_y;
                }
                segment.paint_rect.y += (max_bearing_y - segment.max_bearing_y).trunc_to_i32();
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
                                Fixed::new(x.paint_rect.y)
                                    + Fixed::from_raw(f.metrics().height as i32)
                            }
                            ShaperSegment::Shape(_) => Fixed::new(x.paint_rect.h as i32),
                        },
                    )
                    .max()
                    .unwrap_or(Fixed::ZERO);
            }

            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine {
                segments,
                paint_size: Size2 {
                    w: line_extents.paint_width.trunc_to_i32() as u32,
                    h: line_extents.paint_height.trunc_to_i32() as u32,
                },
            });
        }

        total_rect.h = total_extents.paint_height.trunc_to_i32() as u32;

        if MULTILINE_SHAPER_DEBUG_PRINT {
            println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);
        }

        (lines, total_rect)
    }
}

const DRAW_PERF_DEBUG_INFO: bool = true;
const DRAW_LAYOUT_DEBUG_INFO: bool = false;

pub struct Subrandr {
    logger: log::Logger,
    did_log_version: Cell<bool>,
}

impl Subrandr {
    pub fn init() -> Self {
        Self {
            logger: log::Logger::Default,
            did_log_version: Cell::new(false),
        }
    }
}

// allows for convenient logging with log!(sbr, ...)
impl log::AsLogger for Subrandr {
    fn as_logger(&self) -> &Logger {
        &self.logger
    }
}

struct PerfStats {
    start: std::time::Instant,
    times: VecDeque<f32>,
    times_sum: f32,
}

impl PerfStats {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            times: VecDeque::new(),
            times_sum: 0.0,
        }
    }

    fn start_frame(&mut self) {
        self.start = std::time::Instant::now();
    }

    fn avg_frame_time(&self) -> f32 {
        self.times_sum / self.times.len() as f32
    }

    fn minmax_frame_times(&self) -> (f32, f32) {
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        for time in self.times.iter() {
            min = min.min(*time);
            max = max.max(*time);
        }

        (min, max)
    }

    fn end_frame(&mut self) {
        let end = std::time::Instant::now();
        let time = (end - self.start).as_secs_f32() * 1000.;
        if self.times.len() >= 100 {
            self.times_sum -= self.times.pop_front().unwrap();
        }
        self.times.push_back(time);
        self.times_sum += time;
    }
}

pub struct Renderer<'a> {
    sbr: &'a Subrandr,
    fonts: text::FontSelect,
    dpi: u32,
    subs: &'a Subtitles,
    perf: PerfStats,
}

impl<'a> Renderer<'a> {
    pub fn new(sbr: &'a Subrandr, subs: &'a Subtitles) -> Self {
        if !sbr.did_log_version.get() {
            sbr.did_log_version.set(true);
            info!(
                sbr,
                "subrandr version {} rev {}{}",
                env!("CARGO_PKG_VERSION"),
                env!("BUILD_REV"),
                env!("BUILD_DIRTY")
            );
        }

        Self {
            sbr,
            fonts: text::FontSelect::new().unwrap(),
            dpi: 0,
            subs,
            perf: PerfStats::new(),
        }
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
            .select_simple("monospace", 400., false)
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
            HorizontalAlignment::Center => -extents.paint_width.trunc_to_i32() / 2,
            HorizontalAlignment::Right => (-extents.paint_width
                + Fixed::from_raw(font.horizontal_extents().descender))
            .trunc_to_i32(),
        };

        let oy = match vertical {
            VerticalAlignment::Top => font.horizontal_extents().ascender / 64,
            VerticalAlignment::BaselineCentered => 0,
            VerticalAlignment::Bottom => font.horizontal_extents().descender / 64,
        };

        (ox, oy)
    }

    fn draw_text_full(
        &mut self,
        x: Fixed<6>,
        y: Fixed<6>,
        painter: &mut Painter,
        fonts: &[text::Font],
        glyphs: &[text::Glyph],
        color: BGRA8,
        decoration: &TextDecorations,
        shadows: &[TextShadow],
        scale: f32,
        ctx: &SubtitleContext,
    ) {
        let image = text::render(x.fract(), y.fract(), fonts, glyphs);
        let border = decoration.border * scale;

        // TODO: This should also draw an offset underline I think and possibly strike through
        let mut draw_css_shadow = |shadow: &CssTextShadow| {
            if shadow.color.a > 0 {
                if shadow.blur_radius > f32::EPSILON {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    painter.blit_blurred_monochrome_text(
                        shadow.blur_radius / 2.0,
                        (x + shadow.offset.x).trunc_to_i32(),
                        (y + shadow.offset.y).trunc_to_i32(),
                        image.monochrome(),
                        shadow.color.to_bgr_bytes(),
                        BlendMode::Over,
                    );
                } else {
                    painter.blit_monochrome_text(
                        (x + shadow.offset.x).trunc_to_i32(),
                        (y + shadow.offset.y).trunc_to_i32(),
                        image.monochrome(),
                        shadow.color,
                        BlendMode::Over,
                    );
                }
            }
        };

        let mut out = Vec::new();
        for shadow in shadows.iter().rev() {
            match shadow {
                TextShadow::Css(css_shadow) => draw_css_shadow(css_shadow),
                TextShadow::Srv3(srv3_shadow) => {
                    srv3_shadow.to_css(ctx, &mut out);
                    for css_shadow in out.iter().rev() {
                        draw_css_shadow(css_shadow)
                    }
                    out.clear();
                }
            }
        }

        if decoration.border_color.a > 0 || border.x.max(border.y) >= 1.0 {
            // Draw the border first
            // TODO: in reality the border should probably be blended with the text using
            //       a substraction function (i.e. only draw the border where there is no glyph)
            //       same thing applies to shadow
            let mut x = x;
            let mut rasterizer = NonZeroPolygonRasterizer::new();

            for glyph in glyphs {
                if let Some(outline) = fonts[glyph.font_index].glyph_outline(glyph.index) {
                    let (one, two) = outline::stroke(&outline, border.x, border.y, 1.0);

                    rasterizer.reset();
                    for (a, b) in one.iter_contours().zip(two.iter_contours()) {
                        rasterizer.append_polyline(
                            (x.trunc_to_i32(), y.trunc_to_i32()),
                            &one.flatten_contour(a),
                            false,
                        );
                        rasterizer.append_polyline(
                            (x.trunc_to_i32(), y.trunc_to_i32()),
                            &two.flatten_contour(b),
                            true,
                        );
                    }
                    rasterizer.render_fill(painter, decoration.border_color);
                }

                x += glyph.x_advance;
            }
        }

        let text_end_x = {
            let mut end_x = x;
            let mut it = glyphs.iter();
            if let Some(last) = it.next_back() {
                end_x += fonts[last.font_index].glyph_extents(last.index).width;
            }
            for glyph in it {
                end_x += glyph.x_advance;
            }

            // TODO: ceil_to_i32
            end_x.round_to_i32()
        };

        // TODO: This is actually in TT_Postscript table
        if decoration.underline {
            painter.horizontal_line(
                y.trunc_to_i32(),
                x.trunc_to_i32(),
                text_end_x,
                decoration.underline_color,
            );
        }

        // TODO: This is actually in TT_OS2 table
        if decoration.strike_out {
            let metrics = fonts[0].metrics();
            let strike_y =
                (y - Fixed::from_raw(((metrics.height >> 1) + metrics.descender) as i32))
                    .trunc_to_i32();
            painter.horizontal_line(
                strike_y,
                x.trunc_to_i32(),
                text_end_x,
                decoration.strike_out_color,
            );
        }

        painter.blit_text_image(x.trunc_to_i32(), y.trunc_to_i32(), &image, color);
    }

    pub fn render(&mut self, ctx: &SubtitleContext, t: u32, painter: &mut Painter) {
        if painter.height() == 0 || painter.height() == 0 {
            return;
        }

        self.perf.start_frame();

        painter.clear(BGRA8::ZERO);
        self.dpi = ctx.dpi;

        trace!(
            self.sbr,
            "rendering frame class={} ctx={ctx:?} t={t}ms",
            self.subs.class.get_name()
        );

        if DRAW_PERF_DEBUG_INFO {
            self.debug_text(
                (ctx.padding_left + ctx.video_width) as i32,
                0,
                &format!(
                    "{:.2}x{:.2} dpi:{}",
                    ctx.video_width, ctx.video_height, ctx.dpi
                ),
                Alignment::TopRight,
                16.0,
                BGRA8::from_rgba32(0xFFFFFFFF),
                painter,
            );

            self.debug_text(
                (ctx.padding_left + ctx.video_width) as i32,
                (20.0 * ctx.pixel_scale()) as i32,
                &format!(
                    "l:{:.2} r:{:.2} t:{:.2} b:{:.2}",
                    ctx.padding_left, ctx.padding_right, ctx.padding_top, ctx.padding_bottom
                ),
                Alignment::TopRight,
                16.0,
                BGRA8::from_rgba32(0xFFFFFFFF),
                painter,
            );

            if !self.perf.times.is_empty() {
                let (min, max) = self.perf.minmax_frame_times();
                let avg = self.perf.avg_frame_time();

                self.debug_text(
                    (ctx.padding_left + ctx.video_width) as i32,
                    (40.0 * ctx.pixel_scale()) as i32,
                    &format!(
                        "min={:.1}ms avg={:.1}ms ({:.1}fps) max={:.1}ms ({:.1}fps)",
                        min,
                        avg,
                        1000.0 / avg,
                        max,
                        1000.0 / max
                    ),
                    Alignment::TopRight,
                    16.0,
                    BGRA8::from_rgba32(0xFFFFFFFF),
                    painter,
                );

                let graph_width = 300.0 * ctx.pixel_scale();
                let graph_height = 50.0 * ctx.pixel_scale();
                let offx = (ctx.padding_left + ctx.video_width - graph_width) as i32;
                let mut polyline = vec![];
                for (i, time) in self.perf.times.iter().copied().enumerate() {
                    let x = (i as f32 / self.perf.times.len() as f32) * graph_width;
                    polyline.push(Point2::new(x, -(time / max) * graph_height));
                }

                painter.stroke_polyline(
                    offx,
                    (60.0 * ctx.pixel_scale() + graph_height) as i32,
                    &polyline,
                    BGRA8::new(255, 255, 0, 255),
                );
            }
        }

        let shape_scale = ctx.pixel_scale();

        {
            for event in self
                .subs
                .events
                .iter()
                .filter(|ev| ev.start <= t && ev.end > t)
            {
                let EventLayout {
                    x,
                    y,
                    max_width,
                    max_height,
                } = event.extra.compute_layout(ctx, event);

                let mut shaper = MultilineTextShaper::new();
                for segment in event.segments.iter() {
                    match segment {
                        Segment::Text(segment) => {
                            let font_request = FontRequest {
                                families: segment.font.clone(),
                                weight: util::OrderedF32(segment.font_weight as f32),
                                italic: segment.italic,
                                codepoint: None,
                            };
                            let font = self.fonts.select(&font_request).unwrap().with_size(
                                self.subs.class.get_font_size(ctx, event, segment),
                                ctx.dpi,
                            );

                            shaper.add_text(&segment.text, &font);
                        }
                        Segment::Shape(shape) => {
                            shaper.add_shape(PixelRect {
                                x: (shape.bounding_box.min.x * shape_scale).floor() as i32,
                                y: (shape.bounding_box.min.y * shape_scale).floor() as i32,
                                w: ((shape.bounding_box.size().x + shape.stroke_x / 2.0)
                                    * shape_scale)
                                    .ceil() as u32,
                                h: ((shape.bounding_box.size().y + shape.stroke_y / 2.0)
                                    * shape_scale)
                                    .ceil() as u32,
                            });
                        }
                    }
                }

                let (horizontal_alignment, vertical_alignment) = event.alignment.into_parts();
                let (lines, total_rect) = shaper.shape(
                    horizontal_alignment,
                    TextWrapParams {
                        mode: event.text_wrap,
                        max_width,
                        max_height,
                    },
                    &mut self.fonts,
                );

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => -(total_rect.h as i32) / 2,
                        VerticalAlignment::Bottom => -(total_rect.h as i32),
                    };

                if DRAW_LAYOUT_DEBUG_INFO {
                    painter.stroke_whrect(
                        x + total_rect.x - 1,
                        y + total_rect.y - 1,
                        total_rect.w + 2,
                        total_rect.h + 2,
                        BGRA8::from_rgba32(0xFF00FFFF),
                    );
                }

                let total_position_debug_pos = match vertical_alignment {
                    VerticalAlignment::Top => (total_rect.h as i32 + 20, Alignment::Top),
                    VerticalAlignment::BaselineCentered => {
                        (total_rect.h as i32 + 20, Alignment::Top)
                    }
                    VerticalAlignment::Bottom => (-32, Alignment::Bottom),
                };

                if DRAW_LAYOUT_DEBUG_INFO {
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
                }

                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];

                    let paint_box = (
                        x + shaped_segment.paint_rect.x,
                        y + shaped_segment.paint_rect.y,
                    );

                    if DRAW_LAYOUT_DEBUG_INFO {
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
                                shaped_segment.baseline_offset.0 + x,
                                shaped_segment.baseline_offset.1 + y
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
                                format!(
                                    "{:.0}pt",
                                    self.subs.class.get_font_size(ctx, event, segment)
                                )
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
                            (shaped_segment.baseline_offset.1 + y).trunc_to_i32(),
                            paint_box.0,
                            paint_box.0 + shaped_segment.paint_rect.w as i32,
                            BGRA8::from_rgba32(0x00FF00FF),
                        );
                    }

                    let x = shaped_segment.baseline_offset.0 + x;
                    let y = shaped_segment.baseline_offset.1 + y;

                    match segment {
                        Segment::Text(t) => {
                            let (glyphs, fonts) = shaped_segment.glyphs_and_fonts.as_ref().unwrap();
                            self.draw_text_full(
                                x,
                                y,
                                painter,
                                fonts,
                                glyphs,
                                t.color,
                                &t.decorations,
                                &t.shadows,
                                ctx.pixel_scale(),
                                ctx,
                            );
                        }
                        Segment::Shape(s) => {
                            let mut outline = s.outline.clone();
                            outline.scale(shape_scale);

                            let mut rasterizer = NonZeroPolygonRasterizer::new();
                            if s.fill_color.a > 0 {
                                for c in outline.iter_contours() {
                                    rasterizer.append_polyline(
                                        (x.trunc_to_i32(), y.trunc_to_i32()),
                                        &outline.flatten_contour(c),
                                        false,
                                    );
                                    rasterizer.render_fill(painter, s.fill_color);
                                }
                            }

                            if s.stroke_color.a > 0 && (s.stroke_x >= 0.01 || s.stroke_y >= 0.01) {
                                let stroked = outline::stroke(
                                    &outline,
                                    s.stroke_x * shape_scale / 2.0,
                                    s.stroke_y * shape_scale / 2.0,
                                    1.0,
                                );

                                for (a, b) in
                                    stroked.0.iter_contours().zip(stroked.1.iter_contours())
                                {
                                    rasterizer.reset();
                                    rasterizer.append_polyline(
                                        (x.trunc_to_i32(), y.trunc_to_i32()),
                                        &stroked.0.flatten_contour(a),
                                        false,
                                    );
                                    rasterizer.append_polyline(
                                        (x.trunc_to_i32(), y.trunc_to_i32()),
                                        &stroked.1.flatten_contour(b),
                                        true,
                                    );
                                    rasterizer.render_fill(painter, s.stroke_color);
                                }

                                painter.debug_stroke_outline(
                                    x.trunc_to_i32(),
                                    y.trunc_to_i32(),
                                    &stroked.0,
                                    BGRA8::from_rgba32(0xFF0000FF),
                                    false,
                                );
                                painter.debug_stroke_outline(
                                    x.trunc_to_i32(),
                                    y.trunc_to_i32(),
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

        self.perf.end_frame();
    }
}
