use std::{collections::HashMap, str::FromStr};

use aliasable::boxed::AliasableBox;
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
    None = 0,
    Base = 1,
    Parenthesis = 2,
    Over = 4,
    Under = 5,
}

#[derive(Debug, Clone, Copy)]
pub struct Pen {
    id: i32,

    pub font_size: u16,
    pub font_style: u32,

    pub bold: bool,
    pub italic: bool,

    pub edge_type: EdgeType,
    pub edge_color: u32,

    pub ruby_part: RubyPart,

    pub foreground_color: u32,
    pub background_color: u32,
}

const DEFAULT_PEN: Pen = Pen {
    id: -1,
    font_size: 100,
    font_style: 0,
    bold: false,
    italic: false,
    edge_type: EdgeType::None,
    edge_color: 0x020202,
    ruby_part: RubyPart::None,
    foreground_color: 0xFFFFFFFF,
    // The default opacity is 0.75
    // round(0.75 * 255) = 0xBF
    background_color: 0x080808BF,
};

impl Default for Pen {
    fn default() -> Self {
        DEFAULT_PEN
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

// NOTE: This is not complete there's additional properties here
//       like cc - column count and rc - row count that I've never seen in the wild.
//       I wonder if anyone ever used them???
//
//       I think one of the effects of rc and cc is that they change sizing so that
//       a min width is computed that's equal to "-" * cc. Wild stuff.
#[derive(Debug, Clone)]
pub struct WindowPos {
    id: i32,
    pub point: Point,
    pub x: u32,
    pub y: u32,
}

// TODO: Find correct values
const DEFAULT_WINDOW_POS: WindowPos = WindowPos {
    id: -1,
    point: Point::BottomCenter,
    x: 50,
    y: 100,
};

impl Default for WindowPos {
    fn default() -> Self {
        DEFAULT_WINDOW_POS.clone()
    }
}

// POV: you want to self reference but Rust says "no"
#[derive(Debug)]
pub struct Document {
    pens: AliasableBox<[Pen]>,
    wps: AliasableBox<[WindowPos]>,
    events: Vec<Event>,
}

impl Document {
    pub fn pens(&self) -> &[Pen] {
        &self.pens
    }

    pub fn wps(&self) -> &[WindowPos] {
        &self.wps
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pen: &'static Pen,
    pub text: String,
}

impl Segment {
    pub const fn pen(&self) -> &Pen {
        self.pen
    }
}
#[derive(Debug, Clone)]
pub struct Event {
    pub time: u32,
    pub duration: u32,
    position: &'static WindowPos,
    pub segments: Vec<Segment>,
}

impl Event {
    pub const fn position(&self) -> &WindowPos {
        self.position
    }
}

type AnyError = Box<dyn std::error::Error + Send + Sync>;

// TODO: Subrandr parsing errors are currently not very useful,
//       they could be improved significantly.

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    InvalidStructure(&'static str),
    #[error("There exist two '{0}' elements with the same 'id' of '{1}'")]
    DuplicateId(&'static str, i32),
    #[error("'{0}' element is missing an '{1}' attribute")]
    MissingAttribute(&'static str, &'static str),
    #[error("Attribute {0} has an invalid value {1:?}: {2}")]
    InvalidAttributeValue(&'static str, String, AnyError),
    #[error(transparent)]
    InvalidXML(#[from] XmlError),
}

macro_rules! match_attribute {
    ($attr: expr, $($key: literal($var: ident: $type: ty) $(if ($cond: expr) $error: literal)? => $expr: expr,)+ else $other: pat => $else: expr $(,)?) => {
        match unsafe { std::str::from_utf8_unchecked($attr.key.0) } {
            $(
            $key => {
                let value = unsafe { std::str::from_utf8_unchecked(&$attr.value) };
                let $var: $type = value.parse().map_err(|e| Error::InvalidAttributeValue($key, value.to_string(), Box::from(e)))?;
                $(if !$cond {
                    return Err(Error::InvalidAttributeValue($key, value.to_string(), $error.into()));
                })?
                $expr
            },
            )*
            $other => $else
        }
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
            "4" => Self::Over,
            "5" => Self::Under,
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

fn parse_pen(sbr: &Subrandr, attributes: Attributes, logset: &LogOnceSet) -> Result<Pen, Error> {
    let mut result = Pen::default();

    log_once_state!(in logset; unknown_pen_attribute);

    match_attributes! {
        attributes,
        "id"(id: i32) if (id >= 0) "pen ID must be greater than zero" => {
            result.id = id;
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
            result.edge_color = color.0;
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
        "rb"(value: RubyPart) => {
            result.ruby_part = value;
        },
        else other => {
            warning!(
                sbr, once(unknown_pen_attribute, other),
                "Unknown attribute encountered on pen: {}",
                other,
            );
        }
    }

    if result.id < 0 {
        return Err(Error::MissingAttribute("pen", "id"));
    }

    Ok(result)
}

fn parse_wp(
    sbr: &Subrandr,
    attributes: Attributes,
    logset: &LogOnceSet,
) -> Result<WindowPos, Error> {
    let mut result = WindowPos::default();

    log_once_state!(in logset; unknown_wp_attribute);

    match_attributes! {
        attributes,
        "id"(id: i32) if (id >= 0) "wp ID must be greater than zero" => {
            result.id = id;
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
                "Unknown attribute encountered on wp: {}",
                other,
            );
        }
    }

    if result.id < 0 {
        return Err(Error::MissingAttribute("wp", "id"));
    }

    Ok(result)
}

fn parse_head(
    sbr: &Subrandr,
    reader: &mut quick_xml::Reader<&[u8]>,
) -> Result<(Vec<Pen>, Vec<WindowPos>), Error> {
    let (mut pens, mut wps) = (vec![], vec![]);

    let logset = LogOnceSet::new();
    log_once_state!(in &logset; unknown_elements_head);

    let mut depth = 0;
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) if depth == 0 => {
                match element.local_name().into_inner() {
                    // TODO: Warn on text content in pen or wp
                    b"pen" => {
                        pens.push(parse_pen(sbr, element.attributes(), &logset)?);
                    }
                    b"wp" => {
                        wps.push(parse_wp(sbr, element.attributes(), &logset)?);
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
            XmlEvent::Eof => unreachable!(),
        }
    }

    Ok((pens, wps))
}

fn parse_body(
    sbr: &Subrandr,
    pens: &HashMap<i32, &'static Pen>,
    wps: &HashMap<i32, &'static WindowPos>,
    reader: &mut quick_xml::Reader<&[u8]>,
) -> Result<Vec<Event>, Error> {
    let mut events = vec![];

    log_once_state!(
        unknown_attrs,
        unknown_body_elements,
        unknown_event_elements,
        unknown_segment_attrs,
        unknown_segment_elements,
        non_existant_pen,
        non_existant_wp
    );

    macro_rules! set_or_log {
        ($dst: expr, $map: expr, $id: expr, $log_id: expr, $what: literal) => {
            if let Some(value) = $map.get(&$id) {
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

    let mut current_event_pen = &DEFAULT_PEN;
    let mut current_segment_pen = current_event_pen;
    let mut current_text = String::new();
    let mut current = None;
    let mut depth = 0;
    loop {
        match reader.read_event()? {
            XmlEvent::Start(element) if depth == 0 => {
                match element.local_name().into_inner() {
                    b"p" => {
                        // time=0 and duration=0 are defaults YouTube uses
                        // duration=0 events should probably be stripped during conversion
                        // since they're effectively noops unless I'm missing some subtle behaviour
                        let mut result = Event {
                            time: 0,
                            duration: 0,
                            position: &DEFAULT_WINDOW_POS,
                            segments: vec![],
                        };

                        current_event_pen = &DEFAULT_PEN;

                        match_attributes! {
                            element.attributes(),
                            "t"(time: u32) => {
                                result.time = time;
                            },
                            "d"(duration: u32) => {
                                result.duration = duration;
                            },
                            "p"(id: i32) if (id >= 0) "pen ID must be greater than zero" => {
                                set_or_log!(current_event_pen, pens, id, non_existant_pen, "Pen");
                            },
                            "wp"(id: i32) if (id >= 0) "wp ID must be greater than zero" => {
                                set_or_log!(result.position, wps, id, non_existant_wp, "Window position");
                            },
                            else other => {
                                warning!(
                                    sbr, once(unknown_attrs, other),
                                    "Unknown event attribute {other}"
                                )
                            }
                        }

                        current = Some(result);
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
                            current.as_mut().unwrap().segments.push(Segment {
                                pen: current_event_pen,
                                text: std::mem::take(&mut current_text),
                            });
                        }

                        current_segment_pen = current_event_pen;

                        match_attributes! {
                            element.attributes(),
                            "p"(id: i32) if (id >= 0) "pen ID must be greater than zero" => {
                                set_or_log!(current_segment_pen, pens, id, non_existant_pen, "Pen");
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
                        unsafe { std::str::from_utf8_unchecked(element.local_name().into_inner()) }
                    );
                }
                depth += 1;
            }
            XmlEvent::End(_) if current.is_some() && depth == 2 => {
                if !current_text.is_empty() {
                    current.as_mut().unwrap().segments.push(Segment {
                        pen: current_segment_pen,
                        text: std::mem::take(&mut current_text),
                    });
                }
                depth = 1;
            }
            XmlEvent::End(_) if depth == 1 => {
                if !current_text.is_empty() {
                    current.as_mut().unwrap().segments.push(Segment {
                        pen: current_event_pen,
                        text: std::mem::take(&mut current_text),
                    });
                }

                if let Some(event) = current.take() {
                    if !event.segments.is_empty() {
                        events.push(event);
                    }
                }

                depth = 0;
            }
            XmlEvent::End(_) if depth > 0 => {
                depth -= 1;
            }
            XmlEvent::End(_) => break,
            XmlEvent::Empty(_) => unreachable!(),
            XmlEvent::Text(x) if current.is_some() && (depth > 0 && depth <= 2) => {
                current_text.push_str(&x.unescape()?);
            }
            XmlEvent::Text(x)
                if depth > 0 || x.borrow().into_inner().iter().all(u8::is_ascii_whitespace) => {}
            XmlEvent::Text(_) | XmlEvent::CData(_) => {
                return Err(Error::InvalidStructure(
                    "Unrecognized text content inside body element",
                ));
            }
            XmlEvent::Comment(_) | XmlEvent::Decl(_) | XmlEvent::PI(_) | XmlEvent::DocType(_) => (),
            XmlEvent::Eof => unreachable!(),
        }
    }

    Ok(events)
}

pub fn probe(text: &str) -> bool {
    text.contains("<timedtext") && text.contains("format=\"3\"")
}

pub fn parse(sbr: &Subrandr, text: &str) -> Result<Document, Error> {
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
            XmlEvent::Eof => {
                return Err(Error::InvalidStructure(
                    "Encountered EOF before a root element",
                ))
            }
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
            XmlEvent::Eof => unreachable!(),
        }
    }

    let (pens, wps) = if has_head {
        parse_head(sbr, &mut reader)?
    } else {
        (vec![], vec![])
    };

    let mut doc = Document {
        pens: pens.into_boxed_slice().into(),
        wps: wps.into_boxed_slice().into(),
        events: { vec![] },
    };

    let pens: &'static [Pen] = unsafe { std::mem::transmute(&*doc.pens) };

    let mut pens_by_id = HashMap::new();
    for pen in pens {
        if pens_by_id.insert(pen.id, pen).is_some() {
            return Err(Error::DuplicateId("pen", pen.id));
        }
    }

    let wps: &'static [WindowPos] = unsafe { std::mem::transmute(&*doc.wps) };

    let mut wps_by_id = HashMap::new();
    for wp in wps {
        if wps_by_id.insert(wp.id, wp).is_some() {
            return Err(Error::DuplicateId("wp", wp.id));
        }
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
                XmlEvent::Eof => unreachable!(),
            }
        }
    }

    doc.events = parse_body(sbr, &pens_by_id, &wps_by_id, &mut reader)?;

    Ok(doc)
}
