//! Some basic [PANOSE font classification system](https://monotype.github.io/panose/pan1.htm) types
//! that are useful for extracting style information from fonts that have it.
//!
//! Only the values that are actually useful for us are defined here.

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum Classification {
    Any,
    NoFit,
    LatinText(LatinText),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct LatinText {
    pub serif_style: SerifStyle,
    pub weight: Weight,
    pub proportion: Proportion,
}

impl LatinText {
    fn parse(number: [u8; 10]) -> Option<LatinText> {
        Some(LatinText {
            serif_style: SerifStyle::from_value(number[1])?,
            weight: Weight::from_value(number[2])?,
            proportion: Proportion::from_value(number[3])?,
        })
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[expect(dead_code)] // transmute means these are "never constructed"
pub enum SerifStyle {
    Any = 0,
    NoFit,
    Cove,
    ObtuseCove,
    SquareCove,
    ObtuseSquareCove,
    Square,
    Thin,
    Oval,
    Exaggerated,
    Triangle,
    NormalSans,
    ObtuseSans,
    PerpendicularSans,
    Flared,
    Rounded,
}

impl SerifStyle {
    fn from_value(value: u8) -> Option<SerifStyle> {
        if value > Self::Rounded as u8 {
            return None;
        }

        unsafe { std::mem::transmute(value) }
    }

    pub fn is_sans_serif(self) -> bool {
        matches!(
            self,
            Self::Any
                | Self::Flared
                | Self::Rounded
                | Self::PerpendicularSans
                | Self::ObtuseSans
                | Self::NormalSans
        )
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[expect(dead_code)] // transmute means these are "never constructed"
pub enum Weight {
    Any = 0,
    NoFit,
    VeryLight,
    Light,
    Thin,
    Book,
    Medium,
    Demi,
    Bold,
    Heavy,
    Black,
    ExtraBlack,
}

impl Weight {
    fn from_value(value: u8) -> Option<Weight> {
        if value > Self::ExtraBlack as u8 {
            return None;
        }

        unsafe { std::mem::transmute(value) }
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
#[expect(dead_code)] // transmute means these are "never constructed"
pub enum Proportion {
    Any = 0,
    NoFit,
    OldStyle,
    Modern,
    EvenWidth,
    Extended,
    Condensed,
    VeryExtended,
    VeryCondensed,
    Monospaced,
}

impl Proportion {
    fn from_value(value: u8) -> Option<Proportion> {
        if value > Self::Monospaced as u8 {
            return None;
        }

        unsafe { std::mem::transmute(value) }
    }
}

impl Classification {
    pub fn parse(number: [u8; 10]) -> Option<Classification> {
        match number[0] {
            0 => Some(Classification::Any),
            1 => Some(Classification::NoFit),
            2 => LatinText::parse(number).map(Classification::LatinText),
            _ => None,
        }
    }
}
