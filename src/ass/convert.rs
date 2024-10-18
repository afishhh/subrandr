use crate::Segment;

use super::parse::*;

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

pub fn bgra_to_rgba(bgra: u32) -> u32 {
    (bgra & 0xFF00 << 16) | (bgra & 0xFF0000) | (bgra & 0xFF000000 >> 16) | (0xFF - (bgra & 0xFF))
}

pub fn apply_style_to_segment(segment: &mut Segment, style: &Style) {
    segment.font = style.fontname.to_string();
    segment.font_size = style.fontsize;
    segment.font_weight = style.weight;
    segment.italic = style.italic;
    segment.underline = style.underline;
    segment.strike_out = style.strike_out;
    segment.color = bgra_to_rgba(style.primary_colour);
}

pub fn ass_to_subs(ass: Script) -> crate::Subtitles {
    let mut subs = crate::Subtitles { events: vec![] };

    let layout_resolution = if ass.layout_resolution.0 > 0 && ass.layout_resolution.1 > 0 {
        ass.layout_resolution
    } else {
        ass.play_resolution
    };

    for event in ass.events.iter() {
        let style = ass.get_style(&event.style).unwrap_or(&DEFAULT_STYLE);

        let mut text = String::new();
        // TODO: correct and alignment specific values
        let mut x = 0.5;
        let mut y = 0.8;
        let mut alignment = style.alignment;

        let tokens = parse_event_text(&event.text);
        dbg!(&tokens);

        for token in tokens {
            use ParsedTextPart::*;
            match token {
                Text(content) => text += content,
                Override(Command::An(a) | Command::A(a)) => alignment = a,
                Override(Command::Pos(nx, ny)) => {
                    let (max_x, max_y) = layout_resolution;
                    x = nx as f32 / max_x as f32;
                    y = ny as f32 / max_y as f32;
                }
                Override(Command::R(style)) => {
                    let style = ass.get_style(&event.style).unwrap_or(&DEFAULT_STYLE);
                }
                Override(other) => {
                    println!("ignoring {other:?}");
                }
            }
        }

        // for part in ass::segment_event_text(&event.text) {
        //     match part {
        //         ass::TextPart::Commands(r) => {
        //             let command_block = &event.text[r];
        //             let mut it = command_block.chars();
        //
        //             while !it.as_str().is_empty() {
        //                 while it.next().is_some_and(|c| c != '\\') {}
        //
        //                 let remainder = it.as_str();
        //                 if remainder.len() >= 3 && &remainder[..3] == "pos" {
        //                     assert_eq!(&remainder[3..4], "(");
        //                     let args_end = remainder.find(')').unwrap();
        //                     let args = &remainder[4..args_end];
        //                     let (left, right) = args.split_once(',').unwrap();
        //                     let tx = left.parse::<u32>().unwrap();
        //                     let ty = right.parse::<u32>().unwrap();
        //                     let (max_x, max_y) = layout_resolution;
        //                     x = tx as f32 / max_x as f32;
        //                     y = ty as f32 / max_y as f32
        //                 };
        //                 if remainder.len() >= 2 && &remainder[..2] == "an" {
        //                     alignment = ass::Alignment::from_ass(&remainder[2..3]).unwrap();
        //                 }
        //                 println!("{x} {y}");
        //             }
        //         }
        //         ass::TextPart::Content(c) => {
        //             text += &event.text[c];
        //         }
        //     }
        // }

        subs.events.push(crate::Event {
            start: event.start,
            end: event.end,
            x,
            y,
            alignment: alignment.into(),
            segments: vec![Segment {
                font: style.fontname.to_string(),
                font_size: style.fontsize,
                font_weight: style.weight,
                italic: style.italic,
                underline: style.underline,
                strike_out: style.strike_out,
                color: (style.primary_colour & 0xFF00 << 16)
                    | (style.primary_colour & 0xFF0000)
                    | (style.primary_colour & 0xFF000000 >> 16)
                    | (0xFF - (style.primary_colour & 0xFF)),
                text,
            }],
        })
    }

    subs
}
