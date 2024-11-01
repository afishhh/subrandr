// HACK: everything here is temporary and to-be-cleaned-up

use core::str;
use std::{collections::BTreeMap, str::FromStr, sync::LazyLock};

use thiserror::Error;

// trait SloppyParse {
//     const HUMAN_NAME: &'static str;
//
// }
//
// struct SloppyError {
//     what: String,
//     expected_type: &'static str,
//     got: &'static str
// }
//
// type Sloppy<T> = Result<T, SloppyError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Version {
    SsaV4, // v4.00
    Ass,   // v4.00+
    Ass2,  // v4.00++
}

macro_rules! parse_from_consts {
    ($type: ty, |$s: ident| $trans: expr, $error: literal { $($const: literal => $result: expr),* $(,)? }) => {
        impl FromStr for $type {
            type Err = String;

            fn from_str($s: &str) -> Result<$type, Self::Err> {
                Ok(match $trans {
                    $(
                        $const => $result,
                    )*
                    _ => {
                        return Err(format!(
                            concat!("expected ", $error, " but got {:?}"),
                            $s
                        ))
                    }
                })
            }
        }
    };
}

parse_from_consts!(Version, |s| s, r#"one of "v4.00", "v4.00+", or "v4.00++""# {
    "v4.00" => Self::SsaV4,
    "v4.00+" => Self::Ass,
    "v4.00++" => Self::Ass2,
});

struct YesNo(bool);

parse_from_consts!(YesNo, |s| s.to_ascii_lowercase().as_str(), r#"either of "yes" or "no""# {
    "yes" => Self(true),
    "no" => Self(false)
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YCbCrMatrix {
    Default, // Unset
    Unknown, // Invalid / unsupported
    None,
}

impl FromStr for YCbCrMatrix {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "None" => YCbCrMatrix::None,
            _ => YCbCrMatrix::Unknown,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapStyle {
    WrapEvenly,
    Wrap,
    None,
}

#[derive(Debug, Error)]
#[error("Invalid wrap style")]
pub struct WrapStyleParseError;

impl FromStr for WrapStyle {
    type Err = WrapStyleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "0" => WrapStyle::WrapEvenly,
            "1" => WrapStyle::Wrap,
            "2" => WrapStyle::None,
            _ => return Err(WrapStyleParseError),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Metadata {
    title: Option<String>,
    original_script: Option<String>,
    original_translation: Option<String>,
    original_editing: Option<String>,
    original_timing: Option<String>,
    script_updated_by: Option<String>,
    update_details: Option<String>,

    wildcard: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum BorderStyle {
    Outline,
    Box,
    // libass extension
    ShadowBox,
}

#[derive(Debug, Error)]
#[error("Invalid border style")]
pub struct BorderStyleParseError;

impl FromStr for BorderStyle {
    type Err = BorderStyleParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        println!("{s}");
        Ok(match s.trim() {
            "1" => BorderStyle::Outline,
            "3" => BorderStyle::Box,
            "4" => BorderStyle::ShadowBox,
            _ => {
                // TODO: warn
                BorderStyle::Outline
            }
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    BottomLeft,
    BottomCenter,
    BottomRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    TopLeft,
    TopCenter,
    TopRight,
}

impl Alignment {
    pub fn from_ass(value: &str) -> Option<Alignment> {
        Some(match value {
            "1" => Self::BottomLeft,
            "2" => Self::BottomCenter,
            "3" => Self::BottomRight,
            "4" => Self::MiddleLeft,
            "5" => Self::MiddleCenter,
            "6" => Self::MiddleRight,
            "7" => Self::TopLeft,
            "8" => Self::TopCenter,
            "9" => Self::TopRight,
            _ => return None,
        })
    }

    pub fn from_ssa(value: &str) -> Option<Alignment> {
        Some(match value {
            "1" => Self::BottomLeft,
            "2" => Self::BottomCenter,
            "3" => Self::BottomRight,
            "5" => Self::TopLeft,
            "6" => Self::TopCenter,
            "7" => Self::TopRight,
            "9" => Self::MiddleLeft,
            "10" => Self::MiddleCenter,
            "11" => Self::MiddleRight,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum RelativeTo {
    Player,
    Static,
}

#[derive(Debug, Clone)]
pub struct Style {
    pub name: String,
    pub fontname: String, // len <= 31
    pub fontsize: f32,    // <=511
    pub primary_colour: u32,
    pub secondary_colour: u32,
    pub outline_colour: u32,
    pub back_colour: u32,
    pub weight: u32,
    pub italic: bool,
    pub underline: bool,
    pub strike_out: bool,
    pub scale_x: f32,
    pub scale_y: f32,
    pub spacing: f32,
    pub angle: f32,
    pub border_style: BorderStyle,
    pub outline: f32,
    pub shadow: f32,
    pub alignment: Alignment,
    pub margin_left: i32,
    pub margin_right: i32,
    pub margin_top: i32,
    pub margin_bottom: i32,
    pub relative_to: RelativeTo,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            name: "Default".to_string(),
            fontname: "Arial".to_string(),
            fontsize: 18.0,
            primary_colour: 0xffffff00,
            secondary_colour: 0x00ffff00,
            outline_colour: 0x00000000,
            back_colour: 0x00000080,
            weight: 400,
            italic: false,
            underline: false,
            strike_out: false,
            scale_x: 1.0,
            scale_y: 1.0,
            spacing: 0.0,
            angle: 0.0,
            border_style: BorderStyle::Outline,
            outline: 2.0,
            shadow: 3.0,
            alignment: Alignment::BottomCenter,
            margin_left: 20,
            margin_right: 20,
            margin_top: 20,
            margin_bottom: 20,
            relative_to: RelativeTo::Player,
        }
    }
}

pub static DEFAULT_STYLE: LazyLock<Style> = LazyLock::new(Style::default);

const fn style_section_name(version: Version) -> &'static str {
    match version {
        Version::SsaV4 => "V4 Styles",
        Version::Ass => "V4+ Styles",
        Version::Ass2 => "V4++ Styles",
    }
}

pub(super) fn parse_ass_color(value: &str) -> u32 {
    let value = value.trim().strip_prefix("&H").unwrap();
    let value = value.strip_suffix('&').unwrap_or(value);
    u32::from_str_radix(value, 16).unwrap()
}

fn parse_ass_timestamp(value: &str) -> u32 {
    let (h, value) = value.split_once(':').unwrap();
    let (m, value) = value.split_once(':').unwrap();
    let (s, dd) = value.split_once('.').unwrap();
    let r = h.parse::<u32>().unwrap() * 60 * 60 * 1000
        + m.parse::<u32>().unwrap() * 60 * 1000
        + s.parse::<u32>().unwrap() * 1000
        + dd.parse::<u32>().unwrap() * 10;
    println!("{h}h {m}min {s}s {dd}ds = {r}ms");
    r
}

fn parse_style_line(version: Version, line: &str) -> Style {
    macro_rules! build {
        (generic {$($generic: tt)*}, v4++ {$($v4pp: tt)*}, v4+ {$($v4p: tt)*}, v4 {$($v4: tt)*}) => {
            match version {
                Version::Ass2 => {
                    todo!()
                    // Style { $($generic)* $($v4pp)* }
                }
                Version::Ass => {
                    Style { $($generic)* $($v4p)* }
                }
                Version::SsaV4 => {
                    todo!()
                    // Style { $($generic)* $($v4)* }
                }
            }
        };
    }

    let line = line.strip_prefix("Style: ").unwrap();

    let mut values = [""; 32];
    let mut found = 0;
    for string in line.splitn(23, ',') {
        values[found] = string;
        found += 1;
    }

    if found != 23 {
        panic!()
    }

    assert_eq!(values[22], "1");

    build! {
        generic {
            name: values[0].to_string(),
            fontname: values[1].to_string(),
            fontsize: values[2].parse().unwrap(),
            primary_colour: parse_ass_color(values[3]),
            secondary_colour: parse_ass_color(values[4]),
            outline_colour: parse_ass_color(values[5]),
            back_colour: parse_ass_color(values[6]),
            weight: if values[7] == "-1" { 700 } else { 400 },
            italic: values[8] == "-1",
            underline: values[9] == "-1",
            strike_out: values[10] == "-1",
            scale_x: values[11].parse().unwrap(),
            scale_y: values[12].parse().unwrap(),
            spacing: values[13].parse().unwrap(),
            angle: values[14].parse().unwrap(),
            border_style: values[15].parse().unwrap(),
            outline: values[16].parse().unwrap(),
            shadow: values[17].parse().unwrap(),
            margin_left: values[19].parse().unwrap(),
            margin_right: values[20].parse().unwrap(),
        },
        v4++ {
        },
        v4+ {
            alignment: Alignment::from_ass(values[18]).unwrap(),
            margin_top: values[21].parse().unwrap(),
            margin_bottom: values[21].parse().unwrap(),
            relative_to: RelativeTo::Player
        },
        v4 {
            alignment: Alignment::from_ssa(values[18]).unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub layer: u32,
    pub start: u32,
    pub end: u32,
    pub style: String,
    pub name: String,
    pub margin_left: f32,
    pub margin_right: f32,
    pub margin_top: f32,
    pub margin_bottom: f32,
    pub effect: String,
    pub text: String,
}

fn parse_event_line(version: Version, line: &str) -> Event {
    macro_rules! build {
        (generic {$($generic: tt)*}, v4++ {$($v4pp: tt)*}, v4+ {$($v4p: tt)*}, v4 {$($v4: tt)*}) => {
            match version {
                Version::Ass2 => {
                    todo!()
                    // Style { $($generic)* $($v4pp)* }
                }
                Version::Ass => {
                    Event { $($generic)* $($v4p)* }
                }
                Version::SsaV4 => {
                    todo!()
                    // Style { $($generic)* $($v4)* }
                }
            }
        };
    }

    let line = line.strip_prefix("Dialogue: ").unwrap();

    let mut values = [""; 32];
    let mut found = 0;
    for string in line.splitn(10, ',') {
        values[found] = string;
        found += 1;
    }

    if found != 10 {
        panic!()
    }

    build! {
        generic {
            layer: values[0].parse().unwrap(),
            start: parse_ass_timestamp(values[1]),
            end: parse_ass_timestamp(values[2]),
            style: values[3].to_string(),
            name: values[4].to_string(),
            margin_left: values[5].parse().unwrap(),
            margin_right: values[6].parse().unwrap(),
        },
        v4++ {
        },
        v4+ {
            margin_top: values[7].parse().unwrap(),
            margin_bottom: values[7].parse().unwrap(),
            effect: values[8].to_string(),
            text: values[9].to_string(),
        },
        v4 {
            alignment: Alignment::from_ssa(values[18]).unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Script {
    pub version: Version,

    pub scaled_border_and_shadow: bool,
    pub ycbcr_matrix: YCbCrMatrix,
    pub layout_resolution: (u32, u32),
    pub play_resolution: (u32, u32),
    pub wrap_style: WrapStyle,

    pub metadata: Metadata,

    pub styles: Vec<Style>,
    pub events: Vec<Event>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("file does not start with a section header")]
    NoSection,
    #[error("section {0} appears twice within the file")]
    DuplicateSection(String),
    #[error("header {1} appears twice within {0}")]
    DuplicateHeader(String, String),
    #[error("required secction {0} is missing")]
    MissingSection(String),
    #[error("malformed script info header")]
    InvalidHeaderSyntax,
    #[error("unsupported script type {0:?}")]
    UnsupportedScriptType(String),
    #[error("header {0} has invalid value {1:?}: {2}")]
    InvalidHeaderValue(String, String, String),
    #[error("required header {1} is missing from {0}")]
    MissingHeader(String, String),
    #[error("unexpected end of file")]
    UnexpectedEOF,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn split_into_sections(text: &str) -> Result<BTreeMap<&str, &str>, Error> {
    let mut lines = text
        .lines()
        .map(|x| x.trim())
        .filter(|x| !x.is_empty() && !x.starts_with(';'));

    let mut section_headers: Vec<&str> = vec![];

    let line = lines.next().ok_or(Error::UnexpectedEOF)?;
    section_headers.push(
        line.strip_prefix('[')
            .and_then(|x| x.strip_suffix(']'))
            .ok_or(Error::NoSection)?,
    );

    for line in lines {
        if let Some(name) = line.strip_prefix('[').and_then(|x| x.strip_suffix(']')) {
            section_headers.push(name);
        }
    }

    let mut result = BTreeMap::new();
    let mut it = section_headers.into_iter().peekable();
    while let Some(name) = it.next() {
        let start = unsafe { name[name.len()..].as_ptr().add(1) };
        let end = it
            .peek()
            .map(|x| unsafe { x.as_ptr().sub(1) })
            .unwrap_or(text[text.len()..].as_ptr());

        let content = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                start,
                end.offset_from(start) as usize,
            ))
            .trim()
        };

        if result.insert(name, content).is_some() {
            return Err(Error::DuplicateSection(name.to_string()));
        }
    }

    Ok(result)
}

pub fn parse(text: &str) -> Result<Script, Error> {
    let sections = split_into_sections(text)?;

    let script_info = *sections
        .get("Script Info")
        .ok_or_else(|| Error::MissingSection(text.to_string()))?;

    let mut freeform_headers = vec![];
    let mut headers = BTreeMap::new();
    for line in script_info.lines() {
        if line.starts_with(';') {
            continue;
        }

        let Some((name, value)) = line.split_once(':') else {
            return Err(Error::InvalidHeaderSyntax);
        };

        if name == "!" {
            freeform_headers.push(value.to_string());
        } else if headers.insert(name, value.trim()).is_some() {
            return Err(Error::DuplicateHeader(name.to_string(), name.to_string()));
        }
    }

    macro_rules! get_header {
        ($name: literal) => {
            headers
                .get($name)
                .ok_or_else(|| Error::MissingHeader("Script Info".to_string(), $name.to_string()))?
        };
    }

    macro_rules! parse_header_value {
        ($type: ty, $name: literal) => {{
            let value = get_header!($name);
            value.parse::<$type>().map_err(|x| {
                Error::InvalidHeaderValue($name.to_string(), value.to_string(), x.to_string())
            })?
        }};
        (#optional $type: ty, $name: literal) => {{
            headers
                .get($name)
                .map(|text| {
                    text.parse::<$type>().map_err(|x| {
                        Error::InvalidHeaderValue(
                            $name.to_string(),
                            text.to_string(),
                            x.to_string(),
                        )
                    })
                })
                .transpose()?
        }};
    }

    let version = parse_header_value!(Version, "ScriptType");

    Ok(Script {
        version,
        scaled_border_and_shadow: parse_header_value!(YesNo, "ScaledBorderAndShadow").0,
        ycbcr_matrix: parse_header_value!(#optional YCbCrMatrix, "YCbCr Matrix")
            .unwrap_or(YCbCrMatrix::Default),
        layout_resolution: (
            parse_header_value!(#optional u32, "LayoutResX").unwrap_or(0),
            parse_header_value!(#optional u32, "LayoutResY").unwrap_or(0),
        ),
        play_resolution: (
            parse_header_value!(u32, "PlayResX"),
            parse_header_value!(u32, "PlayResY"),
        ),
        wrap_style: parse_header_value!(#optional WrapStyle, "WrapStyle")
            .unwrap_or(WrapStyle::WrapEvenly),
        metadata: Metadata {
            title: parse_header_value!(#optional String, "Title"),
            original_script: parse_header_value!(#optional String, "Original Script"),
            original_translation: parse_header_value!(#optional String, "Original Translation"),
            original_editing: parse_header_value!(#optional String, "Original Editing"),
            original_timing: parse_header_value!(#optional String, "Original Timing"),
            script_updated_by: parse_header_value!(#optional String, "Script Updated By"),
            update_details: parse_header_value!(#optional String, "Update Details"),
            wildcard: freeform_headers,
        },

        styles: {
            let name = style_section_name(version);
            let section = sections
                .get(name)
                .ok_or_else(|| Error::MissingSection(name.to_string()))?;

            let mut styles = vec![];
            for line in section.lines().skip(1) {
                styles.push(parse_style_line(version, line));
            }

            styles.sort_by(|a, b| a.name.cmp(&b.name));

            styles
        },

        events: {
            let section = sections
                .get("Events")
                .ok_or_else(|| Error::MissingSection("Events".to_string()))?;

            let mut events = vec![];
            for line in section.lines().skip(1) {
                if line.starts_with("Comment:") {
                    continue;
                }

                events.push(parse_event_line(version, line));
            }

            events
        },
    })
    // let version = None;
    //
    //                 if version.is_some() {
    //                     return Err(Error::DuplicateSection("Script Info".to_string()));
    //                 }
    //
    //                 while let Some(line) = lines.next().transpose()? {
    //                     let Some((name, value)) = line.split_once(':') else {
    //                         return Err(Error::InvalidHeaderSyntax);
    //                     };
    //
    //                     match name {
    //                         "ScriptType" => {
    //                             version = Some(match value {
    //                                 "v4.00" => Version::SsaV4,
    //                                 "v4.00+" => Version::Ass,
    //                                 "v4.00++" => Version::Ass2,
    //                                 other => {
    //                                     return Err(Error::UnsupportedScriptType(value.to_string()))
    //                                 }
    //                             });
    //                         }
    //                     }
}

impl Script {
    pub fn get_style(&self, name: &str) -> Option<&Style> {
        self.styles
            .binary_search_by(|x| x.name.as_str().cmp(name))
            .map(|idx| &self.styles[idx])
            .ok()
    }
}
