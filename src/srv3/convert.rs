use crate::Subtitles;

use super::Document;

const BASE_FONT_SIZE: u32 = 38;

pub fn font_size_to_pt(size: u16) -> f32 {
    BASE_FONT_SIZE as f32 * (1.0 + ((size as f32 / 100.0) - 1.0) / 4.0)
}

pub fn convert(document: Document) -> Subtitles {
    let mut result = Subtitles { events: vec![] };

    for event in document.events() {
        let mut segments = vec![];

        for segment in event.segments.iter() {
            segments.push(crate::Segment {
                font: "".to_string(),
                font_size: font_size_to_pt(segment.pen().font_size),
                font_weight: if segment.pen().bold { 700 } else { 400 },
                italic: false,
                underline: false,
                strike_out: false,
                color: segment.pen().foreground_color,
                text_wrap: crate::TextWrappingMode::None,
                text: segment.text.clone(),
            })
        }

        result.events.push(dbg!(crate::Event {
            start: event.time,
            end: event.time + event.duration,
            x: event.position().x as f32 / 100.,
            y: event.position().y as f32 / 100.,
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
            segments,
        }))
    }

    result
}
