/// Converts parsed SRV3 subtitles into Subtitles.
///
/// Was initially based on YTSubConverter, now also reverse engineered from YouTube's captions.js.
use crate::{
    color::BGRA8,
    log::{log_once_state, warning},
    math::{I16Dot16, I26Dot6, Vec2, Vec2f},
    srv3::{RubyPart, Segment},
    text::{self, layout::MultilineTextShaper},
    Alignment, CssTextShadow, FrameRenderPass, RenderError, SubtitleContext, TextDecorations,
    VerticalAlignment,
};

use super::EdgeType;

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

fn apply_coordinate(coord: u32, full: I26Dot6) -> I26Dot6 {
    // full * (0.02 + coord * 0.0096)
    full / 50 + full * coord as i32 * 96 / 10000
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
struct TextShadow {
    kind: EdgeType,
    color: BGRA8,
}

impl TextShadow {
    pub(crate) fn to_css(&self, ctx: &SubtitleContext, out: &mut Vec<CssTextShadow>) {
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

struct Event {
    start: u32,
    end: u32,
    alignment: Alignment,
    segments: Vec<TSegment>,
    x: I26Dot6,
    y: I26Dot6,
}

struct TSegment {
    text: Box<str>,
    font: &'static [&'static str],
    base_size: I26Dot6,
    bold: bool,
    italic: bool,
    color: BGRA8,
    background_color: BGRA8,
    shadow: TextShadow,
    ruby: Ruby,
}

enum Ruby {
    None,
    Base,
    Over,
}

struct Subtitles {
    events: Vec<Event>,
}

pub struct Renderer {}

impl Renderer {
    pub fn new() -> Self {
        Self {}
    }

    pub fn render(
        &mut self,
        pass: &mut FrameRenderPass,
        subs: &Subtitles,
    ) -> Result<(), RenderError> {
        log_once_state!(in pass.logset; ruby_under_unsupported, window_unsupported);

        for event in subs.events.iter() {
            if !pass.add_event_range(event.start..event.end) {
                continue;
            }

            if event.window_id.is_some() {
                warning!(
                    pass.sbr,
                    once(window_unsupported),
                    "Explicit windows on events are not supported yet"
                )
            }

            let mut font_matcher_for = |segment: &Segment, ruby_annotation: bool| {
                text::FontMatcher::match_all(
                    font_style_to_name(segment.pen.font_style),
                    text::FontStyle {
                        weight: if segment.pen.bold {
                            I16Dot16::new(700)
                        } else {
                            I16Dot16::new(400)
                        },
                        italic: segment.pen.italic,
                    },
                    {
                        let mut base =
                            pixels_to_points(font_size_to_pixels(segment.pen.font_size) * 0.75);
                        if ruby_annotation {
                            base /= 2.0;
                        }
                        I26Dot6::from_f32(base * font_scale_from_ctx(pass.ctx))
                    },
                    pass.ctx.dpi,
                    &pass.font_arena,
                    &mut pass.fonts,
                )
            };

            let mut shaper = MultilineTextShaper::new();

            let mut it = event.segments.iter();
            'segment_loop: while let Some(segment) = it.next() {
                'ruby_failed: {
                    if segment.pen.ruby_part == RubyPart::Base && it.as_slice().len() > 3 {
                        let ruby_block = <&[_; 3]>::try_from(&it.as_slice()[..3]).unwrap();

                        if !matches!(
                            ruby_block.each_ref().map(|s| s.pen.ruby_part),
                            [
                                RubyPart::Parenthesis,
                                RubyPart::Over | RubyPart::Under,
                                RubyPart::Parenthesis,
                            ]
                        ) {
                            break 'ruby_failed;
                        }

                        if let RubyPart::Under = ruby_block[1].pen.ruby_part {
                            warning!(
                                pass.sbr,
                                once(ruby_under_unsupported),
                                "`ruby-position: under`-style ruby text is not supported yet"
                            );
                            break 'ruby_failed;
                        };

                        let base_id =
                            shaper.add_ruby_base(&segment.text, font_matcher_for(segment, false)?);

                        _ = it.next().unwrap();
                        shaper.skip_segment_for_output();

                        let annotation = it.next().unwrap();
                        shaper.add_ruby_annotation(
                            base_id,
                            &annotation.text,
                            font_matcher_for(annotation, true)?,
                        );

                        _ = it.next().unwrap();
                        shaper.skip_segment_for_output();

                        continue 'segment_loop;
                    }
                }

                shaper.add_text(&segment.text, font_matcher_for(segment, false)?);
            }

            let (halign, valign) = match event.position.point {
                super::Point::TopLeft => (
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::TopCenter => (
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::TopRight => (
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::Top,
                ),
                super::Point::MiddleLeft => (
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::MiddleCenter => (
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::MiddleRight => (
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::BaselineCentered,
                ),
                super::Point::BottomLeft => (
                    crate::HorizontalAlignment::Left,
                    crate::VerticalAlignment::Bottom,
                ),
                super::Point::BottomCenter => (
                    crate::HorizontalAlignment::Center,
                    crate::VerticalAlignment::Bottom,
                ),
                super::Point::BottomRight => (
                    crate::HorizontalAlignment::Right,
                    crate::VerticalAlignment::Bottom,
                ),
            };

            let (lines, total_rect) = shaper.shape(
                halign,
                text::layout::TextWrapOptions::default(),
                pass.ctx.player_width() * 96 / 100,
                &pass.font_arena,
                &mut pass.fonts,
            )?;

            let x = apply_coordinate(event.position.x, pass.ctx.player_width());
            let y = apply_coordinate(event.position.y, pass.ctx.player_height())
                + match valign {
                    VerticalAlignment::Top => I26Dot6::ZERO,
                    VerticalAlignment::BaselineCentered => -total_rect.height() / 2,
                    VerticalAlignment::Bottom => -total_rect.height(),
                };

            pass.draw_text_total_rect_debug_info(total_rect.translate(Vec2::new(x, y)), valign)?;

            pass.draw_simple_text_background_boxes(
                x,
                y,
                lines.iter().map(|l| &l.segments),
                |index| BGRA8::from_rgba32(event.segments[index].pen.background_color),
            );

            pass.draw_text_segments_full(
                x,
                y,
                lines.iter().flat_map(|line| &line.segments),
                |index, out_shadows| {
                    let segment = &event.segments[index];

                    TextShadow {
                        kind: segment.pen.edge_type,
                        color: BGRA8::from_argb32(segment.pen.edge_color | 0xFF000000),
                    }
                    .to_css(pass.ctx, out_shadows);

                    (
                        BGRA8::from_rgba32(segment.pen.foreground_color),
                        TextDecorations::default(),
                    )
                },
            )?;
        }

        Ok(())
    }
}
