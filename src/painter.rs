use std::ops::DerefMut;

use crate::{
    math::*,
    outline::{self, Outline},
    rasterize, text,
    util::{hsl_to_rgb, rgb_to_hsl},
};

pub trait PainterBuffer: AsRef<[u8]> + AsMut<[u8]> {}

impl PainterBuffer for [u8] {}
impl PainterBuffer for Vec<u8> {}
impl<T: PainterBuffer + ?Sized> PainterBuffer for &mut T {}

pub trait ResizablePainterBuffer: PainterBuffer {
    fn resize(&mut self, size: usize);
}

impl ResizablePainterBuffer for Vec<u8> {
    fn resize(&mut self, size: usize) {
        Vec::resize(self, size, 0)
    }
}

impl<T: ResizablePainterBuffer + ?Sized> ResizablePainterBuffer for &mut T {
    fn resize(&mut self, size: usize) {
        T::resize(*self, size)
    }
}

pub struct Painter<B: PainterBuffer> {
    buffer: B,
    width: u32,
    height: u32,
}

impl Painter<Vec<u8>> {
    pub fn new_vec(width: u32, height: u32) -> Self {
        Self::new(width, height, vec![0; width as usize * height as usize * 4])
    }
}

impl<B: PainterBuffer> Painter<B> {
    pub fn new(width: u32, height: u32, buffer: B) -> Self {
        Self {
            buffer,
            width,
            height,
        }
    }

    #[inline(always)]
    pub fn height(&self) -> u32 {
        self.height
    }

    #[inline(always)]
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn buffer(&self) -> &[u8] {
        self.buffer.as_ref()
    }

    pub fn buffer_mut(&mut self) -> &mut [u8] {
        self.buffer.as_mut()
    }

    pub fn clear(&mut self, color: u32) {
        let end = 4 * self.width as usize * self.height as usize;
        let mut current = 0;
        while current < end {
            let next = current + 4;
            *unsafe {
                TryInto::<&mut [u8; 4]>::try_into(&mut self.buffer.as_mut()[current..next])
                    .unwrap_unchecked()
            } = color.to_be_bytes();
            current = next;
        }
    }

    #[inline(always)]
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32
    }

    #[inline(always)]
    fn pixel(&mut self, x: u32, y: u32) -> &mut [u8; 4] {
        let start = ((y * self.width + x) * 4) as usize;
        assert!(x < self.width && y < self.height);
        (&mut self.buffer.as_mut()[start..start + 4])
            .try_into()
            .unwrap()
    }

    pub fn dot(&mut self, x: i32, y: i32, color: u32) {
        if self.in_bounds(x, y) {
            *self.pixel(x as u32, y as u32) = color.to_be_bytes();
        }
    }

    pub fn horizontal_line(&mut self, x: i32, y: i32, w: u32, color: u32) {
        rasterize::horizontal_line(
            y,
            x,
            x.saturating_add_unsigned(w),
            self.buffer.as_mut(),
            self.width,
            self.height,
            color,
        )
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
        rasterize::line(
            x0,
            y0,
            x1,
            y1,
            self.buffer.as_mut(),
            self.width,
            self.height,
            color,
        )
    }

    pub fn stroke_whrect(&mut self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        rasterize::stroke_polygon(
            [
                (x, y),
                (x.saturating_add_unsigned(w), y),
                (x.saturating_add_unsigned(w), y.saturating_add_unsigned(h)),
                (x, y.saturating_add_unsigned(h)),
            ],
            self.buffer.as_mut(),
            self.width,
            self.height,
            color,
        )
    }

    pub fn stroke_triangle(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
    ) {
        rasterize::stroke_triangle(
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            self.buffer.as_mut(),
            self.width,
            self.height,
            color,
        )
    }

    pub fn fill_triangle(
        &mut self,
        x0: i32,
        y0: i32,
        x1: i32,
        y1: i32,
        x2: i32,
        y2: i32,
        color: u32,
    ) {
        rasterize::fill_triangle(
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            self.buffer.as_mut(),
            self.width,
            self.height,
            color,
        )
    }

    pub fn stroke_outline(&mut self, x: i32, y: i32, outline: &outline::Outline, mut color: u32) {
        if outline.is_empty() {
            return;
        }

        // NOTE: For testing, make each outline segment have a rotated hue
        let [mut h, s, l] = rgb_to_hsl(
            (color >> 24) as u8,
            ((color >> 16) & 0xFF) as u8,
            ((color >> 8) & 0xFF) as u8,
        );

        let first_point = outline.points()[0];
        let mut last_point = (x + first_point.x as i32, y + first_point.y as i32);

        for segment in outline.segments().iter().copied() {
            let [r, g, b] = hsl_to_rgb(h, s, l);
            color = ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (color & 0xFF);

            if segment.degree() == outline::CurveDegree::Linear {
                let points = outline.points_for_segment(segment);
                let (x, y) = (x + points[1].x as i32, y + points[1].y as i32);
                self.line(last_point.0, last_point.1, x, y, color);
                last_point = (x, y)
            } else {
                dbg!(outline
                    .points_for_segment(segment)
                    .iter()
                    .map(|p| *p + Vec2::new(x as f32, y as f32))
                    .collect::<Vec<_>>());
                const SAMPLES: i32 = 20;
                for i in 1..=SAMPLES {
                    let t = i as f32 / SAMPLES as f32;
                    let p = outline.evaluate_segment(segment, t);
                    let (x, y) = (p.x as i32 + x, p.y as i32 + y);

                    self.line(last_point.0, last_point.1, x, y, color);
                    last_point = (x, y);
                }
            }

            h = (h + 0.05).fract();
        }
    }

    pub fn debug_stroke_outline(
        &mut self,
        x: i32,
        y: i32,
        outline: &Outline,
        color: u32,
        inverse_winding: bool,
    ) {
        if outline.is_empty() {
            return;
        }

        for segments in outline.iter_contours() {
            if segments.is_empty() {
                continue;
            }

            let mut polyline = Vec::new();
            for segment in segments.iter().copied() {
                polyline.clear();
                let segment_points = outline.points_for_segment(segment);
                polyline.push(segment_points[0]);
                outline.flatten_segment(segment, 0.01, &mut polyline);
                self.stroke_polyline(x, y, &polyline, color);
                let middle = outline.evaluate_segment(segment, 0.5);
                let start = segment_points[0];
                let end = *segment_points.last().unwrap();
                let diff = (end - start).normalize();
                let deriv = diff.normal();
                const ARROW_SCALE: f32 = 10.0;

                let f = if inverse_winding { -1.0 } else { 1.0 };
                let top = middle + diff * f * ARROW_SCALE;
                let left = middle - deriv * f * ARROW_SCALE;
                let right = middle + deriv * f * ARROW_SCALE;

                self.fill_triangle(
                    x + top.x as i32,
                    y + top.y as i32,
                    x + left.x as i32,
                    y + left.y as i32,
                    x + right.x as i32,
                    y + right.y as i32,
                    color,
                );
            }
        }
    }

    pub fn stroke_polyline(&mut self, x: i32, y: i32, points: &[Point2], color: u32) {
        let mut last = points[0];
        for point in &points[1..] {
            self.line(
                x + last.x as i32,
                y + last.y as i32,
                x + point.x as i32,
                y + point.y as i32,
                color,
            );
            last = *point;
        }
    }

    pub fn stroke_outline_polyline(
        &mut self,
        x: i32,
        y: i32,
        outline: &outline::Outline,
        color: u32,
    ) {
        if outline.is_empty() {
            return;
        }

        for segments in outline.iter_contours() {
            if segments.is_empty() {
                continue;
            }

            let polyline = outline.flatten_contour(segments);
            self.stroke_polyline(x, y, &polyline, color);
        }
    }

    pub fn bezier(&mut self, x: i32, y: i32, curve: &impl Bezier, color: u32) {
        let polyline = curve.flatten(0.01);
        self.stroke_polyline(x, y, &polyline, color);
    }

    pub fn math_line(&mut self, x: i32, y: i32, line: Line, color: u32) {
        if line.b == 0.0 {
            let vx = -line.c / line.a;
            let fx = x + vx as i32;
            self.line(fx, 0, fx, self.height as i32, color);
        } else {
            let y0 = line.sample_y(-x as f32);
            let y1 = line.sample_y(self.width as f32);
            self.line(
                0,
                y0 as i32 + y,
                x + self.width as i32,
                y1 as i32 + y,
                color,
            );
        }
    }

    pub fn text<'g>(
        &mut self,
        x: i32,
        y: i32,
        fonts: &[text::Font],
        text: &[text::Glyph],
        color: u32,
    ) -> (i32, i32) {
        text::paint(
            self.buffer.as_mut(),
            x,
            y,
            self.width as usize,
            self.height as usize,
            (self.width * 4) as usize,
            fonts,
            text,
            [
                ((color & 0xFF000000) >> 24) as u8,
                ((color & 0x00FF0000) >> 16) as u8,
                ((color & 0x0000FF00) >> 8) as u8,
            ],
            ((color & 0xFF) as f32) / 255.0,
        )
    }
}

impl<B: ResizablePainterBuffer> Painter<B> {
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.buffer.resize((width * height * 4) as usize);
    }
}

impl<B: PainterBuffer> Painter<B> {
    pub fn as_ref(&mut self) -> Painter<&mut B> {
        Painter {
            buffer: &mut self.buffer,
            width: self.width,
            height: self.height,
        }
    }

    pub fn map<Y: PainterBuffer>(self, mapper: impl FnOnce(B) -> Y) -> Painter<Y> {
        Painter {
            buffer: mapper(self.buffer),
            width: self.width,
            height: self.height,
        }
    }
}

impl<B: PainterBuffer + DerefMut> Painter<B>
where
    B::Target: PainterBuffer,
{
    pub fn as_deref(&mut self) -> Painter<&mut B::Target> {
        Painter {
            buffer: DerefMut::deref_mut(&mut self.buffer),
            width: self.width,
            height: self.height,
        }
    }
}
