use std::mem::MaybeUninit;

use polygon::NonZeroPolygonRasterizer;

use crate::{
    color::BGRA8,
    math::{Point2f, Rect2f, Vec2f},
};

pub mod polygon;
pub mod sw;

pub enum PixelFormat {
    Mono,
    Bgra,
}

enum RenderTargetInner<'a> {
    Software(sw::RenderTargetImpl<'a>),
    SoftwareTexture(sw::TextureRenderTargetImpl),
}

impl RenderTargetInner<'_> {
    fn variant_name(&self) -> &'static str {
        match self {
            RenderTargetInner::Software(_) => "software",
            RenderTargetInner::SoftwareTexture(_) => "software texture",
        }
    }
}

pub struct RenderTarget<'a>(RenderTargetInner<'a>);

#[derive(Clone)]
enum TextureInner<'a> {
    Software(sw::TextureImpl<'a>),
}

impl TextureInner<'_> {
    fn variant_name(&self) -> &'static str {
        match self {
            TextureInner::Software(_) => "software",
        }
    }
}

#[derive(Clone)]
pub struct Texture<'a>(TextureInner<'a>);

impl Texture<'_> {
    pub fn width(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.width,
        }
    }

    pub fn height(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.height,
        }
    }

    pub fn format(&self) -> PixelFormat {
        match &self.0 {
            TextureInner::Software(sw) => match sw.data {
                sw::TextureData::BorrowedMono(_) => PixelFormat::Mono,
                sw::TextureData::BorrowedBGRA(_) => PixelFormat::Bgra,
                sw::TextureData::OwnedMono(_) => PixelFormat::Mono,
                sw::TextureData::OwnedBgra(_) => PixelFormat::Bgra,
            },
        }
    }
}

pub trait Rasterizer {
    #[allow(clippy::type_complexity)]
    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>]) + '_>,
    ) -> Texture<'static>;

    fn create_mono_texture_rendered(&mut self, width: u32, height: u32) -> RenderTarget<'static>;
    fn finalize_texture_render(&mut self, target: RenderTarget<'static>) -> Texture<'static>;

    fn line(&mut self, target: &mut RenderTarget, p0: Point2f, p1: Point2f, color: BGRA8);

    fn horizontal_line(
        &mut self,
        target: &mut RenderTarget,
        y: f32,
        x0: f32,
        x1: f32,
        color: BGRA8,
    );

    fn stroke_triangle(
        &mut self,
        target: &mut RenderTarget,
        vertices: &[Point2f; 3],
        color: BGRA8,
    ) {
        self.stroke_polygon(target, Vec2f::ZERO, vertices, color);
    }

    fn fill_triangle(&mut self, target: &mut RenderTarget, vertices: &[Point2f; 3], color: BGRA8);

    fn stroke_polygon(
        &mut self,
        target: &mut RenderTarget,
        offset: Vec2f,
        vertices: &[Point2f],
        color: BGRA8,
    ) {
        let mut last = vertices[vertices.len() - 1];
        for &point in vertices {
            self.line(target, last + offset, point + offset, color);
            last = point;
        }
    }

    fn stroke_polyline(
        &mut self,
        target: &mut RenderTarget,
        offset: Vec2f,
        vertices: &[Point2f],
        color: BGRA8,
    ) {
        let mut last = vertices[0];
        for &point in &vertices[1..] {
            self.line(target, last + offset, point + offset, color);
            last = point;
        }
    }

    fn stroke_axis_aligned_rect(&mut self, target: &mut RenderTarget, rect: Rect2f, color: BGRA8) {
        self.stroke_polygon(
            target,
            Vec2f::ZERO,
            &[
                rect.min,
                Point2f::new(rect.max.x, rect.min.y),
                rect.max,
                Point2f::new(rect.min.x, rect.max.y),
            ],
            color,
        )
    }
    fn fill_axis_aligned_rect(&mut self, target: &mut RenderTarget, rect: Rect2f, color: BGRA8);

    fn blit(
        &mut self,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        texture: &Texture,
        color: BGRA8,
    );

    unsafe fn blit_to_mono_texture_unchecked(
        &mut self,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        texture: &Texture,
    );

    fn blit_cpu_polygon(
        &mut self,
        target: &mut RenderTarget,
        rasterizer: &mut NonZeroPolygonRasterizer,
        color: BGRA8,
    );

    fn blur_prepare(&mut self, width: u32, height: u32, sigma: f32);
    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &Texture);
    fn blur_execute(&mut self, target: &mut RenderTarget, dx: i32, dy: i32, color: [u8; 3]);
}
