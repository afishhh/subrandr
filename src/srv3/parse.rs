use std::{collections::HashMap, str::FromStr};

use quick_xml::{
    events::{attributes::Attributes, Event as XmlEvent},
    Error as XmlError,
};
use thiserror::Error;

use crate::{
    log::{log_once_state, warning, LogOnceSet},
    Subrandr,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeType {
    None = 0,
    HardShadow = 1,
    Bevel = 2,
    Glow = 3,
    SoftShadow = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RubyPart {
    None,
    Base,
    Parenthesis,
    Ruby(RubyTextPart),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RubyTextPart {
    pub position: RubyPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RubyPosition {
    #[default]
    Alternate,
    Over,
    Under,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pen {
    pub font_size: u16,
    pub font_style: u32,

    pub bold: bool,
    pub italic: bool,
    pub underline: bool,

    pub edge_type: EdgeType,
    pub edge_color: Option<u32>,

    pub ruby_part: RubyPart,

    pub foreground_color: u32,
    pub background_color: u32,
}

impl Pen {
    pub const DEFAULT: Self = Self {
        font_size: 100,
        font_style: 0,
        bold: false,
        italic: false,
        underline: false,
        edge_type: EdgeType::None,
        edge_color: None,
        ruby_part: RubyPart::None,
        foreground_color: 0xFFFFFFFF,
        // The default opacity is 0.75
        // round(0.75 * 255) = 0xBF
        background_color: 0x080808BF,
    };
}

impl Default for Pen {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Point {
    TopLeft = 0,
    TopCenter = 1,
    TopRight = 2,
    MiddleLeft = 3,
    MiddleCenter = 4,
    MiddleRight = 5,
    BottomLeft = 6,
    BottomCenter = 7,
    BottomRight = 8,
}

#[derive(Debug, Clone)]
pub struct WindowPos {
    pub point: Point,
    pub x: u32,
    pub y: u32,
}

const DEFAULT_WINDOW_POS: WindowPos = WindowPos {
    point: Point::BottomCenter,
    x: 50,
    y: 100,
};

impl Default for WindowPos {
    fn default() -> Self {
        DEFAULT_WINDOW_POS.clone()
    }
}

#[derive(Debug, Clone)]
pub struct WindowStyle {
    pub mode_hint: ModeHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeHint {
    // NOTE: ModeHint=0 is *not* the same as ModeHint=1 as some sources say.
    //       For example while experimenting with the `t` attribute on ruby I
    //       accidentally left `mh=0` and got different results.
    Default = 1,
    Scroll = 2,
}

impl FromStr for ModeHint {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "1" => Self::Default,
            "2" => Self::Scroll,
            _ => return Err("Unknown mode hint".into()),
        })
    }
}

const DEFAULT_WINDOW_STYLE: WindowStyle = WindowStyle {
    mode_hint: ModeHint::Default,
};

impl Default for WindowStyle {
    fn default() -> Self {
        DEFAULT_WINDOW_STYLE
    }
}

#[derive(Debug, Clone)]
pub struct Segment<'h> {
    pub pen: &'h Pen,
    pub time_offset: u32,
    pub text: String,
}

#[derive(Debug)]
pub struct Event<'h> {
    pub time: u32,
    pub duration: u32,
    pub position: &'h WindowPos,
    pub style: &'h WindowStyle,
    pub window_id: Option<Box<str>>,
    pub segments: Vec<Segment<'h>>,
}

#[derive(Debug)]
pub struct Window<'h> {
    pub time: u32,
    pub duration: u32,
    pub position: &'h WindowPos,
    pub style: &'h WindowStyle,
}

type AnyError = Box<dyn std::error::Error + Send + Sync>;

// TODO: Subrandr parsing errors are currently not very useful,
//       they could be improved significantly.

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    InvalidStructure(&'static str),
    #[error("'{0}' element is missing an '{1}' attribute")]
    MissingAttribute(&'static str, &'static str),
    #[error("Attribute {0} has an invalid value {1:?}: {2}")]
    InvalidAttributeValue(&'static str, String, AnyError),
    #[error(transparent)]
    InvalidXML(#[from] XmlError),
    #[error("Unexpected end-of-file")]
    UnexpectedEof,
}

macro_rules! match_attribute {
    ($attr: expr, $($key: literal($var: ident: $($type: tt)*) => $expr: expr,)+ else $other: pat => $else: expr $(,)?) => {
        match unsafe { std::str::from_utf8_unchecked($attr.key.0) } {
            $(
            $key => {
                let value = unsafe { std::str::from_utf8_unchecked(&$attr.value) };
                let $var: $($type)* = match_attribute!(@parse value, $($type)*, $key);
                $expr
            },
            )*
            $other => $else
        }
    };
    (@parse $value: ident, &str, $key: literal) => {
        $value
    };
    (@parse $value: ident, $type: ty, $key: literal) => {
        $value.parse().map_err(|e| Error::InvalidAttributeValue($key, $value.to_string(), Box::from(e)))?
    };
}

macro_rules! match_attributes {
    ($attrs: expr, $($tt: tt)*) => {
        let mut it = $attrs;
        while let Some(attr) = it.next().transpose().map_err(XmlError::from)? {
            match_attribute!(attr, $($tt)*)
        }
    };
}

#[repr(transparent)]
struct HexRGBColor(u32);

impl FromStr for HexRGBColor {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix("#")
            .ok_or("Hex color is missing a '#' prefix")?;

        if hex.len() != 6 {
            return Err("Hex color code does not consist of six characters".into());
        }

        Ok(Self(u32::from_str_radix(hex, 16)?))
    }
}

#[repr(transparent)]
struct Bool01(bool);

impl FromStr for Bool01 {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "0" => Self(false),
            "1" => Self(true),
            _ => return Err("Boolean values must be either '0' or '1'".into()),
        })
    }
}

impl FromStr for EdgeType {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "0" => Self::None,
            "1" => Self::HardShadow,
            "2" => Self::Bevel,
            "3" => Self::Glow,
            "4" => Self::SoftShadow,
            _ => return Err("Unknown edge type".into()),
        })
    }
}

impl FromStr for RubyPart {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "0" => Self::None,
            "1" => Self::Base,
            "2" => Self::Parenthesis,
            "3" => Self::Ruby(RubyTextPart::default()),
            "4" => Self::Ruby(RubyTextPart {
                position: RubyPosition::Over,
            }),
            "5" => Self::Ruby(RubyTextPart {
                position: RubyPosition::Under,
            }),
            _ => return Err("Unknown ruby part".into()),
        })
    }
}

impl FromStr for Point {
    type Err = AnyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "0" => Self::TopLeft,
            "1" => Self::TopCenter,
            "2" => Self::TopRight,
            "3" => Self::MiddleLeft,
            "4" => Self::MiddleCenter,
            "5" => Self::MiddleRight,
            "6" => Self::BottomLeft,
            "7" => Self::BottomCenter,
            "8" => Self::BottomRight,
            _ => return Err("Point number out of range".into()),
        })
    }
}

fn parse_pen(
    sbr: &Subrandr,
    attributes: Attributes,
    logset: &LogOnceSet,
) -> Result<(Box<str>, Pen), Error> {
    let mut result_id = None;
    let mut result = Pen::default();

    log_once_state!(in logset; unknown_pen_attribute);

    match_attributes! {
        attributes,
        "id"(id: &str) => {
            result_id = Some(id.into());
        },
        "fc"(color: HexRGBColor) => {
            result.foreground_color &= 0x000000FF;
            result.foreground_color |= color.0 << 8;
        },
        "fo"(opacity: u8) => {
            result.foreground_color &= 0xFFFFFF00;
            result.foreground_color |= opacity as u32;
        },
        "bc"(color: HexRGBColor) => {
            result.background_color &= 0x000000FF;
            result.background_color |= color.0 << 8;
        },
        "bo"(opacity: u8) => {
            result.background_color &= 0xFFFFFF00;
            result.background_color |= opacity as u32;
        },
        "ec"(color: HexRGBColor) => {
            result.edge_color = Some(color.0);
        },
        "sz"(size: u16) => {
            result.font_size = size;
        },
        "fs"(style: u32) => {
            result.font_style = style;
        },
        "et"(et: EdgeType) => {
            result.edge_type = et;
        },
        "i"(value: Bool01) => {
            result.italic = value.0;
        },
        "b"(value: Bool01) => {
            result.bold = value.0;
        },
        "u"(value: Bool01) => {
            result.underline = value.0;
        },
        "rb"(value: RubyPart) => {
            result.ruby_part = value;
        },
        else other => {
            warning!(
                sbr, once(unknown_pen_attribute, other),
                "Unknown attribute encountered on pen: {other}",
            );
        }
    }

    match result_id {
        // TODO: This should be a warning only
        None => Err(Error::MissingAttribute("pen", "id")),
        Some(id) => Ok((id, result)),
    }
}

fn parse_wp(
    sbr: &Subrandr,
    attributes: Attributes,
    logset: &LogOnceSet,
) -> Result<(Box<str>, WindowPos), Error> {
    let mut result_id = None;
    let mut result = WindowPos::default();

    log_once_state!(in logset; unknown_wp_attribute);

    match_attributes! {
        attributes,
        "id"(id: &str) => {
            result_id = Some(id.into());
        },
        "ap"(point: Point) => {
            result.point = point;
        },
        "ah"(x: u32) => {
            result.x = x;
        },
        "av"(y: u32) => {
            result.y = y;
        },
        else other => {
            warning!(
                sbr, once(unknown_wp_attribute, other),
                "Unknown attribute encountered on wp: {other}",
            );
        }
    }

    match result_id {
        None => Err(Error::MissingAttribute("wp", "id")),
        Some(id) => Ok((id, result)),
    }
}

fn parse_ws(
    sbr: &Subrandr,
    attributes: Attributes,
    logset: &LogOnceSet,
) -> Result<(Box<str>, WindowStyle), Error> {
    let mut result_id = None;
    let mut result = WindowStyle::default();

    log_once_state!(in logset; unknown_ws_attribute);

    match_attributes! {
        attributes,
        "id"(id: &str) => {
            result_id = Some(id.into());
        },
        "mh"(mh: ModeHint) => {
            result.mode_hint = mh;
        },
        else other => {
            warning!(
                sbr, once(unknown_ws_attribute, other),
                "Unknown attribute encountered on ws: {other}",
            );
        }
    }

    match result_id {
        None => Err(Error::MissingAttribute("ws", "id")),
        Some(id) => Ok((id, result)),
    }
}

#[derive(Debug)]
pub struct Head {
    pens: HashMap<Box<str>, Pen>,
    wps: HashMap<Box<str>, WindowPos>,
    wss: HashMap<Box<str>, WindowStyle>,
}

impl Head {
    fn empty() -> Self {
        Self {
            pens: HashMap::new(),
            wps: HashMap::new(),
            wss: HashMap::new(),
        }
    }
}

fn parse_head(
    sbr: &Subrandr,
    reader: &mut quick_xml::Reader<&[u8]>,
    head: &mut Head,
) -> Result<(), Error> {
    let logset = LogOnceSet::new();
    log_once_state!(in &logset; unknown_elements_head);

    let mut depth = 0;
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) if depth == 0 => {
                match element.local_name().into_inner() {
                    // TODO: Warn on text content in pen or wp
                    b"pen" => {
                        let (id, pen) = parse_pen(sbr, element.attributes(), &logset)?;
                        head.pens.insert(id, pen);
                    }
                    b"wp" => {
                        let (id, wp) = parse_wp(sbr, element.attributes(), &logset)?;
                        head.wps.insert(id, wp);
                    }
                    b"ws" => {
                        let (id, ws) = parse_ws(sbr, element.attributes(), &logset)?;
                        head.wss.insert(id, ws);
                    }
                    name => {
                        warning!(
                            sbr,
                            once(unknown_elements_head, name),
                            "Unknown element encountered in head: {}",
                            unsafe { std::str::from_utf8_unchecked(name) }
                        );
                    }
                }
                depth += 1;
            }
            XmlEvent::Start(_) => (),
            XmlEvent::End(_) if depth > 0 => depth -= 1,
            XmlEvent::End(_) => break,
            XmlEvent::Empty(_) => unreachable!(),
            XmlEvent::Text(x)
                if depth > 0 || x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => {}
            XmlEvent::Text(_) | XmlEvent::CData(_) => {
                return Err(Error::InvalidStructure(
                    "Unrecognized text content inside head element",
                ));
            }
            XmlEvent::Comment(_) | XmlEvent::Decl(_) | XmlEvent::PI(_) | XmlEvent::DocType(_) => (),
            XmlEvent::Eof => return Err(Error::UnexpectedEof),
        }
    }

    Ok(())
}

#[derive(Debug)]
pub enum BodyElement<'h> {
    Event(Event<'h>),
    Window(Box<str>, Window<'h>),
}

pub struct BodyParser<'rs> {
    logset: LogOnceSet,
    // NOTE: this could borrow these but that would result in a weird API (currently)
    head: Head,
    reader: quick_xml::Reader<&'rs [u8]>,
}

impl<'rs> BodyParser<'rs> {
    pub fn read_next(&mut self, sbr: &Subrandr) -> Result<Option<BodyElement<'_>>, Error> {
        log_once_state!(
            in &self.logset;
            unknown_attrs,
            unknown_body_elements,
            unknown_event_elements,
            unknown_segment_attrs,
            unknown_segment_elements,
            non_existant_pen,
            non_existant_wp,
            non_existant_ws,
            win_without_id
        );

        macro_rules! set_or_log {
            ($dst: expr, $map: expr, $id: expr, $log_id: expr, $what: literal) => {
                if let Some(value) = $map.get($id) {
                    $dst = value;
                } else {
                    warning!(
                        sbr,
                        once($log_id, $id),
                        concat!($what, " with ID {} does not exist but was referenced"),
                        $id
                    )
                }
            };
        }

        let mut current_event_pen = &Pen::DEFAULT;
        let mut current_segment_pen = current_event_pen;
        let mut current_segment_time_offset = 0;
        let mut current_text = String::new();
        let mut current = None;
        let mut depth = 0;
        loop {
            match self.reader.read_event()? {
                XmlEvent::Start(element) if depth == 0 => {
                    match element.local_name().into_inner() {
                        b"w" => {
                            let mut result_id = None;
                            let mut result = Window {
                                time: 0,
                                duration: u32::MAX,
                                position: &DEFAULT_WINDOW_POS,
                                style: &DEFAULT_WINDOW_STYLE,
                            };

                            match_attributes! {
                                element.attributes(),
                                "id"(id: &str) => {
                                    result_id = Some(id.into());
                                },
                                "t"(time: u32) => {
                                    result.time = time;
                                },
                                "d"(duration: u32) => {
                                    result.duration = duration;
                                },
                                "wp"(id: &str) => {
                                    set_or_log!(result.position, self.head.wps, id, non_existant_wp, "Window position");
                                },
                                "ws"(id: &str) => {
                                    set_or_log!(result.style, self.head.wss, id, non_existant_ws, "Window style");
                                },
                                else other => {
                                    warning!(
                                        sbr, once(unknown_attrs, other),
                                        "Unknown window attribute {other}"
                                    )
                                }
                            }

                            if let Some(id) = result_id {
                                current = Some(BodyElement::Window(id, result));
                            } else {
                                warning!(sbr, once(win_without_id), "Window missing id attribute");
                            }
                        }
                        b"p" => {
                            // time=0 and duration=0 are defaults YouTube uses
                            // duration=0 events should probably be stripped during conversion
                            // since they're effectively no-ops unless I'm missing some subtle behaviour
                            let mut result = Event {
                                time: 0,
                                duration: 0,
                                position: &DEFAULT_WINDOW_POS,
                                style: &DEFAULT_WINDOW_STYLE,
                                window_id: None,
                                segments: vec![],
                            };

                            current_event_pen = &Pen::DEFAULT;

                            match_attributes! {
                                element.attributes(),
                                "t"(time: u32) => {
                                    result.time = time;
                                },
                                "d"(duration: u32) => {
                                    result.duration = duration;
                                },
                                "p"(id: &str) => {
                                    set_or_log!(current_event_pen, self.head.pens, id, non_existant_pen, "Pen");
                                },
                                "wp"(id: &str) => {
                                    set_or_log!(result.position, self.head.wps, id, non_existant_wp, "Window position");
                                },
                                "ws"(id: &str) => {
                                    set_or_log!(result.style, self.head.wss, id, non_existant_ws, "Window style");
                                },
                                "w"(id: &str) => {
                                    result.window_id = Some(id.into());
                                },
                                else other => {
                                    warning!(
                                        sbr, once(unknown_attrs, other),
                                        "Unknown event attribute {other}"
                                    )
                                }
                            }

                            current = Some(BodyElement::Event(result));
                        }
                        name => {
                            warning!(
                                sbr,
                                once(unknown_body_elements, name),
                                "Unknown element encountered in body: {}",
                                unsafe { std::str::from_utf8_unchecked(name) }
                            );
                        }
                    }
                    depth += 1;
                }
                XmlEvent::Start(element) if depth == 1 => {
                    match element.local_name().into_inner() {
                        b"s" => {
                            if !current_text.is_empty() {
                                match &mut current {
                                    Some(BodyElement::Event(event)) => {
                                        event.segments.push(Segment {
                                            pen: current_event_pen,
                                            time_offset: current_segment_time_offset,
                                            text: std::mem::take(&mut current_text),
                                        });
                                    }
                                    _ => {
                                        unreachable!("non-empty text with empty or Window current")
                                    }
                                }
                            }

                            current_segment_pen = current_event_pen;
                            current_segment_time_offset = 0;

                            match_attributes! {
                                element.attributes(),
                                "p"(id: &str) => {
                                    set_or_log!(current_segment_pen, self.head.pens, id, non_existant_pen, "Pen");
                                },
                                "t"(time_offset: u32) => {
                                    current_segment_time_offset = time_offset;
                                },
                                else other => {
                                    warning!(
                                        sbr, once(unknown_segment_attrs, other),
                                        "Unknown segment attribute {other}"
                                    );
                                }
                            }
                        }
                        _ if current.is_some() => {
                            warning!(
                                sbr,
                                once(unknown_event_elements, element.local_name().into_inner()),
                                "Unknown element encountered in event: {}",
                                unsafe {
                                    std::str::from_utf8_unchecked(element.local_name().into_inner())
                                }
                            );
                        }
                        _ => (),
                    }
                    depth += 1;
                }
                XmlEvent::Start(element) => {
                    if current.is_some() {
                        warning!(
                            sbr,
                            once(unknown_segment_elements, element.local_name().into_inner()),
                            "Unknown element encountered in segment: {}",
                            unsafe {
                                std::str::from_utf8_unchecked(element.local_name().into_inner())
                            }
                        );
                    }
                    depth += 1;
                }
                XmlEvent::End(_) if current.is_some() && depth == 2 => {
                    if !current_text.is_empty() {
                        match &mut current {
                            Some(BodyElement::Event(event)) => {
                                event.segments.push(Segment {
                                    pen: current_segment_pen,
                                    time_offset: current_segment_time_offset,
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            _ => unreachable!("non-empty text with empty or Window current"),
                        }
                    }
                    depth = 1;
                }
                XmlEvent::End(_) if depth == 1 => {
                    if !current_text.is_empty() {
                        match &mut current {
                            Some(BodyElement::Event(event)) => {
                                event.segments.push(Segment {
                                    pen: current_event_pen,
                                    time_offset: current_segment_time_offset,
                                    text: std::mem::take(&mut current_text),
                                });
                            }
                            _ => unreachable!("non-empty text with empty or Window current"),
                        }
                    }

                    if let Some(element) = current.take() {
                        return Ok(Some(element));
                    }

                    depth = 0;
                }
                XmlEvent::End(_) if depth > 0 => {
                    depth -= 1;
                }
                XmlEvent::End(_) => return Ok(None),
                XmlEvent::Empty(_) => unreachable!(),
                XmlEvent::Text(x) if current.is_some() && (depth > 0 && depth <= 2) => {
                    current_text.push_str(&x.unescape()?);
                }
                XmlEvent::Text(x)
                    if depth > 0 || x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => {
                }
                XmlEvent::Text(_) | XmlEvent::CData(_) => {
                    return Err(Error::InvalidStructure(
                        "Unrecognized text content inside body element",
                    ));
                }
                XmlEvent::Comment(_)
                | XmlEvent::Decl(_)
                | XmlEvent::PI(_)
                | XmlEvent::DocType(_) => (),
                XmlEvent::Eof => return Err(Error::UnexpectedEof),
            }
        }
    }
}

pub fn probe(text: &str) -> bool {
    text.contains("<timedtext") && text.contains("format=\"3\"")
}

pub fn parse<'s>(sbr: &Subrandr, text: &'s str) -> Result<BodyParser<'s>, Error> {
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().check_comments = false;
    reader.config_mut().expand_empty_elements = true;

    log_once_state!(unknown_toplevel_elements);

    loop {
        match reader.read_event()? {
            XmlEvent::Start(root) => {
                if root.local_name().into_inner() == b"timedtext" {
                    let format = root
                        .attributes()
                        .find_map(|attr| {
                            attr.map(|x| {
                                if x.key.into_inner() == b"format" {
                                    Some(x.value)
                                } else {
                                    None
                                }
                            })
                            .transpose()
                        })
                        .transpose()
                        .map_err(XmlError::from)?;
                    match format {
                        Some(x) if &*x == b"3" => break,
                        Some(_) => {
                            return Err(Error::InvalidStructure(
                                "Root element specifies unsupported version",
                            ));
                        }
                        None => {
                            return Err(Error::InvalidStructure(
                                "Root element lacks \"format\" attribute",
                            ));
                        }
                    }
                } else {
                    return Err(Error::InvalidStructure("Root element is not timedtext"));
                }
            }
            XmlEvent::End(_) | XmlEvent::Empty(_) => unreachable!(),
            XmlEvent::Text(x) if x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => (),
            XmlEvent::Text(_) | XmlEvent::CData(_) => {
                return Err(Error::InvalidStructure(
                    "Encountered content outside of a root element",
                ))
            }
            XmlEvent::Comment(_) | XmlEvent::Decl(_) | XmlEvent::PI(_) | XmlEvent::DocType(_) => (),
            XmlEvent::Eof => return Err(Error::UnexpectedEof),
        }
    }

    let mut has_head = true;
    let mut depth = 0;
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) if depth == 0 => {
                match element.local_name().into_inner() {
                    b"head" => break,
                    b"body" => {
                        has_head = false;
                        break;
                    }
                    name => {
                        warning!(
                            sbr,
                            once(unknown_toplevel_elements, name),
                            "Non-head element encountered: {}",
                            unsafe { std::str::from_utf8_unchecked(name) }
                        );
                    }
                }
                depth += 1;
            }
            XmlEvent::Start(_) => (),
            XmlEvent::End(_) if depth > 0 => depth -= 1,
            XmlEvent::End(_) => {
                return Err(Error::InvalidStructure(
                    "Encountered EOF before a head element",
                ))
            }
            XmlEvent::Empty(_) => unreachable!(),
            XmlEvent::Text(x)
                if depth > 0 || x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => {}
            XmlEvent::Text(_) | XmlEvent::CData(_) => {
                return Err(Error::InvalidStructure(
                    "Encountered content outside of a head or body element",
                ));
            }
            XmlEvent::Comment(_) | XmlEvent::Decl(_) | XmlEvent::PI(_) | XmlEvent::DocType(_) => (),
            XmlEvent::Eof => return Err(Error::UnexpectedEof),
        }
    }

    let mut head = Head::empty();
    if has_head {
        parse_head(sbr, &mut reader, &mut head)?;
    }

    if has_head {
        let mut depth = 0;
        loop {
            match reader.read_event()? {
                XmlEvent::Start(element) if depth == 0 => {
                    match element.local_name().into_inner() {
                        b"head" => {
                            return Err(Error::InvalidStructure("Head element encountered twice"))
                        }
                        b"body" => break,
                        name => {
                            warning!(
                                sbr,
                                once(unknown_toplevel_elements, name),
                                "Non-body element encountered: {}",
                                unsafe { std::str::from_utf8_unchecked(name) }
                            );
                        }
                    }
                    depth += 1;
                }
                XmlEvent::Start(_) => (),
                XmlEvent::End(_) if depth > 0 => depth -= 1,
                XmlEvent::End(_) => {
                    return Err(Error::InvalidStructure(
                        "Encountered EOF before a body element",
                    ))
                }
                XmlEvent::Empty(_) => unreachable!(),
                XmlEvent::Text(x)
                    if depth > 0 || x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => {
                }
                XmlEvent::Text(_) | XmlEvent::CData(_) => {
                    return Err(Error::InvalidStructure(
                        "Encountered content outside of a head or body element",
                    ));
                }
                XmlEvent::Comment(_)
                | XmlEvent::Decl(_)
                | XmlEvent::PI(_)
                | XmlEvent::DocType(_) => (),
                XmlEvent::Eof => return Err(Error::UnexpectedEof),
            }
        }
    }

    Ok(BodyParser {
        logset: LogOnceSet::new(),
        head,
        reader,
    })
}
