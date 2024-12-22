use crate::{color::BGRA8, Subtitles};

use super::Document;

const BASE_FONT_SIZE: u32 = 38;

const SRV3_FONTS: &[&str] = &[
    "Roboto", // also the default
    "Courier New",
    "Times New Roman",
    "Lucida Console",
    "Comis Sans Ms",
    "Monotype Corsiva",
    "Carrois Gothic Sc",
];

fn font_style_to_name(style: u32) -> &'static str {
    SRV3_FONTS.get(style as usize).map_or(SRV3_FONTS[0], |v| v)
}

fn font_size_to_pt(size: u16) -> f32 {
    BASE_FONT_SIZE as f32 * (1.0 + ((size as f32 / 100.0) - 1.0) / 4.0)
}

fn convert_coordinate(coord: f32) -> f32 {
    (2.0 + coord * 0.96) / 100.
}

pub fn convert(document: Document) -> Subtitles {
    let mut result = Subtitles { events: vec![] };

    for event in document.events() {
        let mut segments = vec![];

        for segment in event.segments.iter() {
            segments.push(crate::Segment::Text(crate::TextSegment {
                font: font_style_to_name(segment.pen().font_style).to_owned(),
                font_size: font_size_to_pt(segment.pen().font_size),
                font_weight: if segment.pen().bold { 700 } else { 400 },
                italic: false,
                underline: false,
                strike_out: false,
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
