use std::mem::MaybeUninit;

use polygon::NonZeroPolygonRasterizer;

use crate::{
    color::BGRA8,
    math::{Point2f, Rect2f, Vec2f},
};

pub(crate) mod polygon;
pub(crate) mod sw;
#[cfg(feature = "wgpu")]
pub mod wgpu;

#[derive(Debug, Clone, Copy)]
pub(crate) enum PixelFormat {
    Mono,
    Bgra,
}

impl PixelFormat {
    pub(crate) fn width(&self) -> u8 {
        match self {
            PixelFormat::Mono => 1,
            PixelFormat::Bgra => 4,
        }
    }
}

enum RenderTargetInner<'a> {
    Software(sw::RenderTargetImpl<'a>),
    SoftwareTexture(sw::TextureRenderTargetImpl),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::RenderTargetImpl),
    #[cfg(feature = "wgpu")]
    // For zero-sized renders, TODO: move this logic into RenderTargetImpl
    WgpuEmpty,
}

impl RenderTargetInner<'_> {
    fn variant_name(&self) -> &'static str {
        match self {
            Self::Software(_) => "software",
            Self::SoftwareTexture(_) => "software texture",
            #[cfg(feature = "wgpu")]
            Self::Wgpu(_) | Self::WgpuEmpty => "wgpu",
        }
    }
}

pub struct RenderTarget<'a>(RenderTargetInner<'a>);

impl RenderTarget<'_> {
    pub(crate) fn width(&self) -> u32 {
        match &self.0 {
            // TODO: Make these fields private and have all the impls define accessors for them
            RenderTargetInner::Software(sw) => sw.width,
            RenderTargetInner::SoftwareTexture(sw_tex) => sw_tex.width,
            #[cfg(feature = "wgpu")]
            RenderTargetInner::Wgpu(wgpu) => wgpu.tex.width(),
            #[cfg(feature = "wgpu")]
            RenderTargetInner::WgpuEmpty => 0,
        }
    }

    pub(crate) fn height(&self) -> u32 {
        match &self.0 {
            // TODO: Make these fields private and have all the impls define accessors for them
            RenderTargetInner::Software(sw) => sw.height,
            RenderTargetInner::SoftwareTexture(sw_tex) => sw_tex.height,
            #[cfg(feature = "wgpu")]
            RenderTargetInner::Wgpu(wgpu) => wgpu.tex.height(),
            #[cfg(feature = "wgpu")]
            RenderTargetInner::WgpuEmpty => 0,
        }
    }
}

#[derive(Clone)]
enum TextureInner<'a> {
    Software(sw::TextureImpl<'a>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::TextureImpl),
}

impl TextureInner<'_> {
    fn variant_name(&self) -> &'static str {
        match self {
            TextureInner::Software(_) => "software",
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(_) => "wgpu",
        }
    }
}

#[derive(Clone)]
pub(crate) struct Texture<'a>(TextureInner<'a>);

impl Texture<'_> {
    pub(crate) fn width(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.width,
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu::TextureImpl { tex }) => {
                tex.as_ref().map_or(0, |tex| tex.width())
            }
        }
    }

    pub(crate) fn height(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.height,
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu::TextureImpl { tex }) => {
                tex.as_ref().map_or(0, |tex| tex.height())
            }
        }
    }
}

pub(crate) trait Rasterizer {
    // Used for display debug information
    fn name(&self) -> &'static str;
    fn adapter_info_string(&self) -> Option<String>;

    #[allow(clippy::type_complexity)]
    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
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
    ) {
        self.line(target, Point2f::new(x0, y), Point2f::new(x1, y), color);
    }

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
    fn fill_axis_aligned_antialias_rect(
        &mut self,
        target: &mut RenderTarget,
        rect: Rect2f,
        color: BGRA8,
    ) {
        self.fill_axis_aligned_rect(target, rect, color);
    }

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
