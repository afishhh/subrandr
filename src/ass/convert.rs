use crate::{Segment, TextSegment, TextWrappingMode};

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
                Text(content) => {
                    let mut text = String::new();
                    let mut it = content.chars();
                    // TODO: How should this behave?
                    while let Some(c) = it.next() {
                        if c == '\\' {
                            match it.next() {
                                Some('\\') => text.push('\\'),
                                Some('N') => text.push('\n'),
                                Some(c) => {
                                    text.push('\\');
                                    text.push(c)
                                }
                                None => text.push('\\'),
                            }
                        } else {
                            text.push(c);
                        }
                    }

                    segments.push(Segment::Text(TextSegment {
                        font: current_style.fontname.to_string(),
                        font_size: current_style.fontsize,
                        font_weight: current_style.weight,
                        italic: current_style.italic,
                        underline: current_style.underline,
                        strike_out: current_style.strike_out,
                        color: ass_to_rgba(current_style.primary_colour),
                        text,
                    }))
                }
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
