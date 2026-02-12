use std::{collections::HashMap, ops::Range};

use icu_locale::{LanguageIdentifier, LocaleDirectionality};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6, Vec2},
    rc::Rc,
};

use crate::{
    layout::{
        self,
        block::{BlockContainer, BlockContainerContent},
        inline::{InlineContent, InlineContentBuilder, InlineSpanBuilder},
        FixedL, InlineLayoutError, LayoutConstraints, Point2L, Vec2L,
    },
    log::{log_once_state, warning, LogOnceSet},
    renderer::FrameLayoutPass,
    srv3::{Event, ModeHint, RubyPosition},
    style::{
        computed::{
            Alignment, Direction, FontSlant, HorizontalAlignment, InlineSizing, Length, TextShadow,
            VerticalAlignment, Visibility,
        },
        ComputedStyle,
    },
    text::OpenTypeTag,
    Subrandr, SubtitleContext,
};

use super::{Document, EdgeType, Pen, RubyPart};

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
    x: f32,
    y: f32,
    // TODO: What the heck does this do
    //       How does a timestamp on a window work?
    //       Currently this is just ignored until I figure out what to do with it.
    range: Range<u32>,
    segment_style: ComputedStyle,
    vertical_align: VerticalAlignment,
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
    base_style: ComputedStyle,
    time_offset: u32,
    text: std::rc::Rc<str>,
}

#[derive(Debug, Clone)]
struct LineSegment {
    inner: Segment,
    annotation: Option<Segment>,
}

impl Segment {
    fn compute_shadows(&self, ctx: &SubtitleContext, out: &mut Vec<TextShadow>) {
        let scale = FixedL::from_f32(font_scale_from_ctx(ctx) / 32.0);
        let l1 = Length::from_pixels((scale).max(FixedL::ONE));
        let l2 = Length::from_pixels((scale * 2).max(FixedL::ONE));
        let l3 = Length::from_pixels((scale * 3).max(FixedL::ONE));
        let l5 = Length::from_pixels((scale * 5).max(FixedL::ONE));
        let primary_color = BGRA8::from_argb32(self.pen.edge_color.map_or_else(
            || 0x222222 | (self.pen.foreground_color << 24),
            |c| c | 0xFF000000,
        ));

        match self.pen.edge_type {
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
                let secondary_color = if self.pen.edge_color.is_none() {
                    BGRA8::from_argb32(0xCCCCCC | (self.pen.foreground_color << 24))
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
    ) -> Option<ComputedStyle> {
        let mut result = segment.base_style.clone();
        pass.add_animation_point(self.range.start + segment.time_offset);
        if segment.time_offset > pass.t - self.range.start {
            match mh {
                ModeHint::Default => {
                    *result.make_visibility_mut() = Visibility::Hidden;
                }
                ModeHint::Scroll => return None,
            }
        }

        if pass.srv3_use_inlines {
            *result.make_inline_sizing_mut() = InlineSizing::Stretch;
        }

        *result.make_font_size_mut() = I26Dot6::from(
            font_size_to_pixels(segment.pen.font_size) * font_scale_from_ctx(pass.sctx),
        );

        let mut shadows = vec![];
        segment.compute_shadows(pass.sctx, &mut shadows);

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
        let mut builder = InlineContentBuilder::new(root_inline_style);
        let mut root = builder.root();
        let mut it = self.segments.iter();
        let mut take_next = move |pass: &mut FrameLayoutPass| loop {
            let segment = it.next()?;
            if let Some(style) = self.compute_segment_style(pass, &segment.inner, mh) {
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
                            .compute_segment_style(pass, annotation_segment, mh)
                            .map(|mut style| {
                                // If inline-block layout is enabled then background is handled by the
                                // containing block.
                                if !pass.srv3_use_inlines {
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

            if pass.srv3_use_inlines {
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
        let inner_style = {
            let mut result = self.segment_style.clone();
            *result.make_background_color_mut() = BGRA8::ZERO;
            result
        };
        let mut lines = Vec::new();
        for line in &self.lines {
            if pass.add_event_range(line.range.clone()) {
                lines.push(BlockContainer {
                    style: inner_style.clone(),
                    content: BlockContainerContent::Inline(line.to_inline_content(
                        pass,
                        self.segment_style.clone(),
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

        let mut pos = Point2L::new(
            (self.x * pass.sctx.player_width().into_f32()).into(),
            (self.y * pass.sctx.player_height().into_f32()).into(),
        );

        let fragment_size = fragment.fbox.size_for_layout();
        match self.segment_style.text_align() {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => pos.x -= fragment_size.x / 2,
            HorizontalAlignment::Right => pos.x -= fragment_size.x,
        }

        match self.vertical_align {
            VerticalAlignment::Top => (),
            VerticalAlignment::Center => pos.y -= fragment_size.y / 2,
            VerticalAlignment::Bottom => pos.y -= fragment_size.y,
        }

        Ok(Some((pos, fragment)))
    }
}

fn pen_to_size_independent_style(
    pen: &Pen,
    set_default: bool,
    mut base: ComputedStyle,
) -> ComputedStyle {
    if set_default || pen.font_style != Pen::DEFAULT.font_style {
        *base.make_font_family_mut() = font_style_to_families(pen.font_style).clone();
    }

    if pen.bold {
        *base.make_font_weight_mut() = I16Dot16::new(700);
    }

    if pen.italic {
        *base.make_font_slant_mut() = FontSlant::Italic;
    }

    let bgra_foreground_color = BGRA8::from_rgba32(pen.foreground_color);
    if pen.underline {
        let decorations = base.make_text_decoration_mut();
        decorations.underline = true;
        decorations.underline_color = bgra_foreground_color;
    }

    if set_default || pen.foreground_color != Pen::DEFAULT.foreground_color {
        *base.make_color_mut() = bgra_foreground_color;
    }

    if set_default || pen.background_color != Pen::DEFAULT.background_color {
        *base.make_background_color_mut() = BGRA8::from_rgba32(pen.background_color);
    }

    base
}

fn convert_segment(segment: &super::Segment, text: &str, base_style: &ComputedStyle) -> Segment {
    Segment {
        pen: *segment.pen(),
        base_style: pen_to_size_independent_style(segment.pen(), false, base_style.clone()),
        time_offset: segment.time_offset,
        text: text.into(),
    }
}

struct WindowBuilder<'a> {
    sbr: &'a Subrandr,
    base_style: ComputedStyle,
    logset: LogOnceSet,
}

impl WindowBuilder<'_> {
    fn create_window(
        &self,
        x: f32,
        y: f32,
        time: u32,
        duration: u32,
        align: Alignment,
        mode_hint: ModeHint,
    ) -> Window {
        Window {
            x,
            y,
            range: time..time + duration,
            segment_style: {
                let mut style = self.base_style.clone();
                *style.make_text_align_mut() = align.0;
                style
            },
            vertical_align: align.1,
            lines: Vec::new(),
            mode_hint,
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
                if segment.pen().ruby_part == RubyPart::Base && it.as_slice().len() > 3 {
                    let ruby_block = <&[_; 3]>::try_from(&it.as_slice()[..3]).unwrap();

                    let [RubyPart::Parenthesis, RubyPart::Ruby(part), RubyPart::Parenthesis] =
                        ruby_block.each_ref().map(|s| s.pen().ruby_part)
                    else {
                        break 'ruby_failed;
                    };

                    match part.position {
                        RubyPosition::Alternate | RubyPosition::Over => (),
                        RubyPosition::Under => {
                            warning!(
                                self.sbr,
                                once(ruby_under_unsupported),
                                "`ruby-position: under`-style ruby text is not supported yet"
                            );
                            break 'ruby_failed;
                        }
                    };

                    _ = it.next().unwrap();
                    let next = it.next().unwrap();
                    current_line.segments.push(LineSegment {
                        inner: convert_segment(segment, &segment.text, &window.segment_style),
                        annotation: Some(convert_segment(next, &next.text, &window.segment_style)),
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
                    inner: convert_segment(
                        segment,
                        &segment.text[last..end],
                        &window.segment_style,
                    ),
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

pub fn convert(sbr: &Subrandr, document: Document, lang: Option<&LanguageIdentifier>) -> Subtitles {
    let mut result = Subtitles {
        windows: Vec::new(),
    };
    let mut window_builder = WindowBuilder {
        sbr,
        base_style: {
            let mut style =
                pen_to_size_independent_style(&Pen::DEFAULT, true, ComputedStyle::DEFAULT);
            if let Some(lang) = lang {
                if LocaleDirectionality::new_common().is_right_to_left(lang) {
                    *style.make_direction_mut() = Direction::Rtl;
                }
            }
            style
        },
        logset: LogOnceSet::new(),
    };

    let mut wname_to_index = HashMap::new();
    for (name, window) in document.windows() {
        wname_to_index.insert(&**name, result.windows.len());
        result.windows.push(window_builder.create_window(
            convert_coordinate(window.position().x as f32),
            convert_coordinate(window.position().y as f32),
            window.time,
            window.duration,
            window.position().point.to_alignment(),
            window.style().mode_hint,
        ));
    }

    for event in document.events() {
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
            window_builder.extend_lines(&mut result.windows[widx], event);
        } else {
            let mut window = window_builder.create_window(
                convert_coordinate(event.position().x as f32),
                convert_coordinate(event.position().y as f32),
                event.time,
                event.duration,
                event.position().point.to_alignment(),
                event.style().mode_hint,
            );
            window_builder.extend_lines(&mut window, event);
            result.windows.push(window);
        }
    }

    result
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
