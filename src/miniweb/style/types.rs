use crate::{color::BGRA8, math::I26Dot6, miniweb::layout::Vec2L};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Alignment(pub HorizontalAlignment, pub VerticalAlignment);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlignment {
    Top,
    Center,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

#[derive(Default, Debug, Clone, Copy)]
pub enum FontSlant {
    #[default]
    Regular,
    Italic,
}

#[derive(Debug, Clone, Copy)]
pub enum Ruby {
    None,
    Base,
    Over,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextShadow {
    pub offset: Vec2L,
    pub blur_radius: I26Dot6,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextDecorations {
    pub underline: bool,
    pub underline_color: BGRA8,
    pub strike_out: bool,
    pub strike_out_color: BGRA8,
}

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub color: BGRA8,
    pub font_size: I26Dot6,
    pub background_color: BGRA8,
    pub decorations: TextDecorations,
    pub shadows: Vec<TextShadow>,
}

impl TextDecorations {
    pub const fn none() -> Self {
        Self {
            underline: false,
            underline_color: BGRA8::ZERO,
            strike_out: false,
            strike_out_color: BGRA8::ZERO,
        }
    }
}

impl Default for TextDecorations {
    fn default() -> Self {
        Self::none()
    }
}

#[derive(Default, Debug, Clone, Copy)]
pub enum OutsideDisplayType {
    Block,
    #[default]
    Inline,
}

#[derive(Default, Debug, Clone, Copy)]
pub enum InsideDisplayType {
    #[default]
    Flow,
    FlowRoot,
    Ruby,
}

#[derive(Debug, Clone, Copy)]
// clippy shaming me for not supporting tables
#[allow(clippy::enum_variant_names)]
pub enum InternalDisplay {
    RubyBase,
    RubyText,
    RubyBaseContainer,
    RubyTextContainer,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct FullDisplay {
    pub outer: OutsideDisplayType,
    pub inner: InsideDisplayType,
}

#[derive(Debug, Clone, Copy)]
pub enum Display {
    None,
    Full(FullDisplay),
    Internal(InternalDisplay),
}

impl Default for Display {
    fn default() -> Self {
        Self::Full(FullDisplay::default())
    }
}

impl Display {
    pub const NONE: Display = Display::None;
    pub const BLOCK: Display = Display::Full(FullDisplay {
        outer: OutsideDisplayType::Block,
        inner: InsideDisplayType::Flow,
    });
    pub const INLINE: Display = Display::Full(FullDisplay {
        outer: OutsideDisplayType::Inline,
        inner: InsideDisplayType::Flow,
    });
    pub const INLINE_BLOCK: Display = Display::Full(FullDisplay {
        outer: OutsideDisplayType::Inline,
        inner: InsideDisplayType::FlowRoot,
    });
    pub const RUBY: Display = Display::Full(FullDisplay {
        outer: OutsideDisplayType::Inline,
        inner: InsideDisplayType::Ruby,
    });
}
