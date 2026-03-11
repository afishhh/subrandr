use std::{collections::HashMap, ops::Range};

use icu_locale::{LanguageIdentifier, LocaleDirectionality};
use log::{log_once_state, warn, LogContext, LogOnceSet};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6, Vec2},
    rc::Rc,
};

use crate::{
    config::Config,
    layout::{
        self,
        block::{BlockContainer, BlockContainerContent},
        inline::{InlineContent, InlineContentBuilder, InlineSpanBuilder},
        FixedL, InlineLayoutError, LayoutConstraints, Point2L, Vec2L,
    },
    renderer::FrameLayoutPass,
    srv3::{
        BodyParser, EdgeType, Event, ModeHint, Pen, Point, RubyPart, RubyPosition, WindowPos,
        WindowStyle,
    },
    style::{
        computed::{
            Alignment, Direction, FontSlant, HorizontalAlignment, InlineSizing, Length, TextShadow,
            VerticalAlignment, Visibility,
        },
        ComputedStyle,
    },
    text::OpenTypeTag,
    SubtitleContext,
};

macro_rules! static_rc_of_static_strings {
    [$($values: literal),* $(,)?] => {
        util::rc_static!([
            $(util::rc_static!(str $values)),*
        ])
    };
}

const SRV3_FONTS: &[Rc<[Rc<str>]>] = &[
    static_rc_of_static_strings![
        b"Courier New",
        b"Courier",
        b"Nimbus Mono L",
        b"Cutive Mono",
        b"monospace",
    ],
    static_rc_of_static_strings![
        b"Times New Roman",
        b"Times",
        b"Georgia",
        b"Cambria",
        b"PT Serif Caption",
        b"serif",
    ],
    static_rc_of_static_strings![
        b"Deja Vu Sans Mono", // not a real font :(
        b"Lucida Console",
        b"Monaco",
        b"Consolas",
        b"PT Mono",
        b"monospace",
    ],
    static_rc_of_static_strings![
        b"YouTube Noto",
        b"Roboto",
        b"Arial",
        b"Helvetica",
        b"Verdana",
        b"PT Sans Caption",
        b"sans-serif",
    ],
    static_rc_of_static_strings![b"Comic Sans Ms", b"Impact", b"Handlee", b"fantasy"],
    static_rc_of_static_strings![
        b"Monotype Corsiva",
        b"URW Chancery L",
        b"Apple Chancery",
        b"Dancing Script",
        b"cursive",
    ],
    // YouTube appears to conditionally set this to either:
    // "Carrois Gothic SC", sans-serif-smallcaps
    // or sometimes:
    // Arial, Helvetica, Verdana, "Marcellus SC", sans-serif
    // the first one seems to be used when ran under Cobalt
    // https://developers.google.com/youtube/cobalt
    // i.e. in YouTube TV
    static_rc_of_static_strings![
        b"Arial",
        b"Helvetica",
        b"Verdana",
        b"Marcellus SC",
        b"sans-serif",
    ],
];

fn font_style_to_families(style: u32) -> &'static Rc<[Rc<str>]> {
    style
        .checked_sub(1)
        .and_then(|i| SRV3_FONTS.get(i as usize))
        .map_or(&SRV3_FONTS[3], |v| v)
}

fn convert_coordinate(coord: f32) -> f32 {
    0.02 + coord * 0.0096
}

fn calculate_font_scale(
    mut video_width: f32,
    video_height: f32,
    player_width: f32,
    player_height: f32,
) -> f32 {
    let mut h = video_height / 360.0 * 16.0;
    if video_height >= video_width {
        video_width = 640.0;
        if player_height > player_width * 1.3 {
            video_width = 480.0;
        }
        h = player_width / video_width * 16.0;
    }
    h
}

fn font_scale_from_ctx(ctx: &SubtitleContext) -> f32 {
    calculate_font_scale(
        Length::from_physical_pixels(ctx.video_width, ctx.dpi).to_f32(),
        Length::from_physical_pixels(ctx.video_height, ctx.dpi).to_f32(),
        Length::from_physical_pixels(ctx.player_width(), ctx.dpi).to_f32(),
        Length::from_physical_pixels(ctx.player_height(), ctx.dpi).to_f32(),
    )
}

#[allow(clippy::let_and_return)] // shut up
fn font_size_to_pixels(size: u16) -> f32 {
    let c = 1.0 + 0.25 * (size as f32 / 100.0 - 1.0);
    // NOTE: This appears to be further modified based on an "of" attribute which we
    //       currently don't support.
    //       If we start doing so the correct transformation seems to be
    //       `if of == 0 || of == 2 { c *= 0.8 }`.
    c
}

impl super::Point {
    pub fn to_alignment(self) -> Alignment {
        match self {
            super::Point::TopLeft => Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
            super::Point::TopCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Top)
            }
            super::Point::TopRight => Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
            super::Point::MiddleLeft => {
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Center)
            }
            super::Point::MiddleCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Center)
            }
            super::Point::MiddleRight => {
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Center)
            }
            super::Point::BottomLeft => {
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Bottom)
            }
            super::Point::BottomCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Bottom)
            }
            super::Point::BottomRight => {
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Bottom)
            }
        }
    }
}

#[derive(Debug)]
pub struct Subtitles {
    windows: Vec<Window>,
}

#[derive(Debug)]
struct Window {
    pos: WindowPos,
    text_direction: Direction,
    // TODO: What the heck does this do
    //       How does a timestamp on a window work?
    //       Currently this is just ignored until I figure out what to do with it.
    range: Range<u32>,
    lines: Vec<VisualLine>,
    mode_hint: ModeHint,
}

#[derive(Debug)]
struct VisualLine {
    range: Range<u32>,
    segments: Vec<LineSegment>,
}

#[derive(Debug, Clone)]
struct Segment {
    pen: Pen,
    time_offset: u32,
    text: std::rc::Rc<str>,
}

#[derive(Debug, Clone)]
struct LineSegment {
    inner: Segment,
    annotation: Option<Segment>,
}

#[derive(Debug, Clone, Copy)]
enum LayoutMode {
    InlineBlock,
    // TODO: fully inline mode could probably keep using blocks for ruby which would clean
    //       up a sizing hack in inline layout
    Inline,
}

impl crate::config::OptionFromStr for LayoutMode {
    fn from_str(s: &str) -> Result<Self, util::AnyError> {
        Ok(match s {
            "inline-block" => Self::InlineBlock,
            "inline" => Self::Inline,
            _ => return Err("must be either \"inline-block\" or \"inline\"".into()),
        })
    }
}

fn parse_font_style(s: &str) -> Result<u32, util::AnyError> {
    let value = s.parse::<u32>()?;
    if value == 0 || value as usize > SRV3_FONTS.len() {
        return Err(format!("must be an integer in the range [1, {}]", SRV3_FONTS.len()).into());
    }
    Ok(value)
}

fn parse_edge_type(s: &str) -> Result<EdgeType, util::AnyError> {
    Ok(match s {
        "none" => EdgeType::None,
        "hard-shadow" => EdgeType::HardShadow,
        "bevel" => EdgeType::Bevel,
        "glow" => EdgeType::Glow,
        "soft-shadow" => EdgeType::SoftShadow,
        _ => return Err("not a valid edge type".into()),
    })
}

fn parse_edge_color(s: &str) -> Result<Option<u32>, util::AnyError> {
    const ERROR: &str = "must be a color in #RRGGBB form or \"none\"";

    if s == "none" {
        return Ok(None);
    }

    let hex = s.strip_prefix("#").ok_or(ERROR)?;
    if hex.len() != 6 {
        return Err(ERROR.into());
    }

    Ok(Some(u32::from_str_radix(hex, 16)?))
}

fn parse_point(s: &str) -> Result<Point, util::AnyError> {
    s.parse()
}

fn parse_win_coordinate(s: &str) -> Result<u32, util::AnyError> {
    s.parse()
        .ok()
        .filter(|&x| x <= 100)
        .ok_or_else(|| "must be an integer in the range [0, 100]".into())
}

// `pen` attribute defaults
pub const DEFAULT_PEN_FONT_SIZE: u16 = 100;
pub const DEFAULT_PEN_FONT_STYLE: u32 = 0;
pub const DEFAULT_PEN_BOLD: bool = false;
pub const DEFAULT_PEN_ITALIC: bool = false;
pub const DEFAULT_PEN_UNDERLINE: bool = false;
pub const DEFAULT_PEN_EDGE_TYPE: EdgeType = EdgeType::None;
pub const DEFAULT_PEN_RUBY_PART: RubyPart = RubyPart::None;
pub const DEFAULT_PEN_FOREGROUND_COLOR: BGRA8 = BGRA8::from_rgba32(0xFFFFFFFF);
// The default opacity is 0.75, round(0.75 * 255) = 0xBF
pub const DEFAULT_PEN_BACKGROUND_COLOR: BGRA8 = BGRA8::from_rgba32(0x080808BF);

// `wp` attribute defaults
pub const DEFAULT_WIN_POINT: Point = Point::BottomCenter;
pub const DEFAULT_WIN_X: u32 = 50;
pub const DEFAULT_WIN_Y: u32 = 100;
// `ws` attribute defaults
pub const DEFAULT_WIN_MODE_HINT: ModeHint = ModeHint::Default;

crate::config::define_option_group! {
    pub(crate) struct Options {
        #[option(name = "layout-mode")]
        layout_mode: LayoutMode = LayoutMode::InlineBlock,
        #[option(name = "default-font-size")]
        default_font_size: u16 = DEFAULT_PEN_FONT_SIZE,
        #[option(name = "default-font-style", parse_with = parse_font_style)]
        default_font_style: u32 = DEFAULT_PEN_FONT_STYLE,
        #[option(name = "default-fg-color")]
        default_foreground_color: BGRA8 = DEFAULT_PEN_FOREGROUND_COLOR,
        #[option(name = "default-bg-color")]
        default_background_color: BGRA8 = DEFAULT_PEN_BACKGROUND_COLOR,
        #[option(name = "default-edge-type", parse_with = parse_edge_type)]
        default_edge_type: EdgeType = EdgeType::None,
        #[option(name = "default-edge-color", parse_with = parse_edge_color)]
        default_edge_color: Option<u32> = None,
        #[option(name = "default-win-align", parse_with = parse_point)]
        default_win_align: Point = DEFAULT_WIN_POINT,
        #[option(name = "default-win-x", parse_with = parse_win_coordinate)]
        default_win_x: u32 = DEFAULT_WIN_X,
        #[option(name = "default-win-y", parse_with = parse_win_coordinate)]
        default_win_y: u32 = DEFAULT_WIN_Y,
    }
}

#[derive(Debug, Clone)]
struct ComputedPen {
    font_size: u16,
    font_style: u32,

    bold: bool,
    italic: bool,
    underline: bool,

    edge_type: EdgeType,
    edge_color: Option<u32>,

    foreground_color: rasterize::color::BGRA8,
    background_color: rasterize::color::BGRA8,
}

impl Segment {
    fn compute_pen(&self, pen: &Pen, cfg: &Config) -> ComputedPen {
        fn compute_color(default: u32, color: Option<u32>, opacity: Option<u8>) -> BGRA8 {
            BGRA8::from_argb32(
                color.unwrap_or(default >> 8) | (u32::from(opacity.unwrap_or(default as u8)) << 24),
            )
        }
        ComputedPen {
            font_size: pen.font_size().unwrap_or(cfg.srv3.default_font_size),
            font_style: pen.font_style().unwrap_or(cfg.srv3.default_font_style),
            bold: pen.bold().unwrap_or(DEFAULT_PEN_BOLD),
            italic: pen.italic().unwrap_or(DEFAULT_PEN_ITALIC),
            underline: pen.underline().unwrap_or(DEFAULT_PEN_UNDERLINE),
            edge_type: pen.edge_type().unwrap_or(cfg.srv3.default_edge_type),
            edge_color: pen.edge_color().or(cfg.srv3.default_edge_color),
            foreground_color: compute_color(
                cfg.srv3.default_foreground_color.to_rgba32(),
                pen.foreground_color(),
                pen.foreground_opacity(),
            ),
            background_color: compute_color(
                cfg.srv3.default_background_color.to_rgba32(),
                pen.background_color(),
                pen.background_opacity(),
            ),
        }
    }

    fn compute_shadows(
        &self,
        ctx: &SubtitleContext,
        style: &ComputedPen,
        out: &mut Vec<TextShadow>,
    ) {
        let scale = FixedL::from_f32(font_scale_from_ctx(ctx) / 32.0);
        let l1 = Length::from_pixels((scale).max(FixedL::ONE));
        let l2 = Length::from_pixels((scale * 2).max(FixedL::ONE));
        let l3 = Length::from_pixels((scale * 3).max(FixedL::ONE));
        let l5 = Length::from_pixels((scale * 5).max(FixedL::ONE));
        let primary_color = BGRA8::from_argb32(style.edge_color.map_or_else(
            || 0x222222 | (u32::from(style.foreground_color.a) << 24),
            |c| c | 0xFF000000,
        ));

        match style.edge_type {
            EdgeType::None => (),
            EdgeType::HardShadow => {
                let step = Length::HALF * (ctx.dpi < 144) as i32 + Length::HALF;
                let mut x = l1;
                while x <= l3 {
                    out.push(TextShadow {
                        offset: Vec2::new(x, x),
                        blur_radius: Length::ZERO,
                        color: primary_color,
                    });
                    x += step;
                }
            }
            EdgeType::Bevel => {
                // If there is no explicit edge color set then `Bevel` will use two
                // distinct colors for the positive-offset and negative-offset shadow.
                // The "inner" shadow will end up with a very light gray but the "outer" one
                // will be the usual black.
                let secondary_color = if style.edge_color.is_none() {
                    BGRA8::from_argb32(0xCCCCCC | (u32::from(style.foreground_color.a) << 24))
                } else {
                    primary_color
                };
                let offset = Vec2::new(l1, l1);
                out.push(TextShadow {
                    offset,
                    blur_radius: Length::ZERO,
                    color: secondary_color,
                });
                out.push(TextShadow {
                    offset: -offset,
                    blur_radius: Length::ZERO,
                    color: primary_color,
                });
            }
            EdgeType::Glow => out.extend(std::iter::repeat_n(
                TextShadow {
                    offset: Vec2::ZERO,
                    blur_radius: l2,
                    color: primary_color,
                },
                5,
            )),
            EdgeType::SoftShadow => {
                let offset = Vec2::new(l2, l2);
                let mut x = l3;
                while x <= l5 {
                    out.push(TextShadow {
                        offset,
                        blur_radius: x,
                        color: primary_color,
                    });
                    x += Length::from_pixels(scale);
                }
            }
        }
    }
}

impl VisualLine {
    fn compute_segment_style(
        &self,
        pass: &mut FrameLayoutPass,
        segment: &Segment,
        mh: ModeHint,
        base: &ComputedStyle,
    ) -> Option<ComputedStyle> {
        let mut result = base.clone();
        pass.add_animation_point(self.range.start + segment.time_offset);
        if segment.time_offset > pass.t - self.range.start {
            match mh {
                ModeHint::Default => {
                    *result.make_visibility_mut() = Visibility::Hidden;
                }
                ModeHint::Scroll => return None,
            }
        }

        if matches!(pass.cfg.srv3.layout_mode, LayoutMode::Inline) {
            *result.make_inline_sizing_mut() = InlineSizing::Stretch;
        }

        let pen = segment.compute_pen(&segment.pen, pass.cfg);
        *result.make_font_size_mut() =
            I26Dot6::from(font_size_to_pixels(pen.font_size) * font_scale_from_ctx(pass.sctx));
        *result.make_font_family_mut() = font_style_to_families(pen.font_style).clone();

        if pen.bold {
            *result.make_font_weight_mut() = I16Dot16::new(700);
        }

        if pen.italic {
            *result.make_font_slant_mut() = FontSlant::Italic;
        }

        if pen.underline {
            let decorations = result.make_text_decoration_mut();
            decorations.underline = true;
            decorations.underline_color = pen.foreground_color;
        }

        *result.make_color_mut() = pen.foreground_color;
        *result.make_background_color_mut() = pen.background_color;

        let mut shadows = vec![];
        segment.compute_shadows(pass.sctx, &pen, &mut shadows);

        if !shadows.is_empty() {
            *result.make_text_shadows_mut() = shadows.into();
        }

        Some(result)
    }

    fn to_inline_content(
        &self,
        pass: &mut FrameLayoutPass,
        root_inline_style: ComputedStyle,
        mh: ModeHint,
    ) -> InlineContent {
        let mut builder = InlineContentBuilder::new(root_inline_style.clone());
        let mut root = builder.root();
        let mut it = self.segments.iter();
        let mut take_next = |pass: &mut FrameLayoutPass| loop {
            let segment = it.next()?;
            if let Some(style) =
                self.compute_segment_style(pass, &segment.inner, mh, &root_inline_style)
            {
                break Some((segment, style));
            }
        };

        struct CurrentBlock {
            pen: Option<Pen>,
            style: ComputedStyle,
            builder: InlineContentBuilder,
        }

        impl CurrentBlock {
            fn flush(&mut self, to: &mut InlineSpanBuilder<'_>) {
                to.push_inline_block(BlockContainer {
                    style: std::mem::replace(&mut self.style, ComputedStyle::DEFAULT),
                    content: BlockContainerContent::Inline(self.builder.finish()),
                });
            }
        }

        // Only used if inline-block layout is enabled to coalesce structurally-same-pen
        // segments into a single inline-block.
        // This is what YouTube appears to do from my testing.
        let mut current_block = CurrentBlock {
            pen: None,
            style: ComputedStyle::DEFAULT,
            builder: InlineContentBuilder::new(ComputedStyle::DEFAULT),
        };
        let mut first = true;
        let mut next = take_next(pass);
        while let Some((segment, mut style)) = next {
            next = take_next(pass);

            if first {
                *style.make_padding_left_mut() = Length::from_points(style.font_size() / 4);
                first = false;
            }
            let right_padding = if next.is_none() {
                Some(Length::from_points(style.font_size() / 4))
            } else {
                None
            };

            let layout_to_builder =
                |pass: &mut FrameLayoutPass,
                 inner: &mut InlineSpanBuilder<'_>,
                 inner_style: ComputedStyle| match &segment.annotation {
                    None => {
                        inner.push_span(inner_style).push_text(&segment.inner.text);
                    }
                    Some(annotation_segment) => {
                        let mut ruby = inner.push_ruby(inner_style.create_derived());
                        ruby.push_base(inner_style.clone())
                            .push_text(&segment.inner.text);

                        if let Some(annotation_style) = self
                            .compute_segment_style(pass, annotation_segment, mh, &root_inline_style)
                            .map(|mut style| {
                                // If inline-block layout is enabled then background is handled by the
                                // containing block.
                                if matches!(pass.cfg.srv3.layout_mode, LayoutMode::InlineBlock) {
                                    *style.make_background_color_mut() = BGRA8::ZERO;
                                }
                                *style.make_font_size_mut() /= 2.0;
                                style
                                    .make_font_feature_settings_mut()
                                    .set(OpenTypeTag::FEAT_RUBY, 1);
                                style
                            })
                        {
                            ruby.push_annotation(annotation_style)
                                .push_text(&annotation_segment.text);
                        }
                    }
                };

            if matches!(pass.cfg.srv3.layout_mode, LayoutMode::Inline) {
                if let Some(right_padding) = right_padding {
                    *style.make_padding_right_mut() = right_padding;
                }

                layout_to_builder(pass, &mut root, style);
            } else {
                let inner_style = style.create_derived();
                if current_block
                    .pen
                    .as_ref()
                    .is_none_or(|pen| pen != &segment.inner.pen)
                    || current_block.style.visibility() != style.visibility()
                {
                    if current_block.pen.is_some() {
                        current_block.flush(&mut root);
                    }
                    current_block.pen = Some(segment.inner.pen);
                    current_block.builder.set_root_style(style.create_derived());
                    current_block.style = style;
                };

                if let Some(right_padding) = right_padding {
                    *current_block.style.make_padding_right_mut() = right_padding;
                }

                layout_to_builder(pass, &mut current_block.builder.root(), inner_style);

                if right_padding.is_some() {
                    current_block.flush(&mut root);
                }
            };
        }

        if current_block.pen.is_some() {
            current_block.flush(&mut root);
        }

        drop(root);
        builder.finish()
    }
}

impl Window {
    pub fn layout(
        &self,
        pass: &mut FrameLayoutPass,
    ) -> Result<Option<(Point2L, layout::block::BlockContainerFragment)>, layout::InlineLayoutError>
    {
        let Alignment(text_align, vertical_align) = self
            .pos
            .point()
            .unwrap_or(pass.cfg.srv3.default_win_align)
            .to_alignment();
        let inner_style = {
            let mut result = ComputedStyle::DEFAULT;
            *result.make_direction_mut() = self.text_direction;
            *result.make_text_align_mut() = text_align;
            result
        };

        let mut lines = Vec::new();
        for line in &self.lines {
            if pass.add_event_range(line.range.clone()) {
                lines.push(BlockContainer {
                    style: inner_style.clone(),
                    content: BlockContainerContent::Inline(line.to_inline_content(
                        pass,
                        inner_style.clone(),
                        self.mode_hint,
                    )),
                });
            }
        }

        if lines.is_empty() {
            return Ok(None);
        }

        let window = BlockContainer {
            style: inner_style,
            content: BlockContainerContent::Block(lines),
        };
        let partial_window = layout::block::layout_initial(pass.lctx, &window)?;

        let width = partial_window
            .intrinsic_width()
            .min(pass.sctx.player_width() * 96 / 100);
        let constraints = LayoutConstraints {
            size: Vec2L::new(width, FixedL::MAX),
        };

        let fragment = partial_window.layout(pass.lctx, &constraints)?;

        let x_percentage =
            convert_coordinate(self.pos.x().unwrap_or(pass.cfg.srv3.default_win_x) as f32);
        let y_percentage =
            convert_coordinate(self.pos.y().unwrap_or(pass.cfg.srv3.default_win_y) as f32);
        let mut pos = Point2L::new(
            (x_percentage * pass.sctx.player_width().into_f32()).into(),
            (y_percentage * pass.sctx.player_height().into_f32()).into(),
        );

        let fragment_size = fragment.fbox.size_for_layout();
        match text_align {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => pos.x -= fragment_size.x / 2,
            HorizontalAlignment::Right => pos.x -= fragment_size.x,
        }

        match vertical_align {
            VerticalAlignment::Top => (),
            VerticalAlignment::Center => pos.y -= fragment_size.y / 2,
            VerticalAlignment::Bottom => pos.y -= fragment_size.y,
        }

        Ok(Some((pos, fragment)))
    }
}

fn convert_segment(segment: &super::Segment, text: &str) -> Segment {
    Segment {
        pen: *segment.pen,
        time_offset: segment.time_offset,
        text: text.into(),
    }
}

struct WindowBuilder<'a> {
    log: &'a LogContext<'a>,
    text_direction: Direction,
    logset: LogOnceSet,
}

impl WindowBuilder<'_> {
    fn create_window(
        &self,
        pos: WindowPos,
        style: WindowStyle,
        time: u32,
        duration: u32,
    ) -> Window {
        Window {
            pos,
            text_direction: self.text_direction,
            range: time..time + duration,
            lines: Vec::new(),
            mode_hint: style.mode_hint().unwrap_or(DEFAULT_WIN_MODE_HINT),
        }
    }

    fn extend_lines(&mut self, window: &mut Window, event: &Event) {
        let event_range = event.time..event.time + event.duration;
        let mut current_line = VisualLine {
            range: event_range.clone(),
            segments: Vec::new(),
        };

        log_once_state!(in &self.logset; ruby_under_unsupported);

        let mut it = event.segments.iter();
        'segment_loop: while let Some(segment) = it.next() {
            'ruby_failed: {
                if segment.pen.ruby_part() == Some(RubyPart::Base) && it.as_slice().len() > 3 {
                    let ruby_block = <&[_; 3]>::try_from(&it.as_slice()[..3]).unwrap();

                    let [Some(RubyPart::Parenthesis), Some(RubyPart::Ruby(part)), Some(RubyPart::Parenthesis)] =
                        ruby_block.each_ref().map(|s| s.pen.ruby_part())
                    else {
                        break 'ruby_failed;
                    };

                    match part.position {
                        RubyPosition::Alternate | RubyPosition::Over => (),
                        RubyPosition::Under => {
                            warn!(
                                self.log,
                                once(ruby_under_unsupported),
                                "`ruby-position: under`-style ruby text is not supported yet"
                            );
                            break 'ruby_failed;
                        }
                    };

                    _ = it.next().unwrap();
                    let next = it.next().unwrap();
                    current_line.segments.push(LineSegment {
                        inner: convert_segment(segment, &segment.text),
                        annotation: Some(convert_segment(next, &next.text)),
                    });
                    _ = it.next().unwrap();

                    continue 'segment_loop;
                }
            }

            let mut last = 0;
            loop {
                let end = segment.text[last..]
                    .find('\n')
                    .map_or(segment.text.len(), |i| last + i);

                current_line.segments.push(LineSegment {
                    inner: convert_segment(segment, &segment.text[last..end]),
                    annotation: None,
                });

                if end == segment.text.len() {
                    break;
                }

                window.lines.push(current_line);
                current_line = VisualLine {
                    range: event_range.clone(),
                    segments: Vec::new(),
                };
                last = end + 1;
            }
        }

        window.lines.push(current_line);
    }
}

pub fn convert(
    log: &LogContext,
    mut parser: BodyParser,
    lang: Option<&LanguageIdentifier>,
) -> Result<Subtitles, super::parse::Error> {
    let mut result = Subtitles {
        windows: Vec::new(),
    };
    let mut window_builder = WindowBuilder {
        log,
        text_direction: {
            if lang.is_some_and(|lang| LocaleDirectionality::new_common().is_right_to_left(lang)) {
                Direction::Rtl
            } else {
                Direction::Ltr
            }
        },
        logset: LogOnceSet::new(),
    };

    let mut wname_to_index = HashMap::new();
    while let Some(element) = parser.read_next(log)? {
        match element {
            crate::srv3::BodyElement::Window(id, window) => {
                wname_to_index.insert(id, result.windows.len());
                result.windows.push(window_builder.create_window(
                    *window.position,
                    *window.style,
                    window.time,
                    window.duration,
                ));
            }
            crate::srv3::BodyElement::Event(event) => {
                // YouTube's player seems to skip these segments (which appear in auto-generated subs sometimes).
                // Don't know what the exact rule is but this at least fixes auto-generated subs.
                if event.segments.iter().all(|segment| {
                    segment
                        .text
                        .bytes()
                        .all(|byte| matches!(byte, b'\r' | b'\n'))
                }) {
                    continue;
                }

                if let Some(&widx) = event
                    .window_id
                    .as_ref()
                    .and_then(|wname| wname_to_index.get(&**wname))
                {
                    window_builder.extend_lines(&mut result.windows[widx], &event);
                } else {
                    let mut window = window_builder.create_window(
                        *event.position,
                        *event.style,
                        event.time,
                        event.duration,
                    );
                    window_builder.extend_lines(&mut window, &event);
                    result.windows.push(window);
                }
            }
        }
    }

    Ok(result)
}

pub(crate) struct Layouter {
    subtitles: Rc<Subtitles>,
}

impl Layouter {
    pub fn new(subtitles: Rc<Subtitles>) -> Self {
        Self { subtitles }
    }

    pub fn subtitles(&self) -> &Rc<Subtitles> {
        &self.subtitles
    }

    pub fn layout(&mut self, pass: &mut FrameLayoutPass) -> Result<(), InlineLayoutError> {
        for window in &self.subtitles.windows {
            if !pass.add_event_range(window.range.clone()) {
                continue;
            }

            if let Some((pos, block)) = window.layout(pass)? {
                pass.emit_fragment(pos, block);
            }
        }
        Ok(())
    }
}
