use std::{fmt::Write, mem::MaybeUninit};

use thiserror::Error;
use util::{math::Vec2, AnyError};

pub mod sw;

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

impl From<sw::Texture<'static>> for Texture {
    fn from(value: sw::Texture<'static>) -> Self {
        Self(TextureInner::Software(value))
    }
}

#[derive(Clone)]
enum TextureInner {
    Software(sw::Texture<'static>),
}

impl TextureInner {
    fn variant_name(&self) -> &'static str {
        match self {
            TextureInner::Software(_) => "software",
        }
    }
}

#[derive(Clone)]
pub struct Texture(TextureInner);

impl Texture {
    pub(crate) fn memory_footprint(&self) -> usize {
        match &self.0 {
            TextureInner::Software(sw) => sw.memory_footprint(),
        }
    }

    pub(crate) fn is_mono(&self) -> bool {
        match &self.0 {
            TextureInner::Software(sw) => matches!(sw.format(), PixelFormat::Mono),
        }
    }
}

#[derive(Debug, Error)]
enum SceneRenderErrorInner {
    #[error(transparent)]
    External(AnyError),
}

#[derive(Debug, Error)]
#[error(transparent)]
pub struct SceneRenderError(#[from] SceneRenderErrorInner);

// TODO: Reconsider trait/naming, now that we have everything in `Scene` this is
//       only really used for uploading textures and writing debug info.
pub trait Rasterizer {
    // Used for displaying debug information
    fn name(&self) -> &'static str;
    fn write_debug_info(&self, _writer: &mut dyn Write) -> std::fmt::Result {
        Ok(())
    }

    /// Creates a new empty texture with format [`PixelFormat::Mono`].
    fn empty_mono_texture(&self) -> Texture;

    /// Creates a new texture via memory-mapped initialization.
    ///
    /// # Safety
    ///
    /// `callback` must initialize the entire buffer passed to it before returning.
    #[allow(clippy::type_complexity)]
    unsafe fn create_texture_mapped(
        &mut self,
        size: Vec2<u32>,
        format: PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(sw::RenderTargetView<MaybeUninit<u8>>) + '_>,
    ) -> Texture;
}
