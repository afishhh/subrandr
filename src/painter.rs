use crate::{
    color::{BGRA8Slice, BGRA8},
    math::*,
    outline::{self, Outline},
    rasterize, text,
    util::{calculate_blit_rectangle, hsl_to_rgb, rgb_to_hsl, BlitRectangle},
};

pub trait AsBGRA8Buffer {
    fn as_ref(&self) -> &[BGRA8];
    fn as_mut(&mut self) -> &mut [BGRA8];
}

macro_rules! impl_painter_buffer {
    ($type: ty, $slice: ty) => {
        impl AsBGRA8Buffer for $type {
            fn as_ref(&self) -> &[BGRA8] {
                unsafe { std::mem::transmute(AsRef::<$slice>::as_ref(self)) }
            }

            fn as_mut(&mut self) -> &mut [BGRA8] {
                unsafe { std::mem::transmute(AsMut::<$slice>::as_mut(self)) }
            }
        }
    };
}

impl_painter_buffer!([BGRA8], [BGRA8]);
impl_painter_buffer!([u32], [u32]);

pub struct Painter<'a> {
    buffer: &'a mut [BGRA8],
    width: u32,
    height: u32,
}

impl<'a> Painter<'a> {
    pub fn new(width: u32, height: u32, buffer: &'a mut (impl AsBGRA8Buffer + ?Sized)) -> Self {
        assert!(buffer.as_ref().len() >= width as usize * height as usize);
        Self {
            buffer: buffer.as_mut(),
            width,
            height,
        }
    }

    #[inline(always)]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[inline(always)]
    pub const fn width(&self) -> u32 {
        self.width
    }

    pub fn buffer(&self) -> &[BGRA8] {
        self.buffer
    }

    pub fn buffer_mut(&mut self) -> &mut [BGRA8] {
        self.buffer
    }

    pub fn buffer_bytes(&self) -> &[u8] {
        self.buffer.as_bytes()
    }

    pub fn buffer_bytes_mut(&mut self) -> &mut [u8] {
        self.buffer.as_bytes_mut()
    }

    pub fn clear(&mut self, color: BGRA8) {
        self.buffer.fill(color);
    }

    #[inline(always)]
    pub const fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32
    }

    #[inline(always)]
    fn pixel(&mut self, x: u32, y: u32) -> &mut BGRA8 {
        let start = (y * self.width + x) as usize;
        assert!(x < self.width && y < self.height);
        &mut self.buffer[start]
    }

    pub fn dot(&mut self, x: i32, y: i32, color: BGRA8) {
        if self.in_bounds(x, y) {
            *self.pixel(x as u32, y as u32) = color;
        }
    }

    pub fn horizontal_line(&mut self, y: i32, x1: i32, x2: i32, color: BGRA8) {
        rasterize::horizontal_line(y, x1, x2, self.buffer, self.width, self.height, color)
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: BGRA8) {
        rasterize::line(x0, y0, x1, y1, self.buffer, self.width, self.height, color)
    }

    pub fn stroke_whrect(&mut self, x: i32, y: i32, w: u32, h: u32, color: BGRA8) {
        rasterize::stroke_polygon(
            [
                (x, y),
                (x.saturating_add_unsigned(w), y),
                (x.saturating_add_unsigned(w), y.saturating_add_unsigned(h)),
                (x, y.saturating_add_unsigned(h)),
            ],
            self.buffer,
            self.width,
            self.height,
            color,
        )
    }

    pub fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, color: BGRA8) {
        rasterize::fill_axis_aligned_rect(
            x0,
            y0,
            x1,
            y1,
            self.buffer,
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
        color: BGRA8,
    ) {
        rasterize::stroke_triangle(
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            self.buffer,
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
        color: BGRA8,
    ) {
        rasterize::fill_triangle(
            x0,
            y0,
            x1,
            y1,
            x2,
            y2,
            self.buffer,
            self.width as usize,
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

            if segment.degree() == outline::SegmentDegree::Linear {
                let points = outline.points_for_segment(segment);
                let (x, y) = (x + points[1].x as i32, y + points[1].y as i32);
                self.line(last_point.0, last_point.1, x, y, BGRA8::from_rgba32(color));
                last_point = (x, y)
            } else {
                dbg!(outline
                    .points_for_segment(segment)
                    .iter()
                    .map(|p| *p + Vec2f::new(x as f32, y as f32))
                    .collect::<Vec<_>>());
                const SAMPLES: i32 = 20;
                for i in 1..=SAMPLES {
                    let t = i as f32 / SAMPLES as f32;
                    let p = outline.evaluate_segment(segment, t);
                    let (x, y) = (p.x as i32 + x, p.y as i32 + y);

                    self.line(last_point.0, last_point.1, x, y, BGRA8::from_rgba32(color));
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
        color: BGRA8,
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

    pub fn stroke_polyline(&mut self, x: i32, y: i32, points: &[Point2f], color: BGRA8) {
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
        color: BGRA8,
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

    pub fn bezier(&mut self, x: i32, y: i32, curve: &impl Bezier, color: BGRA8) {
        let polyline = curve.flatten(0.01);
        self.stroke_polyline(x, y, &polyline, color);
    }

    pub fn math_line(&mut self, x: i32, y: i32, line: Line, color: BGRA8) {
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

    pub fn text(
        &mut self,
        x: i32,
        y: i32,
        fonts: &[text::Font],
        glyphs: &[text::Glyph],
        color: BGRA8,
    ) {
        self.blit_text_image(
            x,
            y,
            &text::render(I32Fixed::ZERO, I32Fixed::ZERO, fonts, glyphs),
            color,
        );
    }

    #[inline(always)]
    fn blit_rectangle_with(
        &self,
        x: i32,
        y: i32,
        other_width: u32,
        other_height: u32,
    ) -> Option<BlitRectangle> {
        calculate_blit_rectangle(
            x,
            y,
            self.width as usize,
            self.height as usize,
            other_width as usize,
            other_height as usize,
        )
    }

    pub fn blit_text_image(&mut self, x: i32, y: i32, image: &text::Image, color: BGRA8) {
        image.blit(
            x,
            y,
            self.buffer,
            self.width,
            self.width,
            self.height,
            color,
        )
    }

    pub fn blit_monochrome(
        &mut self,
        x: i32,
        y: i32,
        buffer: &[u8],
        width: u32,
        height: u32,
        color: BGRA8,
    ) {
        let Some(BlitRectangle { xs, ys }) = self.blit_rectangle_with(x, y, width, height) else {
            return;
        };

        for sy in ys {
            for sx in xs.clone() {
                let si = sy * width as usize + sx;
                let di = (y + sy as i32) as usize * self.width as usize + (x + sx as i32) as usize;
                self.buffer[di] = color.mul_alpha(buffer[si]).blend_over(self.buffer[di]).0;
            }
        }
    }

    pub fn blit_monochrome_text(
        &mut self,
        x: i32,
        y: i32,
        text: &text::MonochromeImage,
        color: BGRA8,
    ) {
        self.blit_monochrome(
            x + text.offset.0,
            y + text.offset.1,
            &text.data,
            text.width,
            text.height,
            color,
        );
    }

    // TODO: A Bitmap<> type would be really useful, not sure what the design should be though.
    pub fn blit_blurred_monochrome(
        &mut self,
        sigma: f32,
        x: i32,
        y: i32,
        buffer: &[u8],
        width: u32,
        height: u32,
        color: [u8; 3],
    ) {
        rasterize::monochrome_gaussian_blit(
            sigma,
            x,
            y,
            self.buffer,
            self.width as usize,
            self.height as usize,
            buffer,
            width as usize,
            height as usize,
            color,
        );
    }

    pub fn blit_blurred_monochrome_text(
        &mut self,
        sigma: f32,
        x: i32,
        y: i32,
        text: &text::MonochromeImage,
        color: [u8; 3],
    ) {
        self.blit_blurred_monochrome(
            sigma,
            x + text.offset.0,
            y + text.offset.1,
            &text.data,
            text.width,
            text.height,
            color,
        );
    }
}
