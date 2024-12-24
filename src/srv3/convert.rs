use crate::{color::BGRA8, math::Point2, SubtitleClass, Subtitles, TextDecorations};

use super::Document;

const BASE_FONT_SIZE: u32 = 38;

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

// fn font_size_to_pt(size: u16) -> f32 {
//     BASE_FONT_SIZE as f32 * (1.0 + ((size as f32 / 100.0) - 1.0) / 4.0)
// }

// NWG = function (p, C, V, N) {
//   var H = C / 360 * 16;
//   C >= p &&
//   (p = 640, N > V * 1.3 && (p = 480), H = V / p * 16);
//   return H
// },
fn get_font_scale(
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
fn to_css_pixels(value: f32, dpi: u32) -> f32 {
    (value / dpi as f32) * 96.0
}

#[derive(Debug)]
struct Srv3SubtitleClass;
impl SubtitleClass for Srv3SubtitleClass {
    fn get_name(&self) -> &'static str {
        "srv3"
    }

    fn get_font_size(
        &self,
        ctx: &crate::SubtitleContext,
        _event: &crate::Event,
        segment: &crate::TextSegment,
    ) -> f32 {
        get_font_scale(
            to_css_pixels(ctx.video_width, ctx.dpi),
            to_css_pixels(ctx.video_height, ctx.dpi),
            to_css_pixels(ctx.player_width(), ctx.dpi),
            to_css_pixels(ctx.player_height(), ctx.dpi),
        ) * segment.font_size
    }

    fn get_position(
        &self,
        ctx: &crate::SubtitleContext,
        event: &crate::Event,
    ) -> crate::math::Point2 {
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
            segments.push(crate::Segment::Text(crate::TextSegment {
                font: font_style_to_name(segment.pen().font_style).to_owned(),
                font_size: font_size_to_pixels(segment.pen().font_size) * 0.749_999_4,
                font_weight: if segment.pen().bold { 700 } else { 400 },
                italic: segment.pen().italic,
                decorations: TextDecorations::none(),
                color: BGRA8::from_rgba32(segment.pen().foreground_color),
                text: segment.text.clone(),
            }))
        }

        result.events.push(crate::Event {
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
