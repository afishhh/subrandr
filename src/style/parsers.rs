use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
};

use crate::{css::value::*, style::computed::Length};

impl Length {
    // only absolute lengths
    fn try_parse<'a>(
        stream: &'a ValueParseStream<'a>,
        mut lk: Lookahead<'a>,
    ) -> Result<Self, ParseError> {
        // TODO: accept 0
        Ok(if lk.peek::<Dimension>() {
            let dim = stream.parse::<Dimension>()?;
            if dim.unit().eq_ignore_ascii_case("cm") {
                Length::from_pixels(I26Dot6::from_f64(dim.value() * const { 96.0 / 2.54 }))
            } else if dim.unit().eq_ignore_ascii_case("mm") {
                Length::from_pixels(I26Dot6::from_f64(
                    dim.value() * const { (96.0 / 2.54) / 10.0 },
                ))
            } else if dim.unit().eq_ignore_ascii_case("Q") {
                Length::from_pixels(I26Dot6::from_f64(
                    dim.value() * const { (96.0 / 2.54) / 40.0 },
                ))
            } else if dim.unit().eq_ignore_ascii_case("in") {
                Length::from_pixels(I26Dot6::from_f64(dim.value() * 96.0))
            } else if dim.unit().eq_ignore_ascii_case("pc") {
                Length::from_pixels(I26Dot6::from_f64(dim.value() * const { 96.0 / 6.0 }))
            } else if dim.unit().eq_ignore_ascii_case("pt") {
                Length::from_pixels(I26Dot6::from_f64(dim.value() * const { 96.0 / 72.0 }))
            } else if dim.unit().eq_ignore_ascii_case("px") {
                Length::from_pixels(I26Dot6::from_f64(dim.value()))
            } else {
                todo!();
            }
        } else {
            return Err(lk.error());
        })
    }
}

pub(super) struct FontFamily(pub Rc<[Rc<str>]>);

impl FontFamily {
    fn parse<'a>(
        stream: &'a ValueParseStream<'a>,
        mut lk: Lookahead<'a>,
    ) -> Result<Self, ParseError> {
        let mut result: Vec<util::rc::Rc<str>> = Vec::new();
        loop {
            let mut current = String::new();
            let mut first = true;

            if !result.is_empty() && !current.is_empty() && lk.peek::<End>() {
                result.push(current.as_str().into());
                return Ok(Self(result.into()));
            } else if !current.is_empty() && lk.peek::<Comma>() {
                result.push(current.as_str().into());
                current.clear();
                first = true;
            } else {
                if !first {
                    current.push(' ');
                }
                first = false;

                if lk.peek::<Ident>() {
                    current.extend(stream.parse::<&Ident>()?.value().unescape_iter());
                } else if lk.peek::<StringLit>() {
                    current.extend(stream.parse::<&StringLit>()?.value().unescape_iter());
                } else {
                    return Err(lk.error());
                }
            }

            lk = stream.lookahead1();
        }
    }
}

pub(super) struct FontWeight(pub I16Dot16);

impl FontWeight {
    const NORMAL: I16Dot16 = I16Dot16::new(400);
    const BOLD: I16Dot16 = I16Dot16::new(700);

    // `bolder` and `lighter` relative keywords not supported
    fn parse<'a>(
        stream: &'a ValueParseStream<'a>,
        mut lk: Lookahead<'a>,
    ) -> Result<Self, ParseError> {
        Ok(if lk.peek_keyword("normal") {
            stream.skip();
            Self(Self::NORMAL)
        } else if lk.peek_keyword("bold") {
            stream.skip();
            Self(Self::BOLD)
        } else if lk.peek::<Number>() {
            Self(I16Dot16::from_f64(stream.parse::<Number>()?.value()))
        } else {
            return Err(lk.error());
        })
    }
}

pub(super) struct FontSize(pub I26Dot6);

impl FontSize {
    fn try_parse<'a>(
        stream: &'a ValueParseStream<'a>,
        lk: Lookahead<'a>,
    ) -> Result<Self, ParseError> {
        Length::try_parse(stream, lk)
            .map(Length::to_unscaled_pixels)
            .map(FontSize)
    }
}
