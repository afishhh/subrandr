use std::{any::Any, fmt::Write, mem::MaybeUninit};

use thiserror::Error;
use util::{math::Vec2, AnyError};

use crate::scene::SceneNode;

pub mod sw;
#[cfg(feature = "wgpu")]
pub mod wgpu;

#[derive(Debug, Clone, Copy)]
pub enum PixelFormat {
    Mono,
    Bgra,
}

impl PixelFormat {
    pub fn width(&self) -> u8 {
        match self {
            PixelFormat::Mono => 1,
            PixelFormat::Bgra => 4,
        }
    }
}

enum RenderTargetInner<'a> {
    Software(sw::RenderTarget<'a>),
    #[cfg(feature = "wgpu")]
    Wgpu(Box<wgpu::RenderTarget>),
}

impl RenderTargetInner<'_> {
    fn variant_name(&self) -> &'static str {
        match self {
            Self::Software(_) => "software",
            #[cfg(feature = "wgpu")]
            Self::Wgpu(_) => "wgpu",
        }
    }
}

pub struct RenderTarget<'a>(RenderTargetInner<'a>);

impl RenderTarget<'_> {
    pub fn width(&self) -> u32 {
        match &self.0 {
            RenderTargetInner::Software(sw) => sw.width(),
            #[cfg(feature = "wgpu")]
            RenderTargetInner::Wgpu(wgpu) => wgpu.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match &self.0 {
            RenderTargetInner::Software(sw) => sw.height(),
            #[cfg(feature = "wgpu")]
            RenderTargetInner::Wgpu(wgpu) => wgpu.height(),
        }
    }
}

impl<'a> From<sw::RenderTarget<'a>> for RenderTarget<'a> {
    fn from(value: sw::RenderTarget<'a>) -> Self {
        Self(RenderTargetInner::Software(value))
    }
}

#[derive(Clone)]
enum TextureInner {
    Software(sw::Texture<'static>),
    #[cfg(feature = "wgpu")]
    Wgpu(wgpu::Texture),
}

impl TextureInner {
    fn variant_name(&self) -> &'static str {
        match self {
            TextureInner::Software(_) => "software",
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(_) => "wgpu",
        }
    }
}

#[derive(Clone)]
pub struct Texture(TextureInner);

impl Texture {
    pub fn memory_footprint(&self) -> usize {
        match &self.0 {
            TextureInner::Software(sw) => sw.memory_footprint(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu) => wgpu.memory_footprint(),
        }
    }

    pub fn width(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.width(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu) => wgpu.width(),
        }
    }

    pub fn height(&self) -> u32 {
        match &self.0 {
            TextureInner::Software(sw) => sw.height(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu) => wgpu.height(),
        }
    }

    pub fn is_mono(&self) -> bool {
        match &self.0 {
            TextureInner::Software(sw) => sw.is_mono(),
            #[cfg(feature = "wgpu")]
            TextureInner::Wgpu(wgpu) => wgpu.is_mono(),
        }
    }
}

#[derive(Debug, Error)]
enum SceneRenderErrorInner {
    #[error(transparent)]
    ToBitmaps(AnyError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct SceneRenderError(#[from] SceneRenderErrorInner);

pub trait Rasterizer {
    // Used for displaying debug information
    fn name(&self) -> &'static str;
    fn write_debug_info(&self, _writer: &mut dyn Write) -> std::fmt::Result {
        Ok(())
    }

    /// Creates a new texture via memory-mapped initialization.
    ///
    /// # Safety
    ///
    /// `callback` must initialize the entire buffer passed to it before returning.
    #[allow(clippy::type_complexity)]
    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> Texture;

    /// Creates a new atlased texture via memory-mapped initialization.
    ///
    /// # Safety
    ///
    /// `callback` must initialize the entire buffer passed to it before returning.
    // TODO: Merge into create_texture_mapped as parameter?
    #[allow(clippy::type_complexity)]
    unsafe fn create_packed_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> Texture {
        self.create_texture_mapped(width, height, format, callback)
    }

    fn blur_texture(&mut self, texture: &Texture, blur_sigma: f32) -> BlurOutput;

    fn render_scene(
        &mut self,
        target: &mut RenderTarget,
        scene: &[SceneNode],
        user_data: &(dyn Any + 'static),
    ) -> Result<(), SceneRenderError>;
}

pub struct BlurOutput {
    pub padding: Vec2<u32>,
    pub texture: Texture,
}
