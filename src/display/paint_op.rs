use rasterize::color::BGRA8;
use util::math::{I16Dot16, I26Dot6, Point2};

use crate::{
    layout::{
        inline::{GlyphString, TextFragment},
        Point2L, Rect2L,
    },
    text::FontArena,
};

#[derive(Debug, Clone)]
pub struct Drawing {
    pub nodes: Vec<DrawingNode>,
}

#[derive(Debug, Clone)]
pub enum DrawingNode {
    StrokedPolyline(StrokedPolyline),
}

#[derive(Debug, Clone)]
pub struct StrokedPolyline {
    pub polyline: Vec<Point2<I16Dot16>>,
    pub width: I16Dot16,
    pub color: BGRA8,
}

#[derive(Debug, Clone)]
pub enum PaintOp {
    Text(Text),
    Drawing(PositionedDrawing),
    Rect(RectFill),
}

#[derive(Debug, Clone)]
pub struct Text {
    pub pos: Point2L,
    glyphs: GlyphString<'static>,
    _font_arena: util::rc::Rc<FontArena>,
    pub kind: TextKind,
}

impl Text {
    pub fn from_fragment(pos: Point2L, fragment: &TextFragment, kind: TextKind) -> Self {
        let (glyphs, font_arena) = unsafe { fragment.glyphs_and_font_arena() };
        Self {
            pos,
            glyphs: glyphs.clone(),
            _font_arena: font_arena.clone(),
            kind,
        }
    }

    pub fn glyphs(&self) -> &GlyphString<'_> {
        &self.glyphs
    }
}

#[derive(Debug, Clone, Copy)]
pub enum TextKind {
    Normal { mono_color: BGRA8 },
    Shadow { blur_stddev: I26Dot6, color: BGRA8 },
}

#[derive(Debug, Clone)]
pub struct PositionedDrawing {
    pub pos: Point2L,
    pub drawing: Drawing,
}

#[derive(Debug, Clone, Copy)]
pub struct RectFill {
    pub rect: Rect2L,
    pub color: BGRA8,
}

pub struct PaintOpBuilder<'b>(pub &'b mut Vec<PaintOp>);

impl PaintOpBuilder<'_> {
    pub fn push_text(&mut self, text: Text) {
        self.0.push(PaintOp::Text(text));
    }

    pub fn push_rect_fill(&mut self, rect: Rect2L, color: BGRA8) {
        self.0.push(PaintOp::Rect(RectFill { rect, color }))
    }

    pub fn push_drawing(&mut self, pos: Point2L, drawing: Drawing) {
        self.0
            .push(PaintOp::Drawing(PositionedDrawing { pos, drawing }))
    }
}
