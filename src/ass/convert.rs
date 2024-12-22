use std::str::Chars;

use crate::{
    color::BGRA8,
    math::Point2,
    outline::{CurveDegree, Outline, OutlineBuilder},
    Segment, ShapeSegment, TextSegment, TextWrappingMode,
};

use super::*;

type AssAlignment = super::parse::Alignment;
type Alignment = crate::Alignment;

impl From<AssAlignment> for Alignment {
    fn from(value: AssAlignment) -> Self {
        match value {
            AssAlignment::BottomLeft => Self::BottomLeft,
            AssAlignment::BottomCenter => Self::Bottom,
            AssAlignment::BottomRight => Self::BottomRight,
            AssAlignment::MiddleLeft => Self::Left,
            AssAlignment::MiddleCenter => Self::Center,
            AssAlignment::MiddleRight => Self::Right,
            AssAlignment::TopLeft => Self::TopLeft,
            AssAlignment::TopCenter => Self::Top,
            AssAlignment::TopRight => Self::TopRight,
        }
    }
}

fn skip_space(chars: &mut Chars) -> Option<()> {
    let string = chars.as_str();
    let space_end = string.find(|c| c != ' ')?;
    *chars = string[space_end..].chars();
    Some(())
}

fn get_f32(chars: &mut Chars) -> Option<f32> {
    skip_space(chars)?;
    // TODO: Maybe make this operate on &[u8] instead so no UTF-8
    //       decoding has to be performed while iterating
    let string = chars.as_str();

    let has_leading_dot = string.starts_with('.');
    let without_dot = &string[has_leading_dot as usize..];

    let Some(mut number_end) = without_dot.find(|c: char| !c.is_ascii_digit()) else {
        *chars = string[string.len()..].chars();
        return string.parse::<f32>().ok();
    };

    if !has_leading_dot && string.as_bytes()[number_end] == b'.' {
        let fraction_start = number_end + 1;
        let Some(fraction_len) = without_dot[fraction_start..].find(|c: char| !c.is_ascii_digit())
        else {
            *chars = string[string.len()..].chars();
            return string.parse::<f32>().ok();
        };

        number_end += fraction_len + 1;
    }

    let maybe_e = string.as_bytes()[number_end];
    if maybe_e == b'E' || maybe_e == b'e' {
        number_end += 1;

        let Some(sign) = string.as_bytes().get(number_end).copied() else {
            *chars = string[string.len()..].chars();
            return None;
        };

        if sign == b'+' || sign == b'-' {
            number_end += 1;
        }

        let Some(exp_len) = string[number_end..].find(|c: char| !c.is_ascii_digit()) else {
            *chars = string[string.len()..].chars();
            return string.parse::<f32>().ok();
        };

        number_end += exp_len;
    }

    *chars = string[number_end..].chars();
    string[..number_end].parse::<f32>().ok()
}

fn get_point(chars: &mut Chars) -> Option<Point2> {
    Some(Point2::new(get_f32(chars)?, get_f32(chars)?))
}

fn get_scaled_point(chars: &mut Chars, factor: f32) -> Option<Point2> {
    Some(Point2::new(
        get_f32(chars)? * factor,
        get_f32(chars)? * factor,
    ))
}

fn process_drawing_commands(text: &str, scale: u32) -> Option<Outline> {
    let mut outline = OutlineBuilder::new();
    // let mut tokens = text
    //     .split_ascii_whitespace()
    //     .map(|x| match x.parse::<i32>() {
    //         Ok(v) => DrawToken::Argument(x, v),
    //         Err(e) if *e.kind() == IntErrorKind::InvalidDigit => DrawToken::Command(x),
    //         Err(_) => DrawToken::OutOfRange(x),
    //     })
    //     .peekable();

    let scaling_factor = (1.0f32).powi(-(scale as i32 - 1));
    let mut chars = text.chars();
    let mut m_seen = false;
    let mut started = false;
    // If an outline has not been started yet, this is used as the initial point.
    let mut pen: Option<Point2> = None;
    loop {
        skip_space(&mut chars);
        let Some(cmd) = chars.next() else {
            break;
        };

        match cmd {
            'm' => {
                m_seen = true;
                let mut was_valid = false;
                while let Some(p) = get_scaled_point(&mut chars, scaling_factor) {
                    pen = Some(p);
                    was_valid = true;
                }
                if was_valid && started {
                    outline.add_segment(CurveDegree::Linear);
                    outline.close_contour();
                    started = false;
                }
            }
            'n' => {
                if pen.is_none() {
                    let Some(p) = get_scaled_point(&mut chars, scaling_factor) else {
                        continue;
                    };
                    if !m_seen {
                        break;
                    }
                    pen = Some(p);
                }
                while let Some(p) = get_scaled_point(&mut chars, scaling_factor) {
                    pen = Some(p);
                }
            }
            _ => (),
        }
    }

    if pen.is_none() {
        None
    } else {
        Some(outline.build())
    }
}

pub const fn convert_ass_color(abgr: u32) -> BGRA8 {
    BGRA8::from_argb32(
        ((abgr & 0xFF) << 16)
            | (abgr & 0xFF00)
            | ((abgr & 0xFF0000) >> 16)
            | (0xFF000000 - (abgr & 0xFF000000)),
    )
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

        let mut drawing_scale: u32 = 0;

        for token in tokens {
            use ParsedTextPart::*;

            match token {
                Text(content) => {
                    if drawing_scale != 0 {
                        if let Some(outline) = process_drawing_commands(content, drawing_scale) {
                            segments.push(Segment::Shape(ShapeSegment::new(
                                outline,
                                current_style.outline,
                                convert_ass_color(current_style.outline_colour),
                                convert_ass_color(current_style.primary_colour),
                            )))
                        }
                    } else {
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
                            color: convert_ass_color(current_style.primary_colour),
                            text,
                        }))
                    }
                }
                Override(Command::An(a) | Command::A(a)) => alignment = a,
                Override(Command::Pos(nx, ny)) => {
                    let (max_x, max_y) = layout_resolution;
                    x = nx as f32 / max_x as f32;
                    y = ny as f32 / max_y as f32;
                }
                Override(Command::R(style)) => {
                    current_style = ass.get_style(style).unwrap_or(&DEFAULT_STYLE).clone();
                }
                Override(Command::P(scale)) => {
                    drawing_scale = scale;
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
