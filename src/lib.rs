#![allow(clippy::too_many_arguments)]
#![allow(clippy::missing_transmute_annotations)]

use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    fmt::Debug,
    ops::Range,
};

use thiserror::Error;

use color::BGRA8;
use log::{info, trace, Logger};
use math::{I16Dot16, Point2, Point2f, Rect2, Vec2, Vec2f};
use outline::{OutlineBuilder, SegmentDegree};
use rasterize::{polygon::NonZeroPolygonRasterizer, Rasterizer, RenderTarget};
use srv3::{Srv3Event, Srv3TextShadow};
use text::{
    layout::{MultilineTextShaper, ShapedLine, TextWrapParams},
    FontArena, FreeTypeError, TextMetrics,
};
use vtt::VttEvent;

pub mod srv3;
pub mod vtt;

mod capi;
mod color;
mod log;
mod math;
mod outline;
pub mod rasterize;
mod text;
mod util;

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
    pub const fn from_parts(horiz: HorizontalAlignment, vert: VerticalAlignment) -> Alignment {
        match (horiz, vert) {
            (HorizontalAlignment::Left, VerticalAlignment::Top) => Alignment::TopLeft,
            (HorizontalAlignment::Left, VerticalAlignment::BaselineCentered) => Alignment::Left,
            (HorizontalAlignment::Left, VerticalAlignment::Bottom) => Alignment::BottomLeft,
            (HorizontalAlignment::Center, VerticalAlignment::Top) => Alignment::Top,
            (HorizontalAlignment::Center, VerticalAlignment::BaselineCentered) => Alignment::Center,
            (HorizontalAlignment::Center, VerticalAlignment::Bottom) => Alignment::Bottom,
            (HorizontalAlignment::Right, VerticalAlignment::Top) => Alignment::TopRight,
            (HorizontalAlignment::Right, VerticalAlignment::BaselineCentered) => Alignment::Right,
            (HorizontalAlignment::Right, VerticalAlignment::Bottom) => Alignment::BottomRight,
        }
    }

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
    /// Greedy line breaking.
    Normal,
    None,
}

trait Layouter {
    fn wrap_width(&self, ctx: &SubtitleContext, event: &Event) -> f32;

    fn layout(
        &mut self,
        ctx: &SubtitleContext,
        lines: &mut Vec<ShapedLine>,
        total_rect: &mut Rect2<I26Dot6>,
        event: &Event,
    ) -> Point2f;
}

struct TestLayouter;

impl Layouter for TestLayouter {
    fn wrap_width(&self, ctx: &SubtitleContext, _event: &Event) -> f32 {
        ctx.player_width().into_f32()
    }

    fn layout(
        &mut self,
        ctx: &SubtitleContext,
        _lines: &mut Vec<ShapedLine>,
        _total_rect: &mut Rect2<I26Dot6>,
        event: &Event,
    ) -> Point2f {
        let &EventExtra::Test { x, y } = &event.extra else {
            panic!("TestLayouter received foreign event {:?}", event);
        };

        Point2f::new(
            (ctx.padding_left + (ctx.video_width * x)).into_f32(),
            (ctx.padding_top + (ctx.video_height * y)).into_f32(),
        )
    }
}

#[derive(Debug, Clone)]
enum EventExtra {
    Srv3(Srv3Event),
    Vtt(VttEvent),
    Test { x: f32, y: f32 },
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
    font_size: I26Dot6,
    font_weight: I16Dot16,
    italic: bool,
    decorations: TextDecorations,
    color: BGRA8,
    background_color: BGRA8,
    text: String,
    shadows: Vec<TextShadow>,
    ruby: Ruby,
}

#[derive(Debug, Clone, Copy)]
enum Ruby {
    None,
    Base,
    Over,
}

#[derive(Debug, Clone)]
struct TextDecorations {
    border: Vec2f,
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
    offset: Vec2f,
    blur_radius: I26Dot6,
    color: BGRA8,
}

impl TextDecorations {
    pub const fn none() -> Self {
        Self {
            border: Vec2f::ZERO,
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
    bounding_box: math::Rect2f,
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

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
// TODO: Maybe call this a viewport or have a field called "viewport"
pub struct SubtitleContext {
    pub dpi: u32,
    pub video_width: I26Dot6,
    pub video_height: I26Dot6,
    pub padding_left: I26Dot6,
    pub padding_right: I26Dot6,
    pub padding_top: I26Dot6,
    pub padding_bottom: I26Dot6,
}

pub use math::I26Dot6;

impl SubtitleContext {
    pub fn ppi(&self) -> u32 {
        self.dpi * 96 / 72
    }

    pub fn pixel_scale(&self) -> f32 {
        self.dpi as f32 / 72.0
    }

    pub fn padding_width(&self) -> I26Dot6 {
        self.padding_left + self.padding_right
    }

    pub fn padding_height(&self) -> I26Dot6 {
        self.padding_top + self.padding_bottom
    }

    pub fn player_width(&self) -> I26Dot6 {
        self.video_width + self.padding_width()
    }

    pub fn player_height(&self) -> I26Dot6 {
        self.video_height + self.padding_height()
    }
}

#[derive(Debug)]
struct SubtitleClass {
    name: &'static str,
    get_font_size: fn(ctx: &SubtitleContext, event: &Event, segment: &TextSegment) -> I26Dot6,
    create_layouter: fn() -> Box<dyn Layouter>,
}

// Font size passed through directly.
// Coordinate system 0.0-1.0 percentages
const TEST_SUBTITLE_CLASS: SubtitleClass = SubtitleClass {
    name: "<test>",
    get_font_size: |_ctx: &SubtitleContext, _event: &Event, segment: &TextSegment| -> I26Dot6 {
        segment.font_size
    },
    create_layouter: || Box::new(TestLayouter),
};

#[derive(Debug)]
pub struct Subtitles {
    class: &'static SubtitleClass,
    events: Vec<Event>,
}

impl Subtitles {
    pub const fn empty() -> Self {
        Self {
            class: &TEST_SUBTITLE_CLASS,
            events: vec![],
        }
    }

    #[doc(hidden)]
    pub fn test_new() -> Self {
        Self {
            class: &TEST_SUBTITLE_CLASS,
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
                            font_size: I26Dot6::new(64),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            background_color: BGRA8::ZERO,
                            text: "this ".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: I26Dot6::new(64),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0x0000FFFF),
                            background_color: BGRA8::ZERO,
                            text: "is\n".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["Liberation Sans".to_string()],
                            font_size: I26Dot6::new(64),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF0000FF),
                            background_color: BGRA8::ZERO,
                            text: "mu".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["monospace".to_string()],
                            font_size: I26Dot6::new(48),
                            font_weight: I16Dot16::new(700),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFF00FF),
                            background_color: BGRA8::ZERO,
                            text: "ltil".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["Arial".to_string()],
                            font_size: I26Dot6::new(80),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF00FFFF),
                            background_color: BGRA8::ZERO,
                            text: "iね❌".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Shape(ShapeSegment::new(
                            {
                                let mut b = OutlineBuilder::new();
                                b.add_point(Point2f::new(0.0, 0.0));
                                b.add_point(Point2f::new(30.0, 120.));
                                b.add_point(Point2f::new(120.0, 120.));
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
                        font_size: I26Dot6::new(64),
                        font_weight: I16Dot16::new(400),
                        italic: false,
                        decorations: TextDecorations {
                            border: Vec2f::new(2.0, 2.0),
                            border_color: BGRA8::new(255, 0, 0, 255),
                            underline: true,
                            underline_color: BGRA8::new(255, 255, 255, 255),
                            strike_out: true,
                            strike_out_color: BGRA8::new(255, 255, 255, 255),
                        },
                        color: BGRA8::from_rgba32(0x00FF00AA),
                        background_color: BGRA8::ZERO,
                        text: "this is for comparison".to_string(),
                        shadows: Vec::new(),
                        ruby: Ruby::None,
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
                            font_size: I26Dot6::new(26),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0x00FF00AA),
                            background_color: BGRA8::ZERO,
                            text: "mo".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["sans-serif".to_string()],
                            font_size: I26Dot6::new(26),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFF00AA),
                            background_color: BGRA8::ZERO,
                            text: "ment".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
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
                        font_size: I26Dot6::new(26),
                        font_weight: I16Dot16::new(400),
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0x0000FFAA),
                        background_color: BGRA8::ZERO,
                        text: "moment".to_string(),
                        shadows: Vec::new(),
                        ruby: Ruby::None,
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
                        font_size: I26Dot6::new(64),
                        font_weight: I16Dot16::new(400),
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0x00FF0099),
                        background_color: BGRA8::ZERO,
                        text: "with shadows".to_string(),
                        shadows: vec![
                            TextShadow::Css(CssTextShadow {
                                offset: Vec2f::new(80.0, 80.0),
                                blur_radius: I26Dot6::from_f32(7.5),
                                color: BGRA8::new(0, 0, 255, 255),
                            }),
                            TextShadow::Css(CssTextShadow {
                                offset: Vec2f::new(48.0, 48.0),
                                blur_radius: I26Dot6::from_f32(20.0),
                                color: BGRA8::new(255, 255, 255, 255),
                            }),
                        ],
                        ruby: Ruby::None,
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
                        font_size: I26Dot6::new(64),
                        font_weight: I16Dot16::new(700),
                        italic: false,
                        decorations: TextDecorations::none(),
                        color: BGRA8::from_rgba32(0xFFFFFFFF),
                        background_color: BGRA8::ZERO,
                        text: "this is bold..".to_string(),
                        shadows: Vec::new(),
                        ruby: Ruby::None,
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    extra: EventExtra::Test { x: 0.5, y: 0.5 },
                    alignment: Alignment::Center,
                    text_wrap: TextWrapMode::None,
                    segments: vec![
                        Segment::Text(TextSegment {
                            font: vec!["sans-serif".to_string()],
                            font_size: I26Dot6::new(64) * 96 / 72,
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0x00000000),
                            background_color: BGRA8::ZERO,
                            text: "嗚呼ー".to_string(),
                            shadows: vec![TextShadow::Css(CssTextShadow {
                                offset: Vec2f::ZERO,
                                blur_radius: I26Dot6::from_f32(15.0),
                                color: BGRA8::new(0, 0, 255, 255),
                            })],
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["sans-serif".to_string()],
                            font_size: I26Dot6::new(64) * 96 / 72,
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFF000099),
                            background_color: BGRA8::ZERO,
                            text: "helloworld".to_string(),
                            shadows: vec![TextShadow::Css(CssTextShadow {
                                offset: Vec2f::ZERO,
                                blur_radius: I26Dot6::from_f32(15.0),
                                color: BGRA8::new(0, 0, 255, 255),
                            })],
                            ruby: Ruby::None,
                        }),
                    ],
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
                            font_size: I26Dot6::new(32),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            background_color: BGRA8::ZERO,
                            text: "😭".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                        Segment::Text(TextSegment {
                            font: vec!["emoji".to_string()],
                            font_size: I26Dot6::new(64),
                            font_weight: I16Dot16::new(400),
                            italic: false,
                            decorations: TextDecorations::none(),
                            color: BGRA8::from_rgba32(0xFFFFFFFF),
                            background_color: BGRA8::ZERO,
                            text: "😭".to_string(),
                            shadows: Vec::new(),
                            ruby: Ruby::None,
                        }),
                    ],
                },
            ],
        }
    }
}

#[derive(Default, Debug, Clone)]
struct DebugFlags {
    draw_version_string: bool,
    draw_perf_info: bool,
    draw_layout_info: bool,
    stroke_shape_outlines: bool,
}

impl DebugFlags {
    fn from_env() -> Self {
        let mut result = Self::default();

        if let Ok(s) = std::env::var("SBR_DEBUG") {
            for token in s.split(",") {
                match token {
                    "draw_version" => result.draw_version_string = true,
                    "draw_perf" => result.draw_perf_info = true,
                    "draw_layout" => result.draw_layout_info = true,
                    "draw_shape_outlines" => result.stroke_shape_outlines = true,
                    _ => (),
                }
            }
        }

        result
    }
}

pub struct Subrandr {
    logger: log::Logger,
    did_log_version: Cell<bool>,
    debug: DebugFlags,
}

impl Subrandr {
    pub fn init() -> Self {
        Self {
            logger: log::Logger::Default,
            did_log_version: Cell::new(false),
            debug: DebugFlags::from_env(),
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

    fn end_frame(&mut self) -> f32 {
        let end = std::time::Instant::now();
        let time = (end - self.start).as_secs_f32() * 1000.;
        if self.times.len() >= 100 {
            self.times_sum -= self.times.pop_front().unwrap();
        }
        self.times.push_back(time);
        self.times_sum += time;
        time
    }
}

pub struct Renderer<'a> {
    sbr: &'a Subrandr,
    fonts: text::FontSelect,
    dpi: u32,
    perf: PerfStats,

    unchanged_range: Range<u32>,
    previous_context: SubtitleContext,
    previous_output_size: (u32, u32),
}

impl<'a> Renderer<'a> {
    pub fn new(sbr: &'a Subrandr) -> Self {
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
            fonts: text::FontSelect::new(sbr).unwrap(),
            dpi: 0,
            perf: PerfStats::new(),
            unchanged_range: 0..0,
            previous_context: SubtitleContext {
                dpi: 0,
                video_width: I26Dot6::ZERO,
                video_height: I26Dot6::ZERO,
                padding_left: I26Dot6::ZERO,
                padding_right: I26Dot6::ZERO,
                padding_top: I26Dot6::ZERO,
                padding_bottom: I26Dot6::ZERO,
            },
            previous_output_size: (0, 0),
        }
    }

    pub fn library(&self) -> &'a Subrandr {
        self.sbr
    }

    pub fn invalidate_subtitles(&mut self) {
        self.unchanged_range = 0..0;
    }

    pub fn unchanged_inside(&self) -> Range<u32> {
        self.unchanged_range.clone()
    }

    pub fn did_change(&self, ctx: &SubtitleContext, t: u32) -> bool {
        self.previous_context != *ctx || !self.unchanged_range.contains(&t)
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
    #[error(transparent)]
    FontSelect(#[from] text::font_select::Error),
    #[error(transparent)]
    SimpleShaping(#[from] text::ShapingError),
    #[error(transparent)]
    Layout(#[from] text::layout::LayoutError),
}

impl<'a> Renderer<'a> {
    fn debug_text(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        x: i32,
        y: i32,
        text: &str,
        alignment: Alignment,
        size: I26Dot6,
        color: BGRA8,
    ) -> Result<(), RenderError> {
        let font = self
            .fonts
            .select_simple("monospace", I16Dot16::new(400), false)?
            .with_size(size, self.dpi)?;
        let font_arena = FontArena::new();
        let glyphs = text::simple_shape_text(&font, &font_arena, text)?;
        let (ox, oy) = Self::translate_for_aligned_text(
            &font,
            true,
            &text::compute_extents_ex(true, &glyphs)?,
            alignment,
        );

        let image = text::render(rasterizer, I26Dot6::ZERO, I26Dot6::ZERO, &glyphs)?;
        image.blit(rasterizer, target, x + ox, y + oy, color);

        Ok(())
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextMetrics,
        alignment: Alignment,
    ) -> (i32, i32) {
        assert!(horizontal);

        let (horizontal, vertical) = alignment.into_parts();

        let ox = match horizontal {
            HorizontalAlignment::Left => -font.horizontal_extents().descender / 64 / 2,
            HorizontalAlignment::Center => -extents.paint_size.x.trunc_to_inner() / 2,
            HorizontalAlignment::Right => (-extents.paint_size.x
                + I26Dot6::from_raw(font.horizontal_extents().descender))
            .trunc_to_inner(),
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
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        x: I26Dot6,
        y: I26Dot6,
        glyphs: &[text::Glyph],
        color: BGRA8,
        decoration: &TextDecorations,
        shadows: &[TextShadow],
        scale: f32,
        ctx: &SubtitleContext,
    ) -> Result<(), RenderError> {
        if glyphs.is_empty() {
            // TODO: Maybe instead ensure empty segments aren't emitted during layout?
            return Ok(());
        }

        let image = text::render(rasterizer, x.fract(), y.fract(), glyphs)?;
        let border = decoration.border * scale;

        let mut blurs = HashMap::new();

        // TODO: This should also draw an offset underline I think and possibly strike through?
        let mut draw_css_shadow = |shadow: &CssTextShadow| {
            if shadow.color.a > 0 {
                if shadow.blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    let sigma = shadow.blur_radius / 2;

                    let (blurred, offset) = blurs.entry(sigma).or_insert_with(|| {
                        let offset = image.prepare_for_blur(rasterizer, sigma.into_f32());
                        let padding = rasterizer.blur_padding();
                        (
                            rasterizer.blur_to_mono_texture(),
                            -Vec2f::new(offset.x as f32, offset.y as f32) + padding,
                        )
                    });

                    rasterizer.blit(
                        target,
                        (x + shadow.offset.x - offset.x).trunc_to_inner(),
                        (y + shadow.offset.y - offset.y).trunc_to_inner(),
                        blurred,
                        shadow.color,
                    );
                } else {
                    let monochrome = image.monochrome(rasterizer);
                    monochrome.blit(
                        rasterizer,
                        target,
                        (x + monochrome.offset.x + shadow.offset.x).trunc_to_inner(),
                        (y + monochrome.offset.y + shadow.offset.y).trunc_to_inner(),
                        shadow.color,
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
            //       check how libass handles it
            let mut x = x;
            let mut poly_rasterizer = NonZeroPolygonRasterizer::new();

            poly_rasterizer.reset();
            for glyph in glyphs {
                if let Some(outline) = glyph.font.glyph_outline(glyph.index)? {
                    let (one, two) = outline.stroke(border.x, border.y, 1.0);

                    for (a, b) in one.iter_contours().zip(two.iter_contours()) {
                        poly_rasterizer.append_polyline(
                            (x.trunc_to_inner(), y.trunc_to_inner()),
                            &one.flatten_contour(a),
                            false,
                        );
                        poly_rasterizer.append_polyline(
                            (x.trunc_to_inner(), y.trunc_to_inner()),
                            &two.flatten_contour(b),
                            true,
                        );
                    }
                }

                x += glyph.x_advance;
            }
            rasterizer.blit_cpu_polygon(target, &mut poly_rasterizer, decoration.border_color);
        }

        let text_end_x = {
            let mut end_x = x;

            // TODO: Should this somehow ignore trailing advance?
            //       The issue with that is that it causes issues with cross-segment decorations
            //       so it would have to take those into account.
            for glyph in glyphs.iter() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        image.blit(
            rasterizer,
            target,
            x.trunc_to_inner(),
            y.trunc_to_inner(),
            color,
        );

        // FIXME: This should use the main font for the segment, not the font
        //        of the first glyph..
        let font_metrics = glyphs[0].font.metrics();

        if decoration.underline {
            let thickness = font_metrics.underline_thickness;
            rasterizer.fill_axis_aligned_antialias_rect(
                target,
                Rect2::new(
                    Point2::new(x.into_f32(), y.into_f32()),
                    Point2::new(text_end_x.into_f32(), (y + thickness).into_f32()),
                ),
                decoration.underline_color,
            );
        }

        if decoration.strike_out {
            let strike_y = y + font_metrics.strikeout_top_offset;
            let thickness = font_metrics.strikeout_thickness;
            rasterizer.fill_axis_aligned_antialias_rect(
                target,
                Rect2::new(
                    Point2::new(x.into_f32(), strike_y.into_f32()),
                    Point2::new(text_end_x.into_f32(), (strike_y + thickness).into_f32()),
                ),
                decoration.strike_out_color,
            );
        }

        Ok(())
    }

    pub fn render(
        &mut self,
        ctx: &SubtitleContext,
        t: u32,
        subs: &Subtitles,
        buffer: &mut [BGRA8],
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        self.previous_context = *ctx;
        self.previous_output_size = (width, height);

        buffer.fill(BGRA8::ZERO);
        self.render_to(
            &mut rasterize::sw::Rasterizer::new(),
            &mut rasterize::sw::create_render_target(buffer, width, height),
            ctx,
            t,
            subs,
        )
    }

    // FIXME: This is kinda ugly but `render_to` cannot be public without
    //        exposing the Rasterizer trait.
    //        Maybe just do it and mark it #[doc(hidden)]?
    #[cfg(feature = "wgpu")]
    pub fn render_to_wgpu(
        &mut self,
        rasterizer: &mut rasterize::wgpu::Rasterizer,
        mut target: RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
        subs: &Subtitles,
    ) -> Result<(), RenderError> {
        self.render_to(rasterizer, &mut target, ctx, t, subs)?;
        rasterizer.submit_render(target);
        Ok(())
    }

    fn render_to(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
        subs: &Subtitles,
    ) -> Result<(), RenderError> {
        let (target_width, target_height) = (target.width(), target.width());
        if target_width == 0 || target_height == 0 {
            return Ok(());
        }

        self.perf.start_frame();
        self.fonts.advance_cache_generation();

        self.dpi = ctx.dpi;

        trace!(
            self.sbr,
            "rendering frame (class={} ctx={ctx:?} t={t}ms)",
            subs.class.name
        );

        // FIXME: Currently mpv does not seem to have a way to pass the correct DPI
        //        to a subtitle renderer so this doesn't work.
        let debug_font_size = I26Dot6::new(16);
        let debug_line_height = I26Dot6::new(20) * ctx.pixel_scale();

        if self.sbr.debug.draw_version_string {
            self.debug_text(
                rasterizer,
                target,
                0,
                0,
                concat!(
                    "subrandr ",
                    env!("CARGO_PKG_VERSION"),
                    " rev ",
                    env!("BUILD_REV")
                ),
                Alignment::TopLeft,
                debug_font_size,
                BGRA8::WHITE,
            )?;

            self.debug_text(
                rasterizer,
                target,
                0,
                debug_line_height.round_to_inner(),
                &format!("subtitle class: {}", subs.class.name),
                Alignment::TopLeft,
                debug_font_size,
                BGRA8::WHITE,
            )?;

            let rasterizer_line = format!("rasterizer: {}", rasterizer.name());
            self.debug_text(
                rasterizer,
                target,
                0,
                (debug_line_height * 2).round_to_inner(),
                &rasterizer_line,
                Alignment::TopLeft,
                debug_font_size,
                BGRA8::WHITE,
            )?;

            if let Some(adapter_line) = rasterizer.adapter_info_string() {
                self.debug_text(
                    rasterizer,
                    target,
                    0,
                    (debug_line_height * 3).round_to_inner(),
                    &adapter_line,
                    Alignment::TopLeft,
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
            }
        }

        if self.sbr.debug.draw_perf_info {
            self.debug_text(
                rasterizer,
                target,
                (ctx.padding_left + ctx.video_width).round_to_inner(),
                0,
                &format!(
                    "{:.2}x{:.2} dpi:{}",
                    ctx.video_width, ctx.video_height, ctx.dpi
                ),
                Alignment::TopRight,
                debug_font_size,
                BGRA8::WHITE,
            )?;

            self.debug_text(
                rasterizer,
                target,
                (ctx.padding_left + ctx.video_width).round_to_inner(),
                debug_line_height.round_to_inner(),
                &format!(
                    "l:{:.2} r:{:.2} t:{:.2} b:{:.2}",
                    ctx.padding_left, ctx.padding_right, ctx.padding_top, ctx.padding_bottom
                ),
                Alignment::TopRight,
                debug_font_size,
                BGRA8::WHITE,
            )?;

            if !self.perf.times.is_empty() {
                let (min, max) = self.perf.minmax_frame_times();
                let avg = self.perf.avg_frame_time();

                self.debug_text(
                    rasterizer,
                    target,
                    (ctx.padding_left + ctx.video_width).round_to_inner(),
                    (debug_line_height * 2).round_to_inner(),
                    &format!(
                        "min={:.1}ms avg={:.1}ms ({:.1}fps) max={:.1}ms ({:.1}fps)",
                        min,
                        avg,
                        1000.0 / avg,
                        max,
                        1000.0 / max
                    ),
                    Alignment::TopRight,
                    debug_font_size,
                    BGRA8::WHITE,
                )?;

                if let Some(&last) = self.perf.times.iter().last() {
                    self.debug_text(
                        rasterizer,
                        target,
                        (ctx.padding_left + ctx.video_width).round_to_inner(),
                        (debug_line_height * 3).round_to_inner(),
                        &format!("last={:.1}ms ({:.1}fps)", last, 1000.0 / last),
                        Alignment::TopRight,
                        debug_font_size,
                        BGRA8::WHITE,
                    )?;
                }

                let graph_width = I26Dot6::new(300) * ctx.pixel_scale();
                let graph_height = I26Dot6::new(50) * ctx.pixel_scale();
                let offx = (ctx.padding_left + ctx.video_width - graph_width).round_to_inner();
                let mut polyline = vec![];
                for (i, time) in self.perf.times.iter().copied().enumerate() {
                    let x = (graph_width * i as i32 / self.perf.times.len() as i32).into_f32();
                    let y = -(graph_height * time / max).into_f32();
                    polyline.push(Point2f::new(x, y));
                }

                rasterizer.stroke_polyline(
                    target,
                    Vec2::new(
                        offx as f32,
                        ((debug_line_height * 4.5) + graph_height).into_f32(),
                    ),
                    &polyline,
                    BGRA8::new(255, 255, 0, 255),
                );
            }
        }

        let shape_scale = ctx.pixel_scale();

        let mut unchanged_range: Range<u32> = 0..u32::MAX;
        {
            let mut layouter = (subs.class.create_layouter)();
            let font_arena = FontArena::new();
            for event in subs.events.iter() {
                let r = unchanged_range.clone();
                if (event.start..event.end).contains(&t) {
                    unchanged_range = r.start.max(event.start)..r.end.min(event.end);
                } else {
                    if event.start > t {
                        unchanged_range = r.start..r.end.min(event.start);
                    } else {
                        unchanged_range = r.start.max(event.end)..r.end;
                    }
                    continue;
                }

                let wrap_width = layouter.wrap_width(ctx, event);

                let mut shaper = MultilineTextShaper::new();
                let mut last_ruby_base = None;
                for segment in event.segments.iter() {
                    match segment {
                        Segment::Text(segment) => {
                            let font_request = text::FontRequest {
                                families: segment
                                    .font
                                    .iter()
                                    .map(AsRef::as_ref)
                                    .map(Into::into)
                                    .collect(),
                                weight: segment.font_weight,
                                italic: segment.italic,
                                codepoint: None,
                            };
                            let font =
                                font_arena.insert(&self.fonts.select(&font_request)?.with_size(
                                    (subs.class.get_font_size)(ctx, event, segment),
                                    ctx.dpi,
                                )?);

                            match segment.ruby {
                                Ruby::None => {
                                    shaper.add_text(&segment.text, &font);
                                }
                                Ruby::Base => {
                                    last_ruby_base =
                                        Some(shaper.add_ruby_base(&segment.text, &font));
                                }
                                Ruby::Over => {
                                    shaper.add_ruby_annotation(
                                        last_ruby_base
                                            .expect("Ruby::Over without preceding Ruby::Base"),
                                        &segment.text,
                                        font,
                                    );
                                    last_ruby_base = None;
                                }
                            }
                        }
                        Segment::Shape(shape) => {
                            shaper.add_shape(Rect2::new(
                                Point2::new(
                                    (shape.bounding_box.min.x * shape_scale).floor() as i32,
                                    (shape.bounding_box.min.y * shape_scale).floor() as i32,
                                ),
                                Point2::new(
                                    ((shape.bounding_box.max.x + shape.stroke_x / 2.0)
                                        * shape_scale)
                                        .ceil() as i32,
                                    ((shape.bounding_box.max.y + shape.stroke_y / 2.0)
                                        * shape_scale)
                                        .ceil() as i32,
                                ),
                            ));
                        }
                    }
                }

                let (horizontal_alignment, vertical_alignment) = event.alignment.into_parts();
                let (mut lines, mut total_rect) = shaper.shape(
                    horizontal_alignment,
                    TextWrapParams {
                        mode: event.text_wrap,
                        wrap_width,
                    },
                    &font_arena,
                    &mut self.fonts,
                )?;

                let Point2 { x, y } = layouter.layout(ctx, &mut lines, &mut total_rect, event);

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => {
                            -total_rect.height().trunc_to_inner() / 2
                        }
                        VerticalAlignment::Bottom => -total_rect.height().trunc_to_inner(),
                    };

                let final_total_rect = total_rect.translate(Vec2::new(x, y));

                if self.sbr.debug.draw_layout_info {
                    rasterizer.stroke_axis_aligned_rect(
                        target,
                        Rect2::new(
                            Point2::new(
                                final_total_rect.min.x.into_f32() - 1.,
                                final_total_rect.min.y.into_f32() - 1.,
                            ),
                            Point2::new(
                                final_total_rect.max.x.into_f32() + 2.,
                                final_total_rect.max.y.into_f32() + 2.,
                            ),
                        ),
                        BGRA8::MAGENTA,
                    );
                }

                let total_position_debug_pos = match vertical_alignment {
                    VerticalAlignment::Top => (total_rect.height() + 20, Alignment::Top),
                    VerticalAlignment::BaselineCentered => {
                        (total_rect.height() + 20, Alignment::Top)
                    }
                    VerticalAlignment::Bottom => (I26Dot6::new(-24), Alignment::Bottom),
                };

                if self.sbr.debug.draw_layout_info {
                    self.debug_text(
                        rasterizer,
                        target,
                        (final_total_rect.min.x + final_total_rect.width() / 2).trunc_to_inner(),
                        (final_total_rect.min.y + total_position_debug_pos.0).trunc_to_inner(),
                        &format!(
                            "x:{:.1} y:{:.1} w:{:.1} h:{:.1}",
                            final_total_rect.x(),
                            final_total_rect.y(),
                            final_total_rect.width(),
                            final_total_rect.height()
                        ),
                        total_position_debug_pos.1,
                        debug_font_size,
                        BGRA8::MAGENTA,
                    )?;
                }

                // TODO: A trait for casting these types?
                fn convert_rect(rect: Rect2<I26Dot6>) -> Rect2<f32> {
                    Rect2::new(
                        Point2::new(rect.min.x.into_f32(), rect.min.y.into_f32()),
                        Point2::new(rect.max.x.into_f32(), rect.max.y.into_f32()),
                    )
                }

                for line in &lines {
                    let mut current_background_box = Rect2::<I26Dot6>::NOTHING;
                    let mut last_segment_index = usize::MAX;

                    for shaped_segment in &line.segments {
                        if shaped_segment.corresponding_input_segment != last_segment_index {
                            if last_segment_index != usize::MAX {
                                match &event.segments[last_segment_index] {
                                    Segment::Text(text) => {
                                        if text.background_color.a != 0 {
                                            rasterizer.fill_axis_aligned_rect(
                                                target,
                                                convert_rect(current_background_box)
                                                    .translate(Vec2::new(x as f32, y as f32)),
                                                text.background_color,
                                            );
                                        }
                                    }
                                    Segment::Shape(_) => {}
                                }
                            }

                            current_background_box = shaped_segment.logical_rect;
                            last_segment_index = shaped_segment.corresponding_input_segment;
                        } else {
                            current_background_box.expand_to_point(shaped_segment.logical_rect.max);
                        }
                    }

                    // FIXME: Background boxes should have corner radius (with SRV3, not WebVTT)
                    if last_segment_index != usize::MAX {
                        match &event.segments[last_segment_index] {
                            Segment::Text(text) => {
                                if text.background_color.a != 0 {
                                    rasterizer.fill_axis_aligned_rect(
                                        target,
                                        convert_rect(current_background_box)
                                            .translate(Vec2::new(x as f32, y as f32)),
                                        text.background_color,
                                    );
                                }
                            }
                            Segment::Shape(_) => (),
                        }
                    }
                }

                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];

                    let final_logical_box = convert_rect(shaped_segment.logical_rect)
                        .translate(Vec2::new(x as f32, y as f32));

                    if self.sbr.debug.draw_layout_info {
                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.min.x as i32,
                            final_logical_box.min.y as i32,
                            &format!(
                                "{:.0},{:.0}",
                                shaped_segment.logical_rect.min.x + x,
                                shaped_segment.logical_rect.min.y + y
                            ),
                            Alignment::BottomLeft,
                            debug_font_size,
                            BGRA8::RED,
                        )?;

                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.min.x as i32,
                            final_logical_box.max.y as i32,
                            &format!("{:.1}", shaped_segment.baseline_offset.x),
                            Alignment::TopLeft,
                            debug_font_size,
                            BGRA8::RED,
                        )?;

                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.max.x as i32,
                            final_logical_box.min.y as i32,
                            &if let Segment::Text(segment) = segment {
                                format!("{:.0}pt", (subs.class.get_font_size)(ctx, event, segment))
                            } else {
                                "shape".to_owned()
                            },
                            Alignment::BottomRight,
                            debug_font_size,
                            BGRA8::GOLD,
                        )?;

                        rasterizer.stroke_axis_aligned_rect(target, final_logical_box, BGRA8::BLUE);

                        rasterizer.horizontal_line(
                            target,
                            (shaped_segment.baseline_offset.y + y).into_f32(),
                            final_logical_box.min.x,
                            final_logical_box.max.x,
                            BGRA8::GREEN,
                        );
                    }

                    let x = shaped_segment.baseline_offset.x + x;
                    let y = shaped_segment.baseline_offset.y + y;

                    match segment {
                        Segment::Text(t) => {
                            let glyphs = shaped_segment.glyphs.as_ref().unwrap();
                            self.draw_text_full(
                                rasterizer,
                                target,
                                x,
                                y,
                                glyphs,
                                t.color,
                                &t.decorations,
                                &t.shadows,
                                ctx.pixel_scale(),
                                ctx,
                            )?;
                        }
                        Segment::Shape(s) => {
                            let mut outline = s.outline.clone();
                            outline.scale(shape_scale);

                            let mut poly_rasterizer = NonZeroPolygonRasterizer::new();
                            if s.fill_color.a > 0 {
                                for c in outline.iter_contours() {
                                    poly_rasterizer.append_polyline(
                                        (x.trunc_to_inner(), y.trunc_to_inner()),
                                        &outline.flatten_contour(c),
                                        false,
                                    );
                                    rasterizer.blit_cpu_polygon(
                                        target,
                                        &mut poly_rasterizer,
                                        s.fill_color,
                                    );
                                }
                            }

                            if s.stroke_color.a > 0 && (s.stroke_x >= 0.01 || s.stroke_y >= 0.01) {
                                let stroked = outline.stroke(
                                    s.stroke_x * shape_scale / 2.0,
                                    s.stroke_y * shape_scale / 2.0,
                                    1.0,
                                );

                                for (a, b) in
                                    stroked.0.iter_contours().zip(stroked.1.iter_contours())
                                {
                                    poly_rasterizer.reset();
                                    poly_rasterizer.append_polyline(
                                        (x.trunc_to_inner(), y.trunc_to_inner()),
                                        &stroked.0.flatten_contour(a),
                                        false,
                                    );
                                    poly_rasterizer.append_polyline(
                                        (x.trunc_to_inner(), y.trunc_to_inner()),
                                        &stroked.1.flatten_contour(b),
                                        true,
                                    );
                                    rasterizer.blit_cpu_polygon(
                                        target,
                                        &mut poly_rasterizer,
                                        s.stroke_color,
                                    );
                                }

                                if self.sbr.debug.stroke_shape_outlines {
                                    rasterize::polygon::debug_stroke_outline(
                                        rasterizer,
                                        target,
                                        x.into_f32(),
                                        y.into_f32(),
                                        &stroked.0,
                                        BGRA8::from_rgba32(0xFF0000FF),
                                        false,
                                    );
                                    rasterize::polygon::debug_stroke_outline(
                                        rasterizer,
                                        target,
                                        x.into_f32(),
                                        y.into_f32(),
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

        self.unchanged_range = unchanged_range;

        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {:.2}ms to render", time);

        Ok(())
    }
}
