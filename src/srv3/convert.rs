use std::{ops::Range, rc::Rc};

/// Converts parsed SRV3 subtitles into Subtitles.
///
/// Was initially based on YTSubConverter, now also reverse engineered from YouTube's captions.js.
use crate::{
    color::BGRA8,
    layout::{
        self, BlockContainer, FixedL, InlineContainer, InlineLayoutError, InlineText,
        LayoutConstraints, Point2L, Vec2L,
    },
    log::{log_once_state, warning},
    math::{I16Dot16, I26Dot6, Vec2f},
    renderer::FrameLayoutPass,
    style::{
        self,
        types::{Alignment, FontSlant, HorizontalAlignment, Ruby, TextShadow, VerticalAlignment},
        StyleMap,
    },
    Subrandr, SubtitleContext,
};

use super::{Document, EdgeType, Pen, RubyPart};

const SRV3_FONTS: &[&[&str]] = &[
    &[
        "Courier New",
        "Courier",
        "Nimbus Mono L",
        "Cutive Mono",
        "monospace",
    ],
    &[
        "Times New Roman",
        "Times",
        "Georgia",
        "Cambria",
        "PT Serif Caption",
        "serif",
    ],
    &[
        "Deja Vu Sans Mono", // not a real font :(
        "Lucida Console",
        "Monaco",
        "Consolas",
        "PT Mono",
        "monospace",
    ],
    &[
        "YouTube Noto",
        "Roboto",
        "Arial",
        "Helvetica",
        "Verdana",
        "PT Sans Caption",
        "sans-serif",
    ],
    &["Comic Sans Ms", "Impact", "Handlee", "fantasy"],
    &[
        "Monotype Corsiva",
        "URW Chancery L",
        "Apple Chancery",
        "Dancing Script",
        "cursive",
    ],
    // YouTube appears to conditionally set this to either:
    // "Carrois Gothic SC", sans-serif-smallcaps
    // or sometimes:
    // Arial, Helvetica, Verdana, "Marcellus SC", sans-serif
    // the first one seems to be used when ran under Cobalt
    // https://developers.google.com/youtube/cobalt
    // i.e. in YouTube TV
    &[
        "Arial",
        "Helvetica",
        "Verdana",
        "Marcellus SC",
        "sans-serif",
    ],
];

fn font_style_to_name(style: u32) -> &'static [&'static str] {
    style
        .checked_sub(1)
        .and_then(|i| SRV3_FONTS.get(i as usize))
        .map_or(SRV3_FONTS[3], |v| v)
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

#[derive(Debug)]
pub struct Subtitles {
    root_style: StyleMap,
    events: Vec<Event>,
}

#[derive(Debug)]
struct Event {
    x: f32,
    y: f32,
    range: Range<u32>,
    alignment: Alignment,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    base_style: StyleMap,
    font_size: u16,
    time_offset: u32,
    text: Rc<str>,
    shadow: Srv3TextShadow,
    ruby: Ruby,
}

impl Event {
    pub fn layout(
        &self,
        pass: &mut FrameLayoutPass,
        style: &StyleMap,
    ) -> Result<(Point2L, layout::BlockContainerFragment), layout::InlineLayoutError> {
        let segments = self
            .segments
            .iter()
            .filter_map(|segment| {
                pass.add_animation_point(self.range.start + segment.time_offset);

                if segment.time_offset <= pass.t - self.range.start {
                    Some(InlineText {
                        style: {
                            let mut result = segment.base_style.clone();

                            let mut size =
                                pixels_to_points(font_size_to_pixels(segment.font_size) * 0.75)
                                    * font_scale_from_ctx(pass.sctx);
                            if matches!(segment.ruby, Ruby::Over) {
                                size /= 2.0;
                            }

                            result.set::<style::FontSize>(I26Dot6::from(size));

                            let mut shadows = vec![];
                            segment.shadow.to_css(pass.sctx, &mut shadows);

                            if !shadows.is_empty() {
                                result.set::<style::TextShadows>(shadows)
                            }

                            result
                        },
                        text: segment.text.clone(),
                        ruby: segment.ruby,
                    })
                } else {
                    None
                }
            })
            .collect();

        let block = BlockContainer {
            style: {
                let mut result = StyleMap::new();

                if self.alignment.0 != HorizontalAlignment::Left {
                    result.set::<style::TextAlign>(self.alignment.0);
                }

                result
            },
            contents: vec![InlineContainer {
                contents: segments,
                ..InlineContainer::default()
            }],
        };

        let constraints = LayoutConstraints {
            size: Vec2L::new(pass.sctx.player_width() * 96 / 100, FixedL::MAX),
        };

        let fragment = layout::layout(pass.lctx, constraints, &block, style)?;

        let mut pos = Point2L::new(
            (self.x * pass.sctx.player_width().into_f32()).into(),
            (self.y * pass.sctx.player_height().into_f32()).into(),
        );

        match self.alignment.0 {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => pos.x -= fragment.fbox.size.x / 2,
            HorizontalAlignment::Right => pos.x -= fragment.fbox.size.x,
        }

        match self.alignment.1 {
            VerticalAlignment::Top => (),
            VerticalAlignment::Center => pos.y -= fragment.fbox.size.y / 2,
            VerticalAlignment::Bottom => pos.y -= fragment.fbox.size.y,
        }

        Ok((pos, fragment))
    }
}

fn pen_to_size_independent_styles(pen: &Pen, set_default: bool) -> StyleMap {
    let mut result = StyleMap::new();

    if set_default || pen.font_style != Pen::DEFAULT.font_style {
        result.set::<style::FontFamily>(
            font_style_to_name(pen.font_style)
                .iter()
                .copied()
                .map(Box::<str>::from)
                .collect(),
        );
    }

    if pen.bold {
        result.set::<style::FontWeight>(I16Dot16::new(700));
    }

    if pen.italic {
        result.set::<style::FontStyle>(FontSlant::Italic);
    }

    if set_default || pen.foreground_color != Pen::DEFAULT.foreground_color {
        result.set::<style::Color>(BGRA8::from_rgba32(pen.foreground_color));
    }

    if set_default || pen.background_color != Pen::DEFAULT.background_color {
        result.set::<style::BackgroundColor>(BGRA8::from_rgba32(pen.background_color));
    }

    result
}

fn convert_segment(segment: &super::Segment, ruby: Ruby) -> Segment {
    let style = pen_to_size_independent_styles(segment.pen(), false);

    Segment {
        base_style: style,
        font_size: segment.pen().font_size,
        time_offset: segment.time_offset,
        text: segment.text.as_str().into(),
        shadow: Srv3TextShadow {
            kind: segment.pen().edge_type,
            color: BGRA8::from_argb32(segment.pen().edge_color | 0xFF000000),
        },
        ruby,
    }
}

pub fn convert(sbr: &Subrandr, document: Document) -> Subtitles {
    let mut result = Subtitles {
        root_style: pen_to_size_independent_styles(&Pen::DEFAULT, true),
        events: vec![],
    };

    log_once_state!(ruby_under_unsupported, window_unsupported);

    for event in document.events() {
        let mut segments = vec![];

        if event.window_id.is_some() {
            warning!(
                sbr,
                once(window_unsupported),
                "Explicit windows on events are not supported yet"
            )
        }

        let mut it = event.segments.iter();
        'segment_loop: while let Some(segment) = it.next() {
            'ruby_failed: {
                if segment.pen().ruby_part == RubyPart::Base && it.as_slice().len() > 3 {
                    let ruby_block = <&[_; 3]>::try_from(&it.as_slice()[..3]).unwrap();

                    if !matches!(
                        ruby_block.each_ref().map(|s| s.pen().ruby_part),
                        [
                            RubyPart::Parenthesis,
                            RubyPart::Over | RubyPart::Under,
                            RubyPart::Parenthesis,
                        ]
                    ) {
                        break 'ruby_failed;
                    }
                    let ruby = match ruby_block[1].pen().ruby_part {
                        RubyPart::Over => Ruby::Over,
                        RubyPart::Under => {
                            warning!(
                                sbr,
                                once(ruby_under_unsupported),
                                "`ruby-position: under`-style ruby text is not supported yet"
                            );
                            break 'ruby_failed;
                        }
                        _ => unreachable!(),
                    };

                    segments.push(convert_segment(segment, Ruby::Base));
                    _ = it.next().unwrap();
                    segments.push(convert_segment(it.next().unwrap(), ruby));
                    _ = it.next().unwrap();

                    continue 'segment_loop;
                }
            }

            segments.push(convert_segment(segment, Ruby::None));
        }

        let alignment = match event.position().point {
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
        };

        result.events.push(Event {
            x: convert_coordinate(event.position().x as f32),
            y: convert_coordinate(event.position().y as f32),
            range: event.time..event.time + event.duration,
            alignment,
            segments,
        })
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
        for event in &self.subtitles.events {
            if !pass.add_event_range(event.range.clone()) {
                continue;
            }

            let (pos, block) = event.layout(pass, &self.subtitles.root_style)?;
            pass.emit_fragment(pos, block);
        }

        Ok(())
    }
}
