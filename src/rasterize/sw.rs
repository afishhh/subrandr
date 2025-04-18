use std::{mem::MaybeUninit, sync::Arc};

use blur::gaussian_sigma_to_box_radius;

use crate::{color::BGRA8, math::Point2f};

mod blit;
pub(super) mod blur;

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

        Self {
            dx,
            dy,
            i: yi,
            d,
            x: x0,
            y: y0,
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

        Self {
            dx,
            dy,
            i: xi,
            d,
            x: x0,
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

    pub const fn advance(&mut self, kind: BresenhamKind) -> bool {
        match kind {
            BresenhamKind::Low => self.advance_low(),
            BresenhamKind::High => self.advance_high(),
        }
    }
}

struct DynBresenham {
    dx: i32,
    dy: i32,
    sx: i32,
    sy: i32,
    err: i32,

    x: i32,
    y: i32,
    x1: i32,
    y1: i32,
}

impl DynBresenham {
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> Self {
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let err = dx + dy;

        Self {
            dx,
            dy,
            sx,
            sy,
            err,
            x: x0,
            y: y0,
            x1,
            y1,
        }
    }

    pub const fn current(&mut self) -> (i32, i32) {
        (self.x, self.y)
    }

    pub const fn advance(&mut self) -> bool {
        let err2 = 2 * self.err;

        if err2 >= self.dy {
            if self.x == self.x1 {
                return true;
            }
            self.err += self.dy;
            self.x += self.sx;
        }

        if err2 <= self.dx {
            if self.y == self.y1 {
                return true;
            }
            self.err += self.dx;
            self.y += self.sy;
        }

        self.is_done()
    }

    pub const fn is_done(&self) -> bool {
        self.x == self.x1 && self.y == self.y1
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

pub(super) unsafe fn horizontal_line_unchecked(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [BGRA8],
    width: i32,
    color: BGRA8,
) {
    for x in x0.clamp(0, width)..x1.clamp(0, width) {
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

pub fn fill_axis_aligned_rect(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("fill_axis_aligned_rect", buffer, width, height);

    debug_assert!(x0 <= x1);
    debug_assert!(y0 <= y1);

    for y in y0.clamp(0, height as i32)..y1.clamp(0, height as i32) {
        unsafe {
            horizontal_line_unchecked(
                x0,
                x1,
                &mut buffer[y as usize * width as usize..],
                width as i32,
                color,
            );
        }
    }
}

pub fn fill_axis_aligned_antialias_rect(
    x0: i32,
    y0: f32,
    x1: i32,
    y1: f32,
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    color: BGRA8,
) {
    check_buffer!("fill_axis_aligned_antialias_rect", buffer, width, height);

    debug_assert!(x0 <= x1);
    debug_assert!(y0 <= y1);

    let y0i = if (y0 - y0.round()).abs() > 0.01 {
        let top_fill = 1.0 - y0.fract();
        let top_y = y0.floor() as i32;
        if top_y >= 0 && top_y < height as i32 {
            unsafe {
                horizontal_line_unchecked(
                    x0,
                    x1,
                    &mut buffer[top_y as usize * width as usize..],
                    width as i32,
                    color.mul_alpha((top_fill * 255.) as u8),
                );
            }
        }
        y0.ceil() as i32
    } else {
        y0.round() as i32
    };

    let y1i = if (y1 - y1.round()).abs() > 0.01 {
        let bottom_fill = y1.fract();
        let bottom_y = y1.ceil() as i32;
        if bottom_y >= 0 && bottom_y < height as i32 {
            unsafe {
                horizontal_line_unchecked(
                    x0,
                    x1,
                    &mut buffer[bottom_y as usize * width as usize..],
                    width as i32,
                    color.mul_alpha((bottom_fill * 255.) as u8),
                );
            }
        }
        y1.ceil() as i32
    } else {
        y1.round() as i32
    };

    for y in y0i.clamp(0, height as i32)..y1i.clamp(0, height as i32) {
        unsafe {
            horizontal_line_unchecked(
                x0,
                x1,
                &mut buffer[y as usize * width as usize..],
                width as i32,
                color,
            );
        }
    }
}

unsafe fn draw_triangle_half(
    mut current_y: i32,
    machine1: &mut DynBresenham,
    machine2: &mut DynBresenham,
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
            } else if machine1.is_done() {
                break 'top;
            } else {
                machine1.advance();
            }
        };
        let m2x = loop {
            let (m2x, m2y) = machine2.current();
            if m2y == current_y {
                break m2x;
            } else if machine2.is_done() {
                break 'top;
            } else {
                machine2.advance();
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

fn fill_triangle(
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

    let mut machine1 = DynBresenham::new(x0, y0, x1, y1);
    let mut machine2 = DynBresenham::new(x0, y0, x2, y2);

    let mut current_y = y0;
    current_y = unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            &mut machine2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };

    let mut machine1 = {
        if machine1.is_done() {
            machine2
        } else {
            machine1
        }
    };
    let mut machine2 = DynBresenham::new(x1, y1, x2, y2);

    unsafe {
        draw_triangle_half(
            current_y,
            &mut machine1,
            &mut machine2,
            buffer,
            stride,
            width,
            height,
            color,
        )
    };
}

pub(super) struct RenderTargetImpl<'a> {
    buffer: &'a mut [BGRA8],
    pub width: u32,
    pub height: u32,
}

pub fn create_render_target(buffer: &mut [BGRA8], width: u32, height: u32) -> super::RenderTarget {
    super::RenderTarget(super::RenderTargetInner::Software(RenderTargetImpl {
        buffer,
        width,
        height,
    }))
}

fn unwrap_sw_render_target<'a, 'b>(
    target: &'a mut super::RenderTarget<'b>,
) -> &'a mut RenderTargetImpl<'b> {
    match &mut target.0 {
        super::RenderTargetInner::Software(target) => target,
        target => panic!(
            "Incompatible render target {:?} passed to software rasterizer (expected: software)",
            target.variant_name()
        ),
    }
}

// TODO: The software rasterizer could unify render targets with textures (via borrowed textures)
//       and not have to worry about this split anymore.
pub(super) struct TextureRenderTargetImpl {
    buffer: Arc<[u8]>,
    pub width: u32,
    pub height: u32,
}

fn unwrap_sw_render_texture<'a>(
    target: &'a mut super::RenderTarget,
) -> &'a mut TextureRenderTargetImpl {
    match &mut target.0 {
        super::RenderTargetInner::SoftwareTexture(target) => target,
        target => panic!(
            "Incompatible render target {:?} passed to software rasterizer (expected: software texture)",
            target.variant_name()
        ),
    }
}

#[derive(Clone)]
pub(super) enum TextureData {
    OwnedMono(Arc<[u8]>),
    OwnedBgra(Arc<[BGRA8]>),
}

#[derive(Clone)]
pub(super) struct TextureImpl {
    pub width: u32,
    pub height: u32,
    pub data: TextureData,
}

enum UnwrappedTextureData<'a> {
    Mono(&'a [u8]),
    Bgra(&'a [BGRA8]),
}

struct UnwrappedTexture<'a> {
    width: u32,
    height: u32,
    data: UnwrappedTextureData<'a>,
}

fn unwrap_sw_texture<'a>(texture: &'a super::Texture) -> UnwrappedTexture<'a> {
    #[cfg_attr(not(feature = "wgpu"), expect(unreachable_patterns))]
    match &texture.0 {
        super::TextureInner::Software(texture) => UnwrappedTexture {
            width: texture.width,
            height: texture.height,
            data: match &texture.data {
                TextureData::OwnedMono(mono) => UnwrappedTextureData::Mono(&mono),
                TextureData::OwnedBgra(bgra) => UnwrappedTextureData::Bgra(&bgra),
            },
        },
        target => panic!(
            "Incompatible texture {:?} passed to software rasterizer",
            target.variant_name()
        ),
    }
}

pub struct Rasterizer {
    blurer: blur::Blurer,
}

impl Rasterizer {
    pub fn new() -> Self {
        Self {
            blurer: blur::Blurer::new(),
        }
    }
}

impl super::Rasterizer for Rasterizer {
    fn name(&self) -> &'static str {
        "software"
    }

    fn adapter_info_string(&self) -> Option<String> {
        None
    }

    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: super::PixelFormat,
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> super::Texture {
        let n_pixels = width as usize * height as usize;
        match format {
            super::PixelFormat::Mono => {
                let mut data = Arc::new_uninit_slice(n_pixels);
                let slice = unsafe { Arc::get_mut(&mut data).unwrap_unchecked() };
                callback(slice, width as usize);
                super::Texture(super::TextureInner::Software(TextureImpl {
                    width,
                    height,
                    data: TextureData::OwnedMono(Arc::<[MaybeUninit<u8>]>::assume_init(data)),
                }))
            }
            super::PixelFormat::Bgra => {
                let mut data: Arc<[MaybeUninit<BGRA8>]> = Arc::new_uninit_slice(n_pixels);
                let slice = unsafe { Arc::get_mut(&mut data).unwrap_unchecked() };
                let slice = unsafe {
                    std::slice::from_raw_parts_mut(
                        slice.as_mut_ptr().cast::<MaybeUninit<u8>>(),
                        slice.len() * 4,
                    )
                };

                callback(slice, width as usize * 4);

                super::Texture(super::TextureInner::Software(TextureImpl {
                    width,
                    height,
                    data: TextureData::OwnedBgra(Arc::<[MaybeUninit<BGRA8>]>::assume_init(data)),
                }))
            }
        }
    }

    fn create_mono_texture_rendered(
        &mut self,
        width: u32,
        height: u32,
    ) -> super::RenderTarget<'static> {
        super::RenderTarget(super::RenderTargetInner::SoftwareTexture(
            TextureRenderTargetImpl {
                buffer: {
                    let mut uninit = Arc::new_uninit_slice(width as usize * height as usize);
                    unsafe {
                        Arc::get_mut(&mut uninit)
                            .unwrap_unchecked()
                            .fill(MaybeUninit::zeroed());
                        Arc::<[MaybeUninit<u8>]>::assume_init(uninit)
                    }
                },
                width,
                height,
            },
        ))
    }

    fn finalize_texture_render(&mut self, target: super::RenderTarget<'static>) -> super::Texture {
        match target.0 {
            super::RenderTargetInner::SoftwareTexture(texture) => {
                super::Texture(super::TextureInner::Software(TextureImpl {
                    width: texture.width,
                    height: texture.height,
                    data: TextureData::OwnedMono(texture.buffer)
                }))
            }
            target => panic!(
                "Incompatible texture {:?} passed to software Rasterizer::finalize_texture_render (expected: software texture)",
                target.variant_name()
            ),
        }
    }

    fn line(&mut self, target: &mut super::RenderTarget, p0: Point2f, p1: Point2f, color: BGRA8) {
        let target = unwrap_sw_render_target(target);

        unsafe {
            line_unchecked(
                p0.x as i32,
                p0.y as i32,
                p1.x as i32,
                p1.y as i32,
                target.buffer,
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
        let target = unwrap_sw_render_target(target);
        let y = y as i32;

        if y < 0 || y >= target.height as i32 {
            return;
        }

        unsafe {
            horizontal_line_unchecked(
                x0 as i32,
                x1 as i32,
                &mut target.buffer[y as usize * target.width as usize..],
                target.width as i32,
                color,
            )
        }
    }

    fn fill_triangle(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[Point2f; 3],
        color: BGRA8,
    ) {
        let target = unwrap_sw_render_target(target);

        fill_triangle(
            vertices[0].x as i32,
            vertices[0].y as i32,
            vertices[1].x as i32,
            vertices[1].y as i32,
            vertices[2].x as i32,
            vertices[2].y as i32,
            target.buffer,
            target.width as usize,
            target.width,
            target.height,
            color,
        );
    }

    fn fill_axis_aligned_rect(
        &mut self,
        target: &mut super::RenderTarget,
        rect: crate::math::Rect2f,
        color: BGRA8,
    ) {
        let target = unwrap_sw_render_target(target);

        fill_axis_aligned_rect(
            rect.min.x as i32,
            rect.min.y as i32,
            rect.max.x as i32,
            rect.max.y as i32,
            target.buffer,
            target.width,
            target.height,
            color,
        );
    }

    fn fill_axis_aligned_antialias_rect(
        &mut self,
        target: &mut super::RenderTarget,
        rect: crate::math::Rect2f,
        color: BGRA8,
    ) {
        let target = unwrap_sw_render_target(target);

        // FIXME: GPU rasterization seems to treat all these rects as edge-exclusive
        //        which approach is correct/which should we settle on for Rasterizer?
        let y0 = rect.min.y + 1.;
        let y1 = rect.max.y - 1.;

        if y0 > y1 {
            return;
        }

        fill_axis_aligned_antialias_rect(
            rect.min.x as i32,
            y0,
            rect.max.x as i32,
            y1,
            target.buffer,
            target.width,
            target.height,
            color,
        );
    }

    fn blit(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
        color: BGRA8,
    ) {
        let target = unwrap_sw_render_target(target);
        let texture = unwrap_sw_texture(texture);

        match texture.data {
            UnwrappedTextureData::Mono(source) => {
                blit::blit_monochrome(
                    target.buffer,
                    target.width as usize,
                    target.width as usize,
                    target.height as usize,
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    dx,
                    dy,
                    color,
                );
            }
            UnwrappedTextureData::Bgra(source) => {
                blit::blit_bgra(
                    target.buffer,
                    target.width as usize,
                    target.width as usize,
                    target.height as usize,
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    dx,
                    dy,
                    color.a,
                );
            }
        }
    }

    unsafe fn blit_to_mono_texture_unchecked(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
    ) {
        let target = unwrap_sw_render_texture(target);
        let texture = unwrap_sw_texture(texture);

        match texture.data {
            UnwrappedTextureData::Mono(source) => unsafe {
                blit::blit_mono_to_mono_unchecked(
                    Arc::get_mut(&mut target.buffer).unwrap_unchecked(),
                    target.width as usize,
                    dx,
                    dy,
                    source,
                    texture.width as usize,
                    texture.height as usize,
                );
            },
            UnwrappedTextureData::Bgra(source) => unsafe {
                blit::blit_bgra_to_mono_unchecked(
                    Arc::get_mut(&mut target.buffer).unwrap_unchecked(),
                    target.width as usize,
                    dx,
                    dy,
                    source,
                    texture.width as usize,
                    texture.height as usize,
                );
            },
        }
    }

    fn blit_cpu_polygon(
        &mut self,
        target: &mut super::RenderTarget,
        rasterizer: &mut super::polygon::NonZeroPolygonRasterizer,
        color: BGRA8,
    ) {
        let target = unwrap_sw_render_target(target);

        rasterizer.render_to(
            target.buffer,
            target.width as usize,
            target.width,
            target.height,
            color,
        );
    }

    fn blur_prepare(&mut self, width: u32, height: u32, sigma: f32) {
        self.blurer.prepare(
            width as usize,
            height as usize,
            gaussian_sigma_to_box_radius(sigma),
        );
    }

    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &super::Texture) {
        let texture = unwrap_sw_texture(texture);
        let dx = dx + self.blurer.padding() as i32;
        let dy = dy + self.blurer.padding() as i32;

        let Some((xs, ys)) = blit::calculate_blit_rectangle(
            dx,
            dy,
            self.blurer.width(),
            self.blurer.height(),
            texture.width as usize,
            texture.height as usize,
        ) else {
            return;
        };

        match texture.data {
            UnwrappedTextureData::Mono(source) => unsafe {
                self.blurer.buffer_blit_mono8_unchecked(
                    dx,
                    dy,
                    source,
                    xs,
                    ys,
                    texture.width as usize,
                );
            },
            UnwrappedTextureData::Bgra(source) => unsafe {
                self.blurer.buffer_blit_bgra8_unchecked(
                    dx,
                    dy,
                    source,
                    xs,
                    ys,
                    texture.width as usize,
                );
            },
        }
    }

    fn blur_execute(&mut self, target: &mut super::RenderTarget, dx: i32, dy: i32, color: [u8; 3]) {
        let target = unwrap_sw_render_target(target);
        let dx = dx - self.blurer.padding() as i32;
        let dy = dy - self.blurer.padding() as i32;

        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();

        let Some((xs, ys)) = blit::calculate_blit_rectangle(
            dx,
            dy,
            target.width as usize,
            target.height as usize,
            self.blurer.width(),
            self.blurer.height(),
        ) else {
            return;
        };

        unsafe {
            blit::blit_monochrome_float_unchecked(
                target.buffer,
                target.width as usize,
                dx,
                dy,
                xs,
                ys,
                self.blurer.front(),
                self.blurer.width(),
                color,
            );
        }
    }
}
