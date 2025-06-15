use rasterize::color::BGRA8;
use util::math::{I26Dot6, Vec2f};

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

#[derive(Debug, Clone)]
pub struct TextShadow {
    pub offset: Vec2f,
    pub blur_radius: I26Dot6,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy)]
pub struct TextDecorations {
    // TODO: f32 for size
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
