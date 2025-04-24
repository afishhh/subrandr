/// Converts parsed SRV3 subtitles into Subtitles.
///
/// Was initially based on YTSubConverter, now also reverse engineered from YouTube's captions.js.
use crate::{
    color::BGRA8,
    log::{log_once_state, warning},
    math::{I16Dot16, I26Dot6, Point2, Point2f, Vec2f},
    CssTextShadow, Event, EventExtra, Layouter, Ruby, Subrandr, SubtitleClass, SubtitleContext,
    Subtitles, TextDecorations, TextSegment,
};

use super::{Document, EdgeType, RubyPart};

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
        // "Deja Vu Sans Mono" is not a real font :(
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
    &["Comis Sans Ms", "Impact", "Handlee", "fantasy"],
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
    match style {
        ..=4 => style.checked_sub(1),
        5 => Some(u32::MAX),
        6.. => Some(style),
    }
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
    pub(crate) fn to_css(&self, ctx: &SubtitleContext, out: &mut Vec<CssTextShadow>) {
        let a = font_scale_from_ctx(ctx) / 32.0;
        let e = a.max(1.0);
        let l = (2.0 * a).max(1.0);
        let mut t = (3.0 * a).max(1.0);
        let c = (5.0 * a).max(1.0);

        match self.kind {
            EdgeType::None => unreachable!(),
            EdgeType::HardShadow => {
                // in captions.js it is window.devicePixelRatio >= 2 ? 0.5 : 1
                // BUT that is NOT what we want, I think they do this to increase fidelity on displays
                // with a lower DPI, because browsers scale all their units by window.devicePixelRatio
                // however we're working with direct device pixels here, so we want to do the OPPOSITE
                // of what they do and pick 0.5 when we have less pixels.
                let step = (ctx.dpi >= 144) as i32 as f32 * 0.5 + 0.5;
                let mut x = e;
                while x <= t {
                    out.push(CssTextShadow {
                        offset: Vec2f::new(ctx.pixels_from_css(x), ctx.pixels_from_css(x)),
                        blur_radius: I26Dot6::ZERO,
                        color: self.color,
                    });
                    x += step;
                }
            }
            EdgeType::Bevel => {
                let offset = Vec2f::new(ctx.pixels_from_css(e), ctx.pixels_from_css(e));
                out.push(CssTextShadow {
                    offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
                out.push(CssTextShadow {
                    offset: -offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
            }
            EdgeType::Glow => out.extend(std::iter::repeat_n(
                CssTextShadow {
                    offset: Vec2f::ZERO,
                    blur_radius: I26Dot6::from_f32(ctx.pixels_from_css(l)),
                    color: self.color,
                },
                5,
            )),
            EdgeType::SoftShadow => {
                let offset = Vec2f::new(ctx.pixels_from_css(l), ctx.pixels_from_css(l));
                while t <= c {
                    out.push(CssTextShadow {
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

#[derive(Debug, Clone)]
pub(crate) struct Srv3Event {
    x: f32,
    y: f32,
}

struct Srv3Layouter;

impl Layouter for Srv3Layouter {
    fn wrap_width(&self, ctx: &SubtitleContext, _event: &Event) -> I26Dot6 {
        ctx.player_width() * 0.96
    }

    fn layout(
        &mut self,
        ctx: &SubtitleContext,
        _lines: &mut Vec<crate::text::layout::ShapedLine>,
        _total_rect: &mut crate::math::Rect2<crate::math::I26Dot6>,
        event: &Event,
    ) -> Point2f {
        let EventExtra::Srv3(extra) = &event.extra else {
            panic!("Srv3Layouter received foreign event {:?}", event);
        };

        Point2::new(
            extra.x * ctx.player_width().into_f32(),
            extra.y * ctx.player_height().into_f32(),
        )
    }
}

fn convert_segment(segment: &super::Segment, ruby: Ruby) -> crate::TextSegment {
    let mut shadows = Vec::new();

    if segment.pen().edge_type != EdgeType::None {
        let edge_color = BGRA8::from_argb32(segment.pen().edge_color | 0xFF000000);
        shadows.push(crate::TextShadow::Srv3(Srv3TextShadow {
            kind: segment.pen().edge_type,
            color: edge_color,
        }));
    }

    TextSegment {
        font: font_style_to_name(segment.pen().font_style)
            .iter()
            .copied()
            .map(str::to_owned)
            .collect(),
        font_size: {
            let mut base = pixels_to_points(font_size_to_pixels(segment.pen().font_size) * 0.75);
            if matches!(ruby, Ruby::Over) {
                base /= 2.0;
            }
            I26Dot6::from_f32(base)
        },
        font_weight: if segment.pen().bold {
            I16Dot16::new(700)
        } else {
            I16Dot16::new(400)
        },
        italic: segment.pen().italic,
        decorations: TextDecorations {
            ..Default::default()
        },
        color: BGRA8::from_rgba32(segment.pen().foreground_color),
        background_color: BGRA8::from_rgba32(segment.pen().background_color),
        text: segment.text.clone(),
        shadows,
        ruby,
    }
}

pub fn convert(sbr: &Subrandr, document: Document) -> Subtitles {
    let mut result = Subtitles {
        class: &SubtitleClass {
            name: "srv3",
            get_font_size: |ctx, _event, segment| -> I26Dot6 {
                segment.font_size * font_scale_from_ctx(ctx)
            },
            create_layouter: || Box::new(Srv3Layouter),
        },
        events: vec![],
    };

    log_once_state!(ruby_under_unsupported);

    for event in document.events() {
        let mut segments = vec![];

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
                                "Ruby `ruby-position: under`-style ruby text is not supported yet"
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

        result.events.push(Event {
            start: event.time,
            end: event.time + event.duration,
            extra: EventExtra::Srv3(Srv3Event {
                x: convert_coordinate(event.position().x as f32),
                y: convert_coordinate(event.position().y as f32),
            }),
            alignment: match event.position().point {
                super::Point::TopLeft => crate::Alignment(
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::TopCenter => crate::Alignment(
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::TopRight => crate::Alignment(
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::MiddleLeft => crate::Alignment(
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::MiddleCenter => crate::Alignment(
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::MiddleRight => crate::Alignment(
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::BottomLeft => crate::Alignment(
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::Bottom,
                ),
                super::Point::BottomCenter => crate::Alignment(
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::Bottom,
                ),
                super::Point::BottomRight => crate::Alignment(
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::Bottom,
                ),
            },
            text_wrap: crate::TextWrapOptions::default(),
            segments,
        })
    }

    result
}
