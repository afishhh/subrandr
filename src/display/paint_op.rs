use std::rc::Rc;

use rasterize::color::BGRA8;
use util::math::{I16Dot16, I26Dot6, Point2};

use crate::layout::{inline::GlyphString, Point2L, Rect2L};

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
pub enum PaintOp<'d> {
    Text(Text<'d>),
    Drawing(PositionedDrawing),
    Rect(RectFill),
}

#[derive(Debug, Clone)]
pub struct Text<'d> {
    pub pos: Point2L,
    pub glyphs: GlyphString<'d, Rc<str>>,
    pub kind: TextKind,
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

pub struct PaintOpBuilder<'b, 'p>(pub &'b mut Vec<PaintOp<'p>>);

impl<'p> PaintOpBuilder<'_, 'p> {
    pub fn push_text(&mut self, text: Text<'p>) {
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
