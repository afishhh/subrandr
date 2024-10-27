use crate::{Segment, TextWrappingMode};

use super::*;

type AssAlignment = super::parse::Alignment;
type Alignment = crate::Alignment;

impl From<AssAlignment> for Alignment {
    fn from(value: AssAlignment) -> Self {
        match value {
            AssAlignment::BottomLeft => Alignment::BottomLeft,
            AssAlignment::BottomCenter => Alignment::Bottom,
            AssAlignment::BottomRight => Alignment::BottomRight,
            AssAlignment::MiddleLeft => Alignment::Left,
            AssAlignment::MiddleCenter => Alignment::Center,
            AssAlignment::MiddleRight => Alignment::Right,
            AssAlignment::TopLeft => Alignment::TopLeft,
            AssAlignment::TopCenter => Alignment::Top,
            AssAlignment::TopRight => Alignment::TopRight,
        }
    }
}

pub fn ass_to_rgba(abgr: u32) -> u32 {
    ((abgr & 0xFF) << 24)
        | ((abgr & 0xFF00) << 8)
        | ((abgr & 0xFF0000) >> 8)
        | (0xFF - ((abgr & 0xFF000000) >> 24))
}

pub fn apply_style_to_segment(segment: &mut Segment, style: &Style) {
    segment.font = style.fontname.to_string();
    segment.font_size = style.fontsize;
    segment.font_weight = style.weight;
    segment.italic = style.italic;
    segment.underline = style.underline;
    segment.strike_out = style.strike_out;
    segment.color = ass_to_rgba(style.primary_colour);
}

pub fn convert(ass: Script) -> crate::Subtitles {
    let mut subs = crate::Subtitles { events: vec![] };

    let layout_resolution = if ass.layout_resolution.0 > 0 && ass.layout_resolution.1 > 0 {
        ass.layout_resolution
    } else {
        ass.play_resolution
    };

    for event in ass.events.iter() {
        let event_style = ass.get_style(&event.style).unwrap_or(&DEFAULT_STYLE);

        // TODO: correct and alignment specific values
        let mut x = 0.5;
        let mut y = 0.8;
        let mut alignment = event_style.alignment;

        let tokens = parse_event_text(&event.text);

        let mut segments: Vec<Segment> = vec![];
        let mut current_style = event_style.clone();

        for token in tokens {
            use ParsedTextPart::*;

            match token {
                Text(content) => segments.push(Segment {
                    font: current_style.fontname.to_string(),
                    font_size: current_style.fontsize,
                    font_weight: current_style.weight,
                    italic: current_style.italic,
                    underline: current_style.underline,
                    strike_out: current_style.strike_out,
                    color: ass_to_rgba(current_style.primary_colour),
                    text: content.to_string(),
                }),
                Override(Command::An(a) | Command::A(a)) => alignment = a,
                Override(Command::Pos(nx, ny)) => {
                    let (max_x, max_y) = layout_resolution;
                    x = nx as f32 / max_x as f32;
                    y = ny as f32 / max_y as f32;
                }
                Override(Command::R(style)) => {
                    current_style = ass.get_style(&style).unwrap_or(&DEFAULT_STYLE).clone();
                }
                Override(other) => {
                    eprintln!("ignoring {other:?}");
                }
            }
        }

        subs.events.push(crate::Event {
            start: event.start,
            end: event.end,
            x,
            y,
            alignment: alignment.into(),
            text_wrap: TextWrappingMode::None,
            segments,
        })
    }

    subs
}
