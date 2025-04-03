use std::{ops::Range, sync::Arc};

use crate::{
    color::{Premultiplied, BGRA8},
    math::{I32Fixed, Point2, Vec2},
    rasterize::bitmap::PixelFormat,
    util::{calculate_blit_rectangle, BlitRectangle},
};

mod blur;
pub use blur::*;

use super::{
    bitmap::{Bitmap, BitmapCast, Dynamic},
    RenderTarget,
};

#[derive(Debug, Clone)]
pub struct CpuTextureRenderHandle(Arc<Bitmap<BGRA8>>);

#[derive(Debug, Clone)]
struct Bresenham {
    dx: i32,
    dy: i32,
    // Either xi or yi
    i: i32,
    d: i32,

    x: i32,
    y: i32,
    x1: i32,
    y1: i32,
}

#[derive(Debug, Clone, Copy)]
enum BresenhamKind {
    Low,
    High,
}

impl Bresenham {
    #[inline(always)]
    pub const fn current(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub const fn new_low(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let dx = x1 - x0;
        let mut dy = y1 - y0;
        let mut yi = 1;

        if dy < 0 {
            yi = -1;
            dy = -dy;
        }

        let d = 2 * dy - dx;
        let y = y0;

        Self {
            dx,
            dy,
            i: yi,
            d,
            x: x0,
            y,
            x1,
            y1,
        }
    }

    #[inline(always)]
    pub const fn is_done_low(&self) -> bool {
        self.x > self.x1
    }

    pub const fn advance_low(&mut self) -> bool {
        if self.d > 0 {
            self.y += self.i;
            self.d -= 2 * self.dx;
        }
        self.d += 2 * self.dy;
        self.x += 1;
        self.is_done_low()
    }

    pub const fn new_high(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let mut dx = x1 - x0;
        let dy = y1 - y0;
        let mut xi = 1;

        if dx < 0 {
            xi = -1;
            dx = -dx;
        }

        let d = 2 * dx - dy;
        let x = x0;

        Self {
            dx,
            dy,
            i: xi,
            d,
            x,
            y: y0,
            x1,
            y1,
        }
    }

    #[inline(always)]
    pub const fn is_done_high(&self) -> bool {
        self.y > self.y1
    }

    pub const fn advance_high(&mut self) -> bool {
        if self.d > 0 {
            self.x += self.i;
            self.d -= 2 * self.dy;
        }
        self.d += 2 * self.dx;
        self.y += 1;
        self.is_done_high()
    }

    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> (Self, BresenhamKind) {
        #[allow(clippy::collapsible_else_if)]
        if (y1 - y0).abs() < (x1 - x0).abs() {
            if x0 > x1 {
                (Self::new_low(x1, y1, x0, y0), BresenhamKind::Low)
            } else {
                (Self::new_low(x0, y0, x1, y1), BresenhamKind::Low)
            }
        } else {
            if y0 > y1 {
                (Self::new_high(x1, y1, x0, y0), BresenhamKind::High)
            } else {
                (Self::new_high(x0, y0, x1, y1), BresenhamKind::High)
            }
        }
    }

    pub const fn is_done(&self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.is_done_low(),
            BresenhamKind::High => self.is_done_high(),
        }
    }

    pub const fn advance(&mut self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.advance_low(),
            BresenhamKind::High => self.advance_high(),
        }
    }
}

pub unsafe fn line_unchecked(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    buffer: &mut [BGRA8],
    stride: usize,
    width: i32,
    height: i32,
    color: BGRA8,
) {
    let (mut machine, kind) = Bresenham::new(x0, y0, x1, y1);
    loop {
        let (x, y) = machine.current();

        'a: {
            if y < 0 || y >= height {
                break 'a;
            }

            if x < 0 || x >= width {
                break 'a;
            }

            let i = y as usize * stride + x as usize;
            buffer[i] = color;
        }

        if machine.advance(kind) {
            return;
        }
    }
}

pub unsafe fn horizontal_line_unchecked(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [BGRA8],
    width: i32,
    color: BGRA8,
) {
    for x in x0.clamp(0, width)..(x1 + 1).clamp(0, width) {
        *offset_buffer.get_unchecked_mut(x as usize) = color;
    }
}

macro_rules! check_buffer {
    ($what: literal, $buffer: ident, $width: ident, $height: ident) => {
        if $buffer.len() < $width as usize * $height as usize {
            panic!(concat!(
                "Buffer passed to rasterize::",
                $what,
                " is too small"
            ))
        }
    };
}

pub fn line(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("line", buffer, width, height);

    unsafe {
        line_unchecked(
            x0,
            y0,
            x1,
            y1,
            buffer,
            width as usize,
            width as i32,
            height as i32,
            color,
        )
    }
}

pub fn horizontal_line(
    y: i32,
    x0: i32,
    x1: i32,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("horizontal_line", buffer, width, height);

    if y < 0 || y >= height as i32 {
        return;
    }

    unsafe {
        horizontal_line_unchecked(
            x0,
            x1,
            &mut buffer[y as usize * width as usize..],
            width as i32,
            color,
        )
    }
}

pub fn stroke_polygon(
    points: impl IntoIterator<Item = (i32, i32)>,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("stroke_polygon", buffer, width, height);

    let mut it = points.into_iter();
    let Some(first) = it.next() else {
        return;
    };

    let mut last = first;
    for next in it {
        unsafe {
            line_unchecked(
                last.0,
                last.1,
                next.0,
                next.1,
                buffer,
                width as usize,
                width as i32,
                height as i32,
                color,
            );
        }

        last = next;
    }

    unsafe {
        line_unchecked(
            last.0,
            last.1,
            first.0,
            first.1,
            buffer,
            width as usize,
            width as i32,
            height as i32,
            color,
        );
    }
}

pub fn stroke_triangle(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("stroke_triangle", buffer, width, height);

    let stride = width as usize;
    unsafe {
        line_unchecked(
            x0,
            y0,
            x1,
            y1,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }

    unsafe {
        line_unchecked(
            x1,
            y1,
            x2,
            y2,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }

    unsafe {
        line_unchecked(
            x0,
            y0,
            x2,
            y2,
            buffer,
            stride,
            width as i32,
            height as i32,
            color,
        );
    }
}

unsafe fn draw_triangle_half(
    mut current_y: i32,
    machine1: &mut Bresenham,
    kind1: BresenhamKind,
    machine2: &mut Bresenham,
    kind2: BresenhamKind,
    buffer: &mut [BGRA8],
    stride: usize,
    width: u32,
    height: u32,
    color: BGRA8,
) -> i32 {
    'top: loop {
        // Advance both lines until they are at the current y
        let m1x = loop {
            let (m1x, m1y) = machine1.current();
            if m1y == current_y {
                break m1x;
            } else if machine1.advance(kind1) {
                break 'top;
            }
        };
        let m2x = loop {
            let (m2x, m2y) = machine2.current();
            if m2y == current_y {
                break m2x;
            } else if machine2.advance(kind2) {
                break 'top;
            }
        };

        // Fill the appropriate part of the line at the current y
        if current_y >= 0 && current_y < height as i32 {
            let (lx1, lx2) = if m1x < m2x { (m1x, m2x) } else { (m2x, m1x) };

            unsafe {
                horizontal_line_unchecked(
                    lx1,
                    lx2,
                    &mut buffer[current_y as usize * stride..],
                    width as i32,
                    color,
                );
            }
        }

        current_y += 1;
    }
    current_y
}

pub fn fill_triangle(
    mut x0: i32,
    mut y0: i32,
    mut x1: i32,
    mut y1: i32,
    mut x2: i32,
    mut y2: i32,
    buffer: &mut [BGRA8],
    stride: usize,
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("fill_triangle", buffer, width, height);

    // First, ensure (x0, y0) is the highest point of the triangle
    if y1 < y0 {
        if y2 < y1 {
            std::mem::swap(&mut y0, &mut y2);
            std::mem::swap(&mut x0, &mut x2);
        } else {
            std::mem::swap(&mut y0, &mut y1);
            std::mem::swap(&mut x0, &mut x1);
        }
    } else if y2 < y0 {
        std::mem::swap(&mut y0, &mut y2);
        std::mem::swap(&mut x0, &mut x2);
    }

    // Next, ensure (x2, y2) is the lowest point of the rectangle
    if y1 > y2 {
        std::mem::swap(&mut y2, &mut y1);
        std::mem::swap(&mut x2, &mut x1);
    }

    let (mut machine1, kind1) = Bresenham::new(x0, y0, x1, y1);
    let (mut machine2, kind2) = Bresenham::new(x0, y0, x2, y2);

    let mut current_y = y0;
    current_y = unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            kind1,
            &mut machine2,
            kind2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };

    let (mut machine1, kind1) = {
        if machine1.is_done(kind1) {
            (machine2, kind2)
        } else {
            (machine1, kind1)
        }
    };
    let (mut machine2, kind2) = Bresenham::new(x1, y1, x2, y2);

    unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            kind1,
            &mut machine2,
            kind2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };
}

const POLYGON_RASTERIZER_DEBUG_PRINT: bool = false;

type IFixed18Dot14 = I32Fixed<14>;

#[derive(Debug)]
struct Profile {
    current: IFixed18Dot14,
    step: IFixed18Dot14,
    end_y: u32,
}

#[derive(Debug)]
pub struct NonZeroPolygonRasterizer {
    queue: Vec<(u32, bool, Profile)>,
    left: Vec<Profile>,
    right: Vec<Profile>,
}

impl NonZeroPolygonRasterizer {
    pub const fn new() -> Self {
        Self {
            queue: Vec::new(),
            left: Vec::new(),
            right: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.queue.clear();
        self.left.clear();
        self.right.clear();
    }

    fn add_line(&mut self, offset: (i32, i32), start: &Point2, end: &Point2, invert_winding: bool) {
        let istart = (
            IFixed18Dot14::from_f32(start.x) + offset.0,
            IFixed18Dot14::from_f32(start.y) + offset.1,
        );
        let iend = (
            IFixed18Dot14::from_f32(end.x) + offset.0,
            IFixed18Dot14::from_f32(end.y) + offset.1,
        );

        let direction = match iend.1.cmp(&istart.1) {
            // Line is going up
            std::cmp::Ordering::Less => false ^ invert_winding,
            // Horizontal line, ignore
            std::cmp::Ordering::Equal => return,
            // Line is going down
            std::cmp::Ordering::Greater => true ^ invert_winding,
        };

        let step = if istart.0 == iend.0 {
            IFixed18Dot14::ZERO
        } else {
            (iend.0 - istart.0) / (iend.1 - istart.1)
        };

        let start_y = istart.1.round_to_inner();
        let mut start_x = istart.0;
        start_x -= (istart.1 - start_y) * step;

        let end_y = iend.1.round_to_inner();
        let mut end_x = iend.0;
        end_x -= (iend.1 - end_y) * step;

        let (mut top_y, mut bottom_y, mut init_x) = if end_y >= start_y {
            (start_y, end_y, start_x)
        } else {
            (end_y, start_y, end_x)
        };

        // FIXME: HACK: This is terrible but I tried everything and only this works
        bottom_y -= 1;
        init_x -= step;

        if top_y < 0 {
            init_x += step * -top_y;
            top_y = 0;
        }

        if top_y > bottom_y {
            return;
        }

        if POLYGON_RASTERIZER_DEBUG_PRINT {
            println!("{start_y} {end_y} {start_x} {end_x}");
            println!("{top_y} {bottom_y}");
            println!(
                "line {start:?} -- {end:?} results in top_y={top_y} direction={:?}",
                step > 0
            );
        }

        self.queue.push((
            top_y as u32,
            direction,
            Profile {
                current: init_x,
                step,
                end_y: bottom_y as u32,
            },
        ));
    }

    pub fn append_polyline(
        &mut self,
        offset: (i32, i32),
        polyline: &[Point2],
        invert_winding: bool,
    ) {
        if polyline.is_empty() {
            return;
        }

        let mut i = 0;
        while i < polyline.len() - 1 {
            let start = &polyline[i];
            i += 1;
            let end = &polyline[i];
            self.add_line(offset, start, end, invert_winding)
        }

        let last = polyline.last().unwrap();
        if &polyline[0] != last {
            self.add_line(offset, last, &polyline[0], invert_winding)
        }
    }

    fn queue_pop_if(&mut self, cy: u32) -> Option<(u32, bool, Profile)> {
        let &(y, ..) = self.queue.last()?;

        if y <= cy {
            self.queue.pop()
        } else {
            None
        }
    }

    fn push_queue_to_lr(&mut self, cy: u32) {
        while let Some((_, d, p)) = self.queue_pop_if(cy) {
            let vec = if d { &mut self.right } else { &mut self.left };
            let idx = match vec.binary_search_by_key(&p.current, |profile| profile.current) {
                Ok(i) => i,
                Err(i) => i,
            };
            vec.insert(idx, p);
        }
    }

    fn prune_lr(&mut self, cy: u32) {
        self.left.retain(|profile| profile.end_y >= cy);
        self.right.retain(|profile| profile.end_y >= cy);
    }

    fn advance_lr_sort(&mut self) {
        for profile in self.left.iter_mut() {
            profile.current += profile.step;
        }

        for profile in self.right.iter_mut() {
            profile.current += profile.step;
        }

        self.left.sort_unstable_by_key(|profile| profile.current);
        self.right.sort_unstable_by_key(|profile| profile.current);
    }

    pub fn render_scanlines(
        &mut self,
        width: u32,
        height: u32,
        mut filler: impl FnMut(u32, u32, u32),
    ) {
        self.queue.sort_unstable_by(|(ay, ..), (by, ..)| by.cmp(ay));

        if self.queue.is_empty() {
            return;
        }

        let mut y = self.queue.last().unwrap().0;

        while (!self.queue.is_empty() || !self.left.is_empty()) && y < height {
            self.prune_lr(y);
            self.push_queue_to_lr(y);

            if POLYGON_RASTERIZER_DEBUG_PRINT {
                println!("--- POLYLINE RASTERIZER SCANLINE y={y} ---");
                println!("left: {:?}", self.left);
                println!("right: {:?}", self.right);
                assert_eq!(self.left.len(), self.right.len());
            }

            for i in 0..self.left.len() {
                let (left, right) = (&self.left[i], &self.right[i]);

                let round_clamp = |f: IFixed18Dot14| (f.round_to_inner().max(0) as u32).min(width);
                let mut x0 = round_clamp(left.current);
                let mut x1 = round_clamp(right.current);
                // TODO: is this necessary? can this be removed?
                if x0 > x1 {
                    std::mem::swap(&mut x0, &mut x1);
                }
                filler(y, x0, x1);
            }

            self.advance_lr_sort();

            y += 1;
        }
    }
}

unsafe fn blit_monochrome_unchecked(
    dx: i32,
    dy: i32,
    buffer: &mut [BGRA8],
    dstride: u32,
    color: BGRA8,
    ys: Range<usize>,
    xs: Range<usize>,
    stride: u32,
    source: &[u8],
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * stride as usize + x;
            let sv = *unsafe { source.get_unchecked(si) };

            let di = (fx as usize) + (fy as usize) * dstride as usize;
            let d = unsafe { buffer.get_unchecked_mut(di) };
            *d = color.mul_alpha(sv).blend_over(*d).0;
        }
    }
}

unsafe fn blit_bgra_unchecked(
    dx: i32,
    dy: i32,
    buffer: &mut [BGRA8],
    dstride: u32,
    alpha: u8,
    ys: Range<usize>,
    xs: Range<usize>,
    stride: u32,
    source: &[BGRA8],
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * stride as usize + x;
            // NOTE: This is actually pre-multiplied in linear space...
            //       But I think libass ignores this too.
            //       See note in color.rs
            let n = Premultiplied(*source.get_unchecked(si));

            let di = (fx as usize) + (fy as usize) * dstride as usize;
            let d = unsafe { buffer.get_unchecked_mut(di) };
            *d = n.mul_alpha(alpha).blend_over(*d).0;
        }
    }
}

// TODO: maybe for consistency this should actually care about the color alpha value
//       I think we're memory bottlenecked here anyway
unsafe fn blit_monochrome_float_noalpha_unchecked(
    dx: i32,
    dy: i32,
    buffer: &mut [BGRA8],
    dstride: u32,
    color: [u8; 3],
    ys: Range<usize>,
    xs: Range<usize>,
    stride: u32,
    source: &[f32],
) {
    for y in ys {
        let fy = dy + y as i32;
        for x in xs.clone() {
            let fx = dx + x as i32;

            let si = y * stride as usize + x;
            let sv = (*unsafe { source.get_unchecked(si) }).clamp(0.0, 1.0);

            let di = (fx as usize) + (fy as usize) * dstride as usize;
            let d = unsafe { buffer.get_unchecked_mut(di) };

            let c = BGRA8::from_bytes([color[0], color[1], color[2], (sv * 255.0) as u8]);
            *d = c.blend_over(*d).0;
        }
    }
}

pub struct SoftwareRasterizer {
    polygon_offset: Vec2,
    polygon_rasterizer: NonZeroPolygonRasterizer,

    blurer: Blurer,
    is_in_blur: bool,
}

impl SoftwareRasterizer {
    pub fn new() -> Self {
        Self {
            polygon_offset: Vec2::ZERO,
            polygon_rasterizer: NonZeroPolygonRasterizer::new(),
            blurer: Blurer::new(),
            is_in_blur: false,
        }
    }

    fn get_target_buffer<'a, 'b: 'a>(
        handle: &'a mut super::RenderTargetHandle<'b>,
    ) -> &'a mut [BGRA8] {
        match handle {
            super::RenderTargetHandle::Sw(buffer) => buffer,
            _ => panic!("Unexpected render handle passed to software rasterizer: {handle:?}"),
        }
    }

    fn get_texture_data<'a, 'b: 'a>(handle: &'a super::TextureDataHandle) -> &'a Bitmap<Dynamic> {
        match handle {
            super::TextureDataHandle::Sw(buffer) => buffer,
            handle => panic!("Unexpected texture handle passed to software rasterizer: {handle:?}"),
        }
    }

    pub fn create_render_target(buffer: &mut [BGRA8], width: u32, height: u32) -> RenderTarget {
        assert_eq!(buffer.len(), width as usize * height as usize);

        buffer.fill(BGRA8::TRANSPARENT);

        RenderTarget {
            width,
            height,
            handle: super::RenderTargetHandle::Sw(buffer),
        }
    }
}

impl super::Rasterizer for SoftwareRasterizer {
    fn downcast_sw(&mut self) -> Option<&mut SoftwareRasterizer> {
        Some(self)
    }

    fn copy_or_move_into_texture(&mut self, data: Arc<Bitmap<Dynamic>>) -> super::Texture {
        super::Texture {
            width: data.width(),
            height: data.height(),
            format: PixelFormat::Bgra,
            handle: super::TextureDataHandle::Sw(data),
        }
    }

    fn line(
        &mut self,
        target: &mut super::RenderTarget,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: BGRA8,
    ) {
        let buffer = Self::get_target_buffer(&mut target.handle);

        unsafe {
            line_unchecked(
                x0 as i32,
                y0 as i32,
                x1 as i32,
                y1 as i32,
                buffer,
                target.width as usize,
                target.width as i32,
                target.height as i32,
                color,
            );
        }
    }

    fn horizontal_line(
        &mut self,
        target: &mut super::RenderTarget,
        y: f32,
        x0: f32,
        x1: f32,
        color: BGRA8,
    ) {
        let buffer = Self::get_target_buffer(&mut target.handle);
        horizontal_line(
            y as i32,
            x0 as i32,
            x1 as i32,
            buffer,
            target.width,
            target.height,
            color,
        );
    }

    fn stroke_polygon(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[Point2],
        color: BGRA8,
    ) {
        let buffer = Self::get_target_buffer(&mut target.handle);
        stroke_polygon(
            vertices.iter().map(|&Point2 { x, y }| (x as i32, y as i32)),
            buffer,
            target.width,
            target.height,
            color,
        );
    }

    fn fill_triangle(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[Point2; 3],
        color: BGRA8,
    ) {
        let buffer = Self::get_target_buffer(&mut target.handle);
        fill_triangle(
            vertices[0].x as i32,
            vertices[0].y as i32,
            vertices[1].x as i32,
            vertices[1].y as i32,
            vertices[2].x as i32,
            vertices[2].y as i32,
            buffer,
            target.width as usize,
            target.width,
            target.height,
            color,
        );
    }

    fn polygon_reset(&mut self, offset: crate::math::Vec2) {
        self.polygon_offset = offset;
        self.polygon_rasterizer.reset();
    }

    fn polygon_add_polyline(&mut self, vertices: &[Point2], winding: bool) {
        self.polygon_rasterizer.append_polyline(
            (self.polygon_offset.x as i32, self.polygon_offset.y as i32),
            vertices,
            winding,
        );
    }

    fn polygon_fill(&mut self, target: &mut super::RenderTarget, color: BGRA8) {
        let buffer = Self::get_target_buffer(&mut target.handle);
        self.polygon_rasterizer
            .render_scanlines(target.width, target.height, |y, x0, x1| unsafe {
                horizontal_line_unchecked(
                    x0 as i32,
                    x1 as i32,
                    &mut buffer[y as usize * target.width as usize..],
                    target.width as i32,
                    color,
                );
            });
    }

    fn blit(
        &mut self,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
        alpha: BGRA8,
    ) {
        let buffer = Self::get_target_buffer(&mut target.handle);
        let Some(BlitRectangle { xs, ys }) = calculate_blit_rectangle(
            dx,
            dy,
            target.width as usize,
            target.height as usize,
            texture.width as usize,
            texture.height as usize,
        ) else {
            return;
        };

        match Self::get_texture_data(&texture.handle).cast() {
            BitmapCast::Bgra(bitmap) => unsafe {
                debug_assert_eq!(texture.format, PixelFormat::Bgra);

                blit_bgra_unchecked(
                    dx,
                    dy,
                    buffer,
                    target.width,
                    alpha.a,
                    ys,
                    xs,
                    texture.width,
                    bitmap.data(),
                );
            },
            BitmapCast::Mono(bitmap) => unsafe {
                debug_assert_eq!(texture.format, PixelFormat::Mono);

                blit_monochrome_unchecked(
                    dx,
                    dy,
                    buffer,
                    target.width,
                    alpha,
                    ys,
                    xs,
                    texture.width,
                    bitmap.data(),
                );
            },
        }
    }

    fn blur_prepare(&mut self, width: u32, height: u32, sigma: f32) {
        if self.is_in_blur {
            panic!("SoftwareRasterizer::blur_prepare called twice")
        }

        self.is_in_blur = true;
        self.blurer.prepare(
            width as usize,
            height as usize,
            gaussian_sigma_to_box_radius(sigma),
        );
    }

    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &super::Texture) {
        let dx = dx + self.blurer.padding() as i32;
        let dy = dy + self.blurer.padding() as i32;

        let Some(BlitRectangle { xs, ys }) = calculate_blit_rectangle(
            dx,
            dy,
            self.blurer.width(),
            self.blurer.height(),
            texture.width as usize,
            texture.height as usize,
        ) else {
            return;
        };

        match Self::get_texture_data(&texture.handle).cast() {
            BitmapCast::Bgra(bitmap) => unsafe {
                debug_assert_eq!(texture.format, PixelFormat::Bgra);

                self.blurer.buffer_blit_bgra8_unchecked(
                    dx as usize,
                    dy as usize,
                    bitmap.data(),
                    ys,
                    xs,
                    texture.width as usize,
                );
            },
            BitmapCast::Mono(mono) => unsafe {
                debug_assert_eq!(texture.format, PixelFormat::Mono);

                self.blurer.buffer_blit_mono8_unchecked(
                    dx as usize,
                    dy as usize,
                    mono.data(),
                    ys,
                    xs,
                    texture.width as usize,
                );
            },
        }
    }

    fn blur_execute(&mut self, target: &mut RenderTarget, dx: i32, dy: i32, color: [u8; 3]) {
        self.is_in_blur = false;
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();

        let buffer = Self::get_target_buffer(&mut target.handle);
        let Some(BlitRectangle { xs, ys }) = calculate_blit_rectangle(
            dx - self.blurer.padding() as i32,
            dy - self.blurer.padding() as i32,
            target.width as usize,
            target.height as usize,
            self.blurer.width(),
            self.blurer.height(),
        ) else {
            return;
        };

        unsafe {
            blit_monochrome_float_noalpha_unchecked(
                dx - self.blurer.padding() as i32,
                dy - self.blurer.padding() as i32,
                buffer,
                target.width,
                color,
                ys,
                xs,
                self.blurer.width() as u32,
                self.blurer.front(),
            );
        }
    }
}
