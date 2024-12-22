use std::{convert::Infallible, ops::Range, str::FromStr};

use super::{parse_ass_color, Alignment};

fn find_first_unespaced(text: &str, chr: char, escape: char) -> Option<usize> {
    let mut it = text.char_indices();

    while let Some((i, c)) = it.next() {
        if c == escape {
            it.next();
        } else if c == chr {
            return Some(i);
        }
    }

    None
}

#[derive(Debug, Clone)]
pub enum TextPart {
    Commands,
    Content,
}

pub fn segment_event_text(text: &str) -> Vec<(TextPart, Range<usize>)> {
    let mut result = vec![];

    let mut current = 0;

    loop {
        if let Some((start, end)) =
            find_first_unespaced(&text[current..], '{', '\\').and_then(|start| {
                let start = start + current;
                find_first_unespaced(&text[start..], '}', '\\')
                    .map(|closing| (start, start + closing))
            })
        {
            if start != current {
                result.push((TextPart::Content, current..start));
            }
            result.push((TextPart::Commands, start + 1..end));
            current = end + 1;
        } else {
            if current < text.len() {
                result.push((TextPart::Content, current..text.len()))
            }

            break;
        }
    }

    result
}

fn take_and_split_override_args(text: &str) -> (Vec<&str>, usize) {
    if text.starts_with('(') {
        let end = text.find(')').unwrap_or(text.len());
        (text[1..end].split(',').collect(), end + 1)
    } else {
        let end = text.find('\\').unwrap_or(text.len());
        (vec![&text[..end]], end)
    }
}

trait OverrideParse<'a> {
    type Output;
    type Error;

    fn parse(text: &'a str) -> Result<Self::Output, Self::Error>;
}

impl<T: FromStr> OverrideParse<'_> for T {
    type Output = T;
    type Error = <T as FromStr>::Err;

    fn parse(text: &str) -> Result<Self::Output, Self::Error> {
        T::from_str(text)
    }
}

// for tags with consistent parsing rules, i.e. not \fn and \r
macro_rules! parse_generic {
    ($name: literal $kind: ident $(, $arg: ident: $argt: ty)*) => {
        OverrideTagParser {
            name: $name,
            parse: &|text| {
                let (args, end) = take_and_split_override_args(text);
                let [$($arg),*] = &args[..] else {
                    return (Command::MismatchedArgumentCount($name, &text[..end]), end)
                };

            #[allow(irrefutable_let_patterns)]
            $(let Ok($arg) = <$argt as OverrideParse>::parse($arg) else {
                return (Command::InvalidArgument {
                    tag: $name,
                    arg: stringify!($arg),
                    value_type: stringify!($argt),
                    got: $arg
                }, end);
            };)*

                (Command::$kind($($arg),*), end)
            },
        }
    };
}

struct AssAlignmentParser;
impl OverrideParse<'_> for AssAlignmentParser {
    type Output = Alignment;
    type Error = ();

    fn parse(text: &str) -> Result<Self::Output, Self::Error> {
        Alignment::from_ass(text).ok_or(())
    }
}

struct SsaAlignmentParser;
impl OverrideParse<'_> for SsaAlignmentParser {
    type Output = Alignment;
    type Error = ();

    fn parse(text: &str) -> Result<Self::Output, Self::Error> {
        Alignment::from_ssa(text).ok_or(())
    }
}

struct OverrideBool01;
impl OverrideParse<'_> for OverrideBool01 {
    type Output = bool;
    type Error = ();

    fn parse(text: &str) -> Result<Self::Output, Self::Error> {
        match text {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => Err(()),
        }
    }
}

struct OverrideTagParser {
    name: &'static str,
    parse: &'static dyn Fn(&str) -> (Command, usize),
}

struct OverrideColor;
impl OverrideParse<'_> for OverrideColor {
    type Output = u32;
    type Error = ();

    fn parse(text: &str) -> Result<Self::Output, Self::Error> {
        Ok(parse_ass_color(text))
    }
}

impl<'a> OverrideParse<'a> for str {
    type Output = &'a str;
    type Error = Infallible;

    fn parse(text: &'a str) -> Result<Self::Output, Self::Error> {
        Ok(text)
    }
}

#[derive(Debug, Clone)]
pub enum Command<'a> {
    Pos(u32, u32),
    An(Alignment),
    A(Alignment),
    P(u32),
    I(bool),
    B(bool),
    U(bool),
    S(bool),
    Bord(f32),
    XBord(f32),
    YBord(f32),
    Shad(f32),
    XShad(f32),
    YShad(f32),
    Be(u32),
    Blur(f32),
    Fn(&'a str),
    Fs(u32),
    Fscx(f32),
    Fscy(f32),
    Fsp(f32),
    Frx(f32),
    Fry(f32),
    Frz(f32),
    Fax(f32),
    Fay(f32),
    R(&'a str),
    MismatchedArgumentCount(&'a str, &'a str),
    InvalidArgument {
        tag: &'static str,
        arg: &'static str,
        value_type: &'static str,
        got: &'a str,
    },
}

// NOTE: These should be ordered from longest to shorted (I think)
const OVERRIDE_TAGS: &[OverrideTagParser] = &[
    parse_generic!("xbord" XBord, value: f32),
    parse_generic!("ybord" YBord, value: f32),
    parse_generic!("bord" Bord, value: f32),
    parse_generic!("xshad" XShad, value: f32),
    parse_generic!("yshad" YShad, value: f32),
    parse_generic!("shad" Shad, value: f32),
    parse_generic!("blur" Blur, value: f32),
    parse_generic!("fscx" Fscx, value: f32),
    parse_generic!("fscy" Fscy, value: f32),
    parse_generic!("pos" Pos, x: u32, y: u32),
    parse_generic!("fsp" Fsp, value: f32),
    parse_generic!("frx" Frx, value: f32),
    parse_generic!("fry" Fry, value: f32),
    parse_generic!("frz" Frz, value: f32),
    parse_generic!("fax" Fax, value: f32),
    parse_generic!("fay" Fay, value: f32),
    parse_generic!("fn" Fn, value: str),
    parse_generic!("fs" Fs, value: u32),
    parse_generic!("fr" Frz, value: f32),
    parse_generic!("an" An, x: AssAlignmentParser),
    parse_generic!("be" Be, value: u32),
    parse_generic!("p" P, scale: u32),
    parse_generic!("a" A, x: SsaAlignmentParser),
    parse_generic!("i" I, value: OverrideBool01),
    parse_generic!("b" B, value: OverrideBool01),
    parse_generic!("u" U, value: OverrideBool01),
    parse_generic!("s" S, value: OverrideBool01),
    parse_generic!("r" R, value: str),
];

#[derive(Debug, Clone)]
pub enum ParsedTextPart<'a> {
    Text(&'a str),
    Override(Command<'a>),
}

pub fn parse_command_block<'a>(mut content: &'a str, mut push: impl FnMut(Command<'a>)) {
    while let Some(command_start) = content.find('\\') {
        content = &content[command_start + 1..];
        for tag in OVERRIDE_TAGS {
            if let Some(rest) = content.strip_prefix(tag.name) {
                let (command, skip) = (tag.parse)(rest);
                push(command);
                content = &rest[skip..];
            }
        }
    }
}

pub fn parse_event_text(line: &str) -> Vec<ParsedTextPart<'_>> {
    let parts = segment_event_text(line);
    let mut result = vec![];

    for (part, range) in parts {
        let text = &line[range];

        match part {
            TextPart::Commands => {
                parse_command_block(text, |c| result.push(ParsedTextPart::Override(c)));
            }
            TextPart::Content => result.push(ParsedTextPart::Text(text)),
        }
    }

    result
}
