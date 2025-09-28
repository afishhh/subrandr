//! Converts parsed SRV3 subtitles into Subtitles.

use std::{collections::HashMap, ops::Range};

use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6, Vec2f},
    rc::Rc,
};

use crate::{
    layout::{
        self, inline::InlineContentBuilder, FixedL, InlineLayoutError, LayoutConstraints, Point2L,
        Vec2L,
    },
    log::{log_once_state, warning, LogOnceSet},
    renderer::FrameLayoutPass,
    srv3::{Event, RubyPosition},
    style::{
        computed::{
            Alignment, FontSlant, HorizontalAlignment, Length, Ruby, TextShadow, VerticalAlignment,
        },
        ComputedStyle,
    },
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
        ctx.pixels_to_css(ctx.video_width.into_f32()),
        ctx.pixels_to_css(ctx.video_height.into_f32()),
        ctx.pixels_to_css(ctx.player_width().into_f32()),
        ctx.pixels_to_css(ctx.player_height().into_f32()),
    )
}

#[allow(clippy::let_and_return)] // shut up
fn font_size_to_pixels(size: u16) -> f32 {
    let c = 1.0 + 0.25 * (size as f32 / 100.0 - 1.0);
    // This appears to be further modified based on an "of" attribute
    // currently we don't even parse it but if start doing so this is the
    // correct transformation:
    // if offset == 0 || offset == 2 {
    //     c *= 0.8;
    // }
    c
}

trait SubtitleContextCssExt {
    // 1px = 1/96in
    fn pixels_to_css(&self, physical_pixels: f32) -> f32;
    fn pixels_from_css(&self, css_pixels: f32) -> f32;
}

impl SubtitleContextCssExt for SubtitleContext {
    fn pixels_to_css(&self, physical_pixels: f32) -> f32 {
        physical_pixels / self.pixel_scale()
    }

    fn pixels_from_css(&self, css_pixels: f32) -> f32 {
        css_pixels * self.pixel_scale()
    }
}

fn pixels_to_points(pixels: f32) -> f32 {
    pixels * 96.0 / 72.0
}

#[derive(Debug, Clone)]
pub struct Srv3TextShadow {
    // never None
    kind: EdgeType,
    color: BGRA8,
}

impl Srv3TextShadow {
    pub(crate) fn to_css(&self, ctx: &SubtitleContext, out: &mut Vec<TextShadow>) {
        let a = font_scale_from_ctx(ctx) / 32.0;
        let e = a.max(1.0);
        let l = (2.0 * a).max(1.0);
        let mut t = (3.0 * a).max(1.0);
        let c = (5.0 * a).max(1.0);

        match self.kind {
            EdgeType::None => (),
            EdgeType::HardShadow => {
                // in captions.js it is window.devicePixelRatio >= 2 ? 0.5 : 1
                // BUT that is NOT what we want, I think they do this to increase fidelity on displays
                // with a lower DPI, because browsers scale all their units by window.devicePixelRatio
                // however we're working with direct device pixels here, so we want to do the OPPOSITE
                // of what they do and pick 0.5 when we have less pixels.
                let step = (ctx.dpi >= 144) as i32 as f32 * 0.5 + 0.5;
                let mut x = e;
                while x <= t {
                    out.push(TextShadow {
                        offset: Vec2f::new(ctx.pixels_from_css(x), ctx.pixels_from_css(x)),
                        blur_radius: I26Dot6::ZERO,
                        color: self.color,
                    });
                    x += step;
                }
            }
            EdgeType::Bevel => {
                let offset = Vec2f::new(ctx.pixels_from_css(e), ctx.pixels_from_css(e));
                out.push(TextShadow {
                    offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
                out.push(TextShadow {
                    offset: -offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
            }
            EdgeType::Glow => out.extend(std::iter::repeat_n(
                TextShadow {
                    offset: Vec2f::ZERO,
                    blur_radius: I26Dot6::from_f32(ctx.pixels_from_css(l)),
                    color: self.color,
                },
                5,
            )),
            EdgeType::SoftShadow => {
                let offset = Vec2f::new(ctx.pixels_from_css(l), ctx.pixels_from_css(l));
                while t <= c {
                    out.push(TextShadow {
                        offset,
                        blur_radius: I26Dot6::from_f32(ctx.pixels_from_css(t)),
                        color: self.color,
                    });
                    t += a;
                }
            }
        }
    }
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
    alignment: Alignment,
    lines: Vec<VisualLine>,
}

#[derive(Debug)]
struct VisualLine {
    range: Range<u32>,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    base_style: ComputedStyle,
    font_size: u16,
    time_offset: u32,
    text: std::rc::Rc<str>,
    shadow: Srv3TextShadow,
    ruby: Ruby,
}

impl Segment {
    fn compute_style(&self, sctx: &SubtitleContext) -> ComputedStyle {
        let mut result = self.base_style.clone();

        let mut size = pixels_to_points(font_size_to_pixels(self.font_size) * 0.75)
            * font_scale_from_ctx(sctx);
        if matches!(self.ruby, Ruby::Over) {
            size /= 2.0;
        }

        *result.make_font_size_mut() = I26Dot6::from(size);

        let mut shadows = vec![];
        self.shadow.to_css(sctx, &mut shadows);

        if !shadows.is_empty() {
            *result.make_text_shadows_mut() = shadows.into();
        }

        result
    }
}

fn segments_to_inline(
    pass: &mut FrameLayoutPass,
    content: &mut InlineContentBuilder,
    event_time: u32,
    segments: &[Segment],
) {
    // What lack of Peekable::inner() and Filter::inner() does to a language...
    let mut next_idx = 0;
    let sctx = pass.sctx;
    let mut root = content.root();
    let mut it = segments
        .iter()
        .filter(|segment| {
            pass.add_animation_point(event_time + segment.time_offset);
            segment.time_offset <= pass.t - event_time
        })
        .peekable();

    while let Some(segment) = it.next() {
        let mut style = segment.compute_style(sctx);

        if next_idx == 0 {
            *style.make_padding_left_mut() = Length::from_points(style.font_size() / 4);
        }
        next_idx += 1;
        // NOTE: This purposefully ignores whether or not the next segment is
        //       currently visible as that is what YouTube seems to do.
        if segments.get(next_idx).is_none() {
            *style.make_padding_right_mut() = Length::from_points(style.font_size() / 4);
        }

        match segment.ruby {
            Ruby::None => {
                root.push_span(style).push_text(&segment.text);
            }
            Ruby::Base => {
                let mut ruby = root.push_ruby(style.create_derived());
                ruby.push_base(style.clone()).push_text(&segment.text);
                if let Some(next) = it.peek() {
                    if let Ruby::Over = next.ruby {
                        ruby.push_annotation(next.compute_style(sctx))
                            .push_text(&next.text);
                        _ = it.next();
                    }
                }
            }
            Ruby::Over => {
                root.push_ruby(style.clone())
                    .push_annotation(style)
                    .push_text(&segment.text);
            }
        }
    }
}

impl Window {
    pub fn layout(
        &self,
        pass: &mut FrameLayoutPass,
    ) -> Result<Option<(Point2L, layout::inline::InlineContentFragment)>, layout::InlineLayoutError>
    {
        let mut content = InlineContentBuilder::new();
        for line in &self.lines {
            if pass.add_event_range(line.range.clone()) {
                if !content.is_empty() {
                    content.root().push_text("\n");
                }

                segments_to_inline(pass, &mut content, line.range.start, &line.segments)
            }
        }

        if content.is_empty() {
            return Ok(None);
        }

        let constraints = LayoutConstraints {
            size: Vec2L::new(pass.sctx.player_width() * 96 / 100, FixedL::MAX),
        };

        let fragment =
            layout::inline::layout(pass.lctx, &constraints, &content.finish(), self.alignment.0)?;

        let mut pos = Point2L::new(
            (self.x * pass.sctx.player_width().into_f32()).into(),
            (self.y * pass.sctx.player_height().into_f32()).into(),
        );

        let fragment_size = fragment.fbox.size_for_layout();
        match self.alignment.0 {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => pos.x -= fragment_size.x / 2,
            HorizontalAlignment::Right => pos.x -= fragment_size.x,
        }

        match self.alignment.1 {
            VerticalAlignment::Top => (),
            VerticalAlignment::Center => pos.y -= fragment_size.y / 2,
            VerticalAlignment::Bottom => pos.y -= fragment_size.y,
        }

        Ok(Some((pos, fragment)))
    }
}

fn pen_to_size_independent_styles(
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

    if set_default || pen.foreground_color != Pen::DEFAULT.foreground_color {
        *base.make_color_mut() = BGRA8::from_rgba32(pen.foreground_color);
    }

    if set_default || pen.background_color != Pen::DEFAULT.background_color {
        *base.make_background_color_mut() = BGRA8::from_rgba32(pen.background_color);
    }

    base
}

fn convert_segment(
    segment: &super::Segment,
    text: &str,
    ruby: Ruby,
    base_style: &ComputedStyle,
) -> Segment {
    Segment {
        base_style: pen_to_size_independent_styles(segment.pen(), false, base_style.clone()),
        font_size: segment.pen().font_size,
        time_offset: segment.time_offset,
        text: text.into(),
        shadow: Srv3TextShadow {
            kind: segment.pen().edge_type,
            color: BGRA8::from_argb32(segment.pen().edge_color | 0xFF000000),
        },
        ruby,
    }
}

struct WindowBuilder<'a> {
    sbr: &'a Subrandr,
    base_style: ComputedStyle,
    logset: LogOnceSet,
}

impl WindowBuilder<'_> {
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

                    let ruby = match part.position {
                        RubyPosition::Alternate => Ruby::Over,
                        RubyPosition::Over => Ruby::Over,
                        RubyPosition::Under => {
                            warning!(
                                self.sbr,
                                once(ruby_under_unsupported),
                                "`ruby-position: under`-style ruby text is not supported yet"
                            );
                            break 'ruby_failed;
                        }
                    };

                    current_line.segments.push(convert_segment(
                        segment,
                        &segment.text,
                        Ruby::Base,
                        &self.base_style,
                    ));
                    _ = it.next().unwrap();
                    let next = it.next().unwrap();
                    current_line.segments.push(convert_segment(
                        next,
                        &next.text,
                        ruby,
                        &self.base_style,
                    ));
                    _ = it.next().unwrap();

                    continue 'segment_loop;
                }
            }

            let mut last = 0;
            loop {
                let end = segment.text[last..]
                    .find('\n')
                    .map_or(segment.text.len(), |i| last + i);

                current_line.segments.push(convert_segment(
                    segment,
                    &segment.text[last..end],
                    Ruby::None,
                    &self.base_style,
                ));

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

pub fn convert(sbr: &Subrandr, document: Document) -> Subtitles {
    let mut result = Subtitles {
        windows: Vec::new(),
    };
    let mut window_builder = WindowBuilder {
        sbr,
        base_style: pen_to_size_independent_styles(&Pen::DEFAULT, true, ComputedStyle::DEFAULT),
        logset: LogOnceSet::new(),
    };

    let mut wname_to_index = HashMap::new();
    for (name, window) in document.windows() {
        wname_to_index.insert(&**name, result.windows.len());
        result.windows.push(Window {
            x: convert_coordinate(window.position().x as f32),
            y: convert_coordinate(window.position().y as f32),
            range: window.time..window.time + window.duration,
            alignment: window.position().point.to_alignment(),
            lines: Vec::new(),
        });
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
            let mut window = Window {
                x: convert_coordinate(event.position().x as f32),
                y: convert_coordinate(event.position().y as f32),
                range: event.time..event.time + event.duration,
                alignment: event.position().point.to_alignment(),
                lines: Vec::new(),
            };
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
