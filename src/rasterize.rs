use std::sync::Arc;

use bitmap::{Bitmap, Dynamic, PixelFormat};
use sw::SoftwareRasterizer;
use wgpu::GpuRasterizer;

use crate::{
    color::BGRA8,
    math::{Point2, Vec2},
};

pub mod bitmap;
pub mod sw;
#[cfg(feature = "wgpu")]
pub mod wgpu;

#[derive(Debug, Clone)]
enum TextureDataHandle {
    #[cfg(feature = "wgpu")]
    Gpu(::wgpu::Texture),
    Sw(Arc<Bitmap<Dynamic>>),
}

#[derive(Debug, Clone)]
pub struct Texture {
    width: u32,
    height: u32,
    format: PixelFormat,
    handle: TextureDataHandle,
}

impl Texture {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }
}

#[derive(Debug)]
enum RenderTargetHandle<'a> {
    #[cfg(feature = "wgpu")]
    Gpu(wgpu::GpuRenderTargetHandle),
    Sw(&'a mut [BGRA8]),
}

pub struct RenderTarget<'a> {
    width: u32,
    height: u32,
    handle: RenderTargetHandle<'a>,
}

impl RenderTarget<'_> {
    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

// TODO: static dispatch with an enum instead
pub trait Rasterizer {
    fn downcast_sw(&mut self) -> Option<&mut SoftwareRasterizer> {
        None
    }

    fn downcast_gpu(&mut self) -> Option<&mut GpuRasterizer> {
        None
    }

    fn copy_or_move_into_texture(&mut self, data: Arc<Bitmap<Dynamic>>) -> Texture;

    #[allow(unused_variables)]
    fn submit_render(&mut self, target: RenderTarget) {}

    fn line(&mut self, target: &mut RenderTarget, x0: f32, y0: f32, x1: f32, y1: f32, color: BGRA8);
    fn horizontal_line(
        &mut self,
        target: &mut RenderTarget,
        y: f32,
        x0: f32,
        x1: f32,
        color: BGRA8,
    );
    fn stroke_triangle(&mut self, target: &mut RenderTarget, vertices: &[Point2; 3], color: BGRA8) {
        self.stroke_polygon(target, vertices, color);
    }
    fn fill_triangle(&mut self, target: &mut RenderTarget, vertices: &[Point2; 3], color: BGRA8);

    fn stroke_polygon(&mut self, target: &mut RenderTarget, vertices: &[Point2], color: BGRA8) {
        self.stroke_polyline(target, Vec2::ZERO, vertices, color);
        self.line(
            target,
            vertices[0].x,
            vertices[0].y,
            vertices[vertices.len() - 1].x,
            vertices[vertices.len() - 1].y,
            color,
        );
    }

    fn stroke_polyline(
        &mut self,
        target: &mut RenderTarget,
        offset: Vec2,
        vertices: &[Point2],
        color: BGRA8,
    ) {
        let mut last = vertices[0];
        for point in &vertices[1..] {
            self.line(
                target,
                offset.x + last.x,
                offset.y + last.y,
                offset.x + point.x,
                offset.y + point.y,
                color,
            );
            last = *point;
        }
    }

    fn stroke_whrectangle(
        &mut self,
        target: &mut RenderTarget,
        pos: Point2,
        size: Vec2,
        color: BGRA8,
    ) {
        self.stroke_polygon(
            target,
            &[
                pos,
                Point2::new(pos.x + size.x, pos.y),
                pos + size,
                Point2::new(pos.x, pos.y + size.y),
            ],
            color,
        )
    }

    fn polygon_reset(&mut self, offset: Vec2);
    fn polygon_add_polyline(&mut self, vertices: &[Point2], winding: bool);
    fn polygon_fill(&mut self, target: &mut RenderTarget, color: BGRA8);

    fn blit(
        &mut self,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
        texture: &Texture,
        color: BGRA8,
    );

    fn blur_prepare(&mut self, width: u32, height: u32, sigma: f32);
    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &Texture);
    fn blur_execute(&mut self, target: &mut RenderTarget, dx: i32, dy: i32, color: [u8; 3]);
}
