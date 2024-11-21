use std::ops::Rem;

use crate::{
    outline, rasterize, text,
    util::{hsl_to_rgb, rgb_to_hsl},
};

pub trait PainterBuffer: AsRef<[u8]> + AsMut<[u8]> {}

impl PainterBuffer for [u8] {}
impl PainterBuffer for Vec<u8> {}
impl<'a, T: PainterBuffer> PainterBuffer for &'a mut T {}

pub trait ResizablePainterBuffer: PainterBuffer {
    fn resize(&mut self, size: usize);
}

impl ResizablePainterBuffer for Vec<u8> {
    fn resize(&mut self, size: usize) {
        Vec::resize(self, size, 0)
    }
}

impl<'a, T: ResizablePainterBuffer> ResizablePainterBuffer for &'a mut T {
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

            if segment.degree() == outline::SplineDegree::Linear {
                let points = outline.points_for_segment(segment);
                let (x, y) = (x + points[1].x as i32, y + points[1].y as i32);
                self.line(last_point.0, last_point.1, x, y, color);
                last_point = (x, y)
            } else {
                dbg!(outline
                    .points_for_segment(segment)
                    .iter()
                    .map(|p| *p + crate::util::math::Vec2::new(x as f32, y as f32))
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

    pub fn paint_text<'g>(
        &mut self,
        x: i32,
        y: i32,
        font: &text::Font,
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
            font,
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
