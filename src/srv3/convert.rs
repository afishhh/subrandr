/// Converts parsed SRV3 subtitles into Subtitles.
///
/// Was initially based on YTSubConverter, now mostly reverse engineered straight from YouTube's captions.js.
use crate::{
    color::BGRA8,
    math::{Point2, Vec2},
    CssTextShadow, Event, SubtitleClass, SubtitleContext, Subtitles, TextDecorations, TextSegment,
};

use super::{Document, EdgeType};

const SRV3_FONTS: &[&str] = &[
    "Roboto",          // also the default
    "Courier New",     // '"Courier New", Courier, "Nimbus Mono L", "Cutive Mono", monospace'
    "Times New Roman", // '"Times New Roman", Times, Georgia, Cambria, "PT Serif Caption", serif
    // "Deja Vu Sans Mono" is not a real font so browsers fall back to Lucida Console
    "Lucida Console", // '"Deja Vu Sans Mono", "Lucida Console", Monaco, Consolas, "PT Mono", monospace'
    "Roboto", // '"YouTube Noto", Roboto, Arial, Helvetica, Verdana, "PT San     s Caption", sans-serif'
    "Comis Sans Ms", // '"Comic Sans MS", Impact, Handlee, fantasy'
    "Monotype Corsiva", // '"Monotype Corsiva", "URW Chancery L", "Apple Chancery", "D     ancing Script", cursive'
    // TODO: This should also select the "small-caps" font variant
    "Carrois Gothic Sc", // '"Carrois Gothic SC", sans-serif-smallcaps' : 'Arial, Helvetica     , Verdana, "Marcellus SC", sans-serif'
];

fn font_style_to_name(style: u32) -> &'static str {
    SRV3_FONTS.get(style as usize).map_or(SRV3_FONTS[0], |v| v)
}

fn convert_coordinate(coord: f32) -> f32 {
    0.02 + coord * 0.0096
}

// NWG = function (p, C, V, N) {
//   var H = C / 360 * 16;
//   C >= p &&
//   (p = 640, N > V * 1.3 && (p = 480), H = V / p * 16);
//   return H
// },
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
        to_css_pixels(ctx.video_width, ctx.dpi),
        to_css_pixels(ctx.video_height, ctx.dpi),
        to_css_pixels(ctx.player_width(), ctx.dpi),
        to_css_pixels(ctx.player_height(), ctx.dpi),
    )
}

// Hey = function (p) {
//   var C = 1 + 0.25 * (p.fontSizeIncrement || 0);
//   if (p.offset === 0 || p.offset === 2) C *= 0.8;
//   return C
// },
#[allow(clippy::let_and_return)] // shut up
fn font_size_to_pixels(size: u16) -> f32 {
    // fontSizeIncrement is acqiured via H.szPenSize / 100 - 1
    let c = 1.0 + 0.25 * (size as f32 / 100.0 - 1.0);
    // offset is "H.ofOffset", don't know what that is
    //  if (p.offset === 0 || p.offset === 2) C *= 0.8;
    c
}

// 1px = 1/96in
fn to_css_pixels(real_pixels: f32, dpi: u32) -> f32 {
    (real_pixels / dpi as f32) * 96.0
}

fn to_real_pixels(css_pixels: f32, dpi: u32) -> f32 {
    css_pixels * (dpi as f32 / 96.0)
}

fn css_pixels_to_dpi72(value: f32) -> f32 {
    value * (72.0 / 96.0)
}

#[derive(Debug, Clone)]
pub struct Srv3TextShadow {
    // never None
    kind: EdgeType,
    color: BGRA8,
}

impl Srv3TextShadow {
    pub(crate) fn to_css(&self, ctx: &SubtitleContext, out: &mut Vec<CssTextShadow>) {
        let a = calculate_font_scale(
            ctx.video_width,
            ctx.video_height,
            ctx.player_width(),
            ctx.player_height(),
        ) / 32.0;
        let e = a.max(1.0);
        let l = (2.0 * a).max(1.0);
        let t = (3.0 * a).max(1.0);
        let c = (5.0 * a).max(1.0);

        match self.kind {
            EdgeType::None => unreachable!(),
            EdgeType::HardShadow => {
                // in captions.js it is window.devicePixelRatio >= 2 ? 0.5 : 1
                // BUT that is NOT what we want, I think they do this to increase fidelity on displays
                // with a lower DPI, because browsers scale all their units by window.devicePixelRation
                // however we're working with direct device pixels here, so we want to do the OPPOSITE
                // of what they do and pick 0.5 when we have less pixels.
                let step = (ctx.dpi >= 144) as i32 as f32 * 0.5 + 0.5;
                let mut x = e;
                while x <= t {
                    out.push(CssTextShadow {
                        offset: Vec2::new(to_real_pixels(x, ctx.dpi), to_real_pixels(x, ctx.dpi)),
                        color: self.color,
                    });
                    x += step;
                }
            }
            EdgeType::Bevel => {
                let offset = Vec2::new(to_real_pixels(e, ctx.dpi), to_real_pixels(e, ctx.dpi));
                out.push(CssTextShadow {
                    offset,
                    color: self.color,
                });
                out.push(CssTextShadow {
                    offset: -offset,
                    color: self.color,
                });
            }
            EdgeType::Glow => (),       // todo!(),
            EdgeType::SoftShadow => (), // ,
        }
    }
}

#[derive(Debug)]
struct Srv3SubtitleClass;
impl SubtitleClass for Srv3SubtitleClass {
    fn get_name(&self) -> &'static str {
        "srv3"
    }

    fn get_font_size(&self, ctx: &SubtitleContext, _event: &Event, segment: &TextSegment) -> f32 {
        font_scale_from_ctx(ctx) * segment.font_size
    }

    fn get_position(&self, ctx: &SubtitleContext, event: &Event) -> Point2 {
        Point2::new(event.x * ctx.player_width(), event.y * ctx.player_height())
    }
}

pub fn convert(document: Document) -> Subtitles {
    let mut result = Subtitles {
        class: &Srv3SubtitleClass,
        events: vec![],
    };

    for event in document.events() {
        let mut segments = vec![];

        for segment in event.segments.iter() {
            let mut shadows = Vec::new();
            if segment.pen().edge_type != EdgeType::None {
                shadows.push(crate::TextShadow::Srv3(Srv3TextShadow {
                    kind: segment.pen().edge_type,
                    color: BGRA8::from_rgba32(segment.pen().edge_color),
                }));
            }
            segments.push(crate::Segment::Text(TextSegment {
                font: font_style_to_name(segment.pen().font_style).to_owned(),
                font_size: font_size_to_pixels(segment.pen().font_size) * 0.749_999_4,
                font_weight: if segment.pen().bold { 700 } else { 400 },
                italic: segment.pen().italic,
                decorations: TextDecorations {
                    ..Default::default()
                },
                color: BGRA8::from_rgba32(segment.pen().foreground_color),
                text: segment.text.clone(),
                shadows,
            }))
        }

        result.events.push(Event {
            start: event.time,
            end: event.time + event.duration,
            x: convert_coordinate(event.position().x as f32),
            y: convert_coordinate(event.position().y as f32),
            alignment: match event.position().point {
                super::Point::TopLeft => crate::Alignment::TopLeft,
                super::Point::TopCenter => crate::Alignment::Top,
                super::Point::TopRight => crate::Alignment::TopRight,
                super::Point::MiddleLeft => crate::Alignment::Left,
                super::Point::MiddleCenter => crate::Alignment::Center,
                super::Point::MiddleRight => crate::Alignment::Right,
                super::Point::BottomLeft => crate::Alignment::BottomLeft,
                super::Point::BottomCenter => crate::Alignment::Bottom,
                super::Point::BottomRight => crate::Alignment::BottomRight,
            },
            text_wrap: crate::TextWrappingMode::None,
            segments,
        })
    }

    result
}
