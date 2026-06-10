use std::{
    collections::HashMap,
    hash::Hash,
    mem::MaybeUninit,
    rc::{Rc, Weak},
};

use log::{trace, LogContext};
use util::{
    cache::{Cache, CacheConfiguration, CacheValue},
    math::{I26Dot6, Point2, Rect2, Vec2},
    rc::{Arc, UniqueArc},
    slice_assume_init_mut,
};

use crate::{
    color::{Premultiplied, Premultiply, BGRA8},
    rasterizer::SceneRenderErrorInner,
    scene::{
        Bitmap, BitmapFilter, ExternalSubscene, FilledRect, FixedS, Rect2S, Scene, SceneFilter,
        SceneNode, SubsceneKind,
    },
    PixelFormat, SceneRenderError,
};

mod blit;
pub(super) mod blur;
use blur::*;
mod scale;
mod strip;
pub use strip::*;

trait DrawPixel: Copy + Sized {
    fn scale_alpha(self, scale: u8) -> Self;
}

impl DrawPixel for Premultiplied<BGRA8> {
    fn scale_alpha(self, scale: u8) -> Self {
        self.mul_alpha(scale)
    }
}

trait BlendMode<P: DrawPixel> {
    fn blend(dst: &mut P, src: P);
}

struct BlendOver;
impl BlendMode<Premultiplied<BGRA8>> for BlendOver {
    fn blend(dst: &mut Premultiplied<BGRA8>, src: Premultiplied<BGRA8>) {
        *dst = src.blend_over(*dst)
    }
}

struct BlendSet;
impl BlendMode<Premultiplied<BGRA8>> for BlendSet {
    fn blend(dst: &mut Premultiplied<BGRA8>, src: Premultiplied<BGRA8>) {
        *dst = src;
    }
}

unsafe fn horizontal_line_unchecked<B: BlendMode<P>, P: DrawPixel>(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [P],
    width: i32,
    color: P,
) {
    for x in x0.clamp(0, width)..x1.clamp(0, width) {
        B::blend(offset_buffer.get_unchecked_mut(x as usize), color);
    }
}

unsafe fn vertical_line_unchecked<B: BlendMode<P>, P: DrawPixel>(
    y0: i32,
    y1: i32,
    offset_buffer: &mut [P],
    height: i32,
    stride: i32,
    color: P,
) {
    for y in y0.clamp(0, height)..y1.clamp(0, height) {
        B::blend(
            offset_buffer.get_unchecked_mut((y * stride) as usize),
            color,
        );
    }
}

// Scuffed Anti-Aliasing™ (SAA)
fn fill_rect<B: BlendMode<P>, P: DrawPixel>(
    mut target: RenderTargetView<P>,
    rect: Rect2S,
    color: P,
) {
    if rect.is_empty() {
        return;
    }

    const AA_THRESHOLD: FixedS = FixedS::from_quotient(1, 64);

    let (left_aa, full_left) = if (rect.min.x - rect.min.x.round()).abs() > AA_THRESHOLD {
        (true, rect.min.x.ceil_to_inner())
    } else {
        (false, rect.min.x.round_to_inner())
    };

    let (right_aa, full_right) = if (rect.max.x - rect.max.x.round()).abs() > AA_THRESHOLD {
        (true, rect.max.x.floor_to_inner())
    } else {
        (false, rect.max.x.round_to_inner())
    };

    let (top_aa_width, full_top) = if (rect.min.y - rect.min.y.round()).abs() > AA_THRESHOLD {
        let top_width = FixedS::ONE - rect.min.y.fract();
        let top_fill = (top_width * 255).round_to_inner() as u8;
        let top_y = rect.min.y.floor_to_inner();
        if top_y >= 0 && top_y < target.height as i32 {
            unsafe {
                horizontal_line_unchecked::<B, _>(
                    full_left,
                    full_right,
                    &mut target.buffer[top_y as usize * target.stride as usize..],
                    target.width as i32,
                    color.scale_alpha(top_fill),
                );
            }
        }
        (top_width, top_y + 1)
    } else {
        (FixedS::ONE, rect.min.y.round_to_inner())
    };

    let (bottom_aa_width, full_bottom) = if (rect.max.y - rect.max.y.round()).abs() > AA_THRESHOLD {
        let bottom_width = rect.max.y.fract();
        let bottom_fill = (bottom_width * 255).round_to_inner() as u8;
        let bottom_y = rect.max.y.floor_to_inner();
        if bottom_y >= 0 && bottom_y < target.height as i32 {
            unsafe {
                horizontal_line_unchecked::<B, _>(
                    full_left,
                    full_right,
                    &mut target.buffer[bottom_y as usize * target.stride as usize..],
                    target.width as i32,
                    color.scale_alpha(bottom_fill),
                );
            }
        }
        (bottom_width, bottom_y)
    } else {
        (FixedS::ONE, rect.max.y.round_to_inner())
    };

    if left_aa {
        let left_fill = (FixedS::ONE - rect.min.x.fract()) * 255;
        let left_x = full_left - 1;
        if left_x >= 0 && left_x < target.width as i32 {
            if let Some(pixel) = target.pixel_at(left_x, full_top - 1) {
                B::blend(
                    pixel,
                    color.scale_alpha((left_fill * top_aa_width).round_to_inner() as u8),
                )
            }

            unsafe {
                vertical_line_unchecked::<B, _>(
                    full_top,
                    full_bottom,
                    &mut target.buffer[left_x as usize..],
                    target.height as i32,
                    target.stride as i32,
                    color.scale_alpha(left_fill.round_to_inner() as u8),
                );
            }

            if let Some(pixel) = target.pixel_at(left_x, full_bottom) {
                B::blend(
                    pixel,
                    color.scale_alpha((left_fill * bottom_aa_width).round_to_inner() as u8),
                );
            }
        }
    }

    if right_aa {
        let right_fill = rect.max.x.fract() * 255;
        let right_x = full_right;
        if right_x >= 0 && right_x < target.width as i32 {
            if let Some(pixel) = target.pixel_at(right_x, full_top - 1) {
                B::blend(
                    pixel,
                    color.scale_alpha((right_fill * top_aa_width).round_to_inner() as u8),
                )
            }

            unsafe {
                vertical_line_unchecked::<B, _>(
                    full_top,
                    full_bottom,
                    &mut target.buffer[right_x as usize..],
                    target.height as i32,
                    target.stride as i32,
                    color.scale_alpha(right_fill.round_to_inner() as u8),
                );
            }

            if let Some(pixel) = target.pixel_at(right_x, full_bottom) {
                B::blend(
                    pixel,
                    color.scale_alpha((right_fill * bottom_aa_width).round_to_inner() as u8),
                );
            }
        }
    }

    for y in full_top.clamp(0, target.height as i32)..full_bottom.clamp(0, target.height as i32) {
        unsafe {
            horizontal_line_unchecked::<B, _>(
                full_left,
                full_right,
                &mut target.buffer[y as usize * target.stride as usize..],
                target.width as i32,
                color,
            );
        }
    }
}

#[track_caller]
fn check_pixel_buffer_dimensions(
    buffer_len: usize,
    width: u32,
    height: u32,
    stride: u32,
    name: &std::fmt::Arguments,
) {
    let Some(n_pixels) = (height as usize).checked_mul(stride as usize) else {
        panic!("Size passed to {name} overflows `height * stride`",);
    };
    assert!(
        buffer_len >= n_pixels,
        "Buffer passed to {name} is too small",
    );
    assert!(
        width <= stride,
        "width passed to {name} is larger than stride",
    );
}

pub struct RenderTargetView<'a, P> {
    buffer: &'a mut [P],
    width: u32,
    height: u32,
    stride: u32,
}

impl<'a, P> RenderTargetView<'a, P> {
    pub fn new(buffer: &'a mut [P], width: u32, height: u32, stride: u32) -> Self {
        check_pixel_buffer_dimensions(
            buffer.len(),
            width,
            height,
            stride,
            &format_args!("{}::new", std::any::type_name::<Self>()),
        );

        Self {
            buffer,
            width,
            height,
            stride,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn stride(&self) -> u32 {
        self.stride
    }

    pub fn reborrow(&mut self) -> RenderTargetView<'_, P> {
        RenderTargetView {
            buffer: &mut *self.buffer,
            ..*self
        }
    }

    #[inline]
    fn pixel_at(&mut self, x: i32, y: i32) -> Option<&mut P> {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return None;
        };

        if x >= self.width || y >= self.height {
            return None;
        }

        Some(unsafe {
            self.buffer
                .get_unchecked_mut(y as usize * self.stride as usize + x as usize)
        })
    }

    pub fn row(&mut self, y: i32) -> Option<&mut [P]> {
        let y = u32::try_from(y).ok()?;

        if y >= self.height {
            return None;
        }

        let start = y as usize * self.stride as usize;
        Some(unsafe {
            self.buffer
                .get_unchecked_mut(start..start + self.width as usize)
        })
    }
}

impl<P> RenderTargetView<'_, MaybeUninit<P>> {
    fn as_bytes(&mut self) -> RenderTargetView<'_, MaybeUninit<u8>> {
        let size = std::mem::size_of_val(&*self.buffer);
        let Some(byte_width) = self.width.checked_mul(std::mem::size_of::<P>() as u32) else {
            panic!("Width overflowed u32 while casting RenderTargetView to byte pixels");
        };
        let Some(byte_stride) = self.stride.checked_mul(std::mem::size_of::<P>() as u32) else {
            panic!("Stride overflowed u32 while casting RenderTargetView to byte pixels");
        };

        RenderTargetView {
            buffer: unsafe {
                std::slice::from_raw_parts_mut(self.buffer.as_mut_ptr().cast(), size)
            },
            width: byte_width,
            height: self.height,
            stride: byte_stride,
        }
    }
}

pub type RenderTarget<'a> = RenderTargetView<'a, Premultiplied<BGRA8>>;

#[derive(Clone)]
enum TextureData<'a> {
    OwnedMono(Arc<[u8]>),
    OwnedBgra(Arc<[Premultiplied<BGRA8>]>),
    BorrowedMono(&'a [u8]),
}

impl TextureData<'_> {
    fn as_ref(&self) -> TextureDataRef<'_> {
        match self {
            Self::OwnedMono(a) => TextureDataRef::Mono(a),
            Self::OwnedBgra(bgra) => TextureDataRef::Bgra(bgra),
            Self::BorrowedMono(a) => TextureDataRef::Mono(a),
        }
    }
}

impl Hash for TextureData<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        core::mem::discriminant(self).hash(state);
        match self {
            Self::OwnedMono(mono) => Arc::hash_ptr(mono, state),
            Self::OwnedBgra(bgra) => Arc::hash_ptr(bgra, state),
            Self::BorrowedMono(mono) => mono.as_ptr().hash(state),
        }
    }
}

impl PartialEq for TextureData<'_> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::OwnedMono(l), Self::OwnedMono(r)) => Arc::ptr_eq(l, r),
            (Self::OwnedBgra(l), Self::OwnedBgra(r)) => Arc::ptr_eq(l, r),
            (Self::BorrowedMono(l), Self::BorrowedMono(r)) => std::ptr::eq(l, r),
            _ => false,
        }
    }
}

impl Eq for TextureData<'_> {}

#[derive(Clone, Copy)]
enum TextureDataRef<'a> {
    Mono(&'a [u8]),
    Bgra(&'a [Premultiplied<BGRA8>]),
}

impl TextureDataRef<'_> {
    fn hash_content<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match *self {
            TextureDataRef::Mono(mono) => mono.hash(state),
            TextureDataRef::Bgra(bgra) => bgra.hash(state),
        }
    }

    fn eq_content(&self, other: &Self) -> bool {
        match (*self, *other) {
            (TextureDataRef::Mono(mono), TextureDataRef::Mono(other_mono)) => mono == other_mono,
            (TextureDataRef::Bgra(bgra), TextureDataRef::Bgra(other_bgra)) => bgra == other_bgra,
            _ => false,
        }
    }
}

trait TexturePixel: Sized {
    fn to_texture_data(owned: UniqueArc<[Self]>) -> TextureData<'static>;
}

impl TexturePixel for Premultiplied<BGRA8> {
    fn to_texture_data(owned: UniqueArc<[Self]>) -> TextureData<'static> {
        TextureData::OwnedBgra(UniqueArc::into_shared(owned))
    }
}

impl TexturePixel for u8 {
    fn to_texture_data(owned: UniqueArc<[Self]>) -> TextureData<'static> {
        TextureData::OwnedMono(UniqueArc::into_shared(owned))
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Texture<'a> {
    data: TextureData<'a>,
    width: u32,
    height: u32,
}

impl Texture<'static> {
    fn new_with<P: TexturePixel>(
        size: Vec2<u32>,
        render: impl FnOnce(RenderTargetView<P>),
    ) -> Self {
        unsafe {
            Self::new_with_uninit(size, |target| {
                let len = target.buffer.len();
                target.buffer.as_mut_ptr().write_bytes(0, len);
                render(RenderTargetView {
                    buffer: slice_assume_init_mut(target.buffer),
                    width: target.width,
                    height: target.height,
                    stride: target.stride,
                })
            })
        }
    }

    unsafe fn new_with_uninit<P: TexturePixel>(
        size: Vec2<u32>,
        fill: impl FnOnce(RenderTargetView<MaybeUninit<P>>),
    ) -> Self {
        let mut buffer = UniqueArc::new_uninit_slice(size.x as usize * size.y as usize);

        fill(RenderTargetView::new(&mut buffer, size.x, size.y, size.x));

        Texture {
            width: size.x,
            height: size.y,
            data: P::to_texture_data(unsafe { UniqueArc::assume_init(buffer) }),
        }
    }

    const EMPTY_MONO: Texture<'static> = Texture {
        width: 0,
        height: 0,
        data: TextureData::BorrowedMono(&[]),
    };

    const SINGLE_FILLED_MONO_PIXEL: Texture<'static> = Texture {
        width: 1,
        height: 1,
        data: TextureData::BorrowedMono(&[u8::MAX]),
    };
}

impl<'a> Texture<'a> {
    pub fn new_borrowed_mono(data: &'a [u8], width: u32, height: u32) -> Self {
        check_pixel_buffer_dimensions(
            data.len(),
            width,
            height,
            width,
            &format_args!("{}::new_borrowed_mono", std::any::type_name::<Self>()),
        );

        Self {
            data: TextureData::BorrowedMono(data),
            width,
            height,
        }
    }

    pub fn size(&self) -> Vec2<u32> {
        Vec2::new(self.width, self.height)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn format(&self) -> PixelFormat {
        match &self.data {
            TextureData::OwnedMono(_) => PixelFormat::Mono,
            TextureData::OwnedBgra(_) | TextureData::BorrowedMono(_) => PixelFormat::Bgra,
        }
    }

    fn hash_content<H: std::hash::Hasher>(&self, state: &mut H) {
        self.width.hash(state);
        self.height.hash(state);
        self.data.as_ref().hash_content(state);
    }

    fn eq_content(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && TextureDataRef::eq_content(&self.data.as_ref(), &other.data.as_ref())
    }

    pub(crate) fn memory_footprint(&self) -> usize {
        match &self.data {
            TextureData::OwnedMono(mono) => mono.len(),
            TextureData::OwnedBgra(bgra) => bgra.len() * 4,
            TextureData::BorrowedMono(_) => 0,
        }
    }
}

impl std::fmt::Debug for Texture<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}x{} {} texture]@{:?}",
            self.width,
            self.height,
            match self.data.as_ref() {
                TextureDataRef::Mono(_) => "A8",
                TextureDataRef::Bgra(_) => "BGRA8",
            },
            match self.data.as_ref() {
                TextureDataRef::Mono(s) => s.as_ptr() as *const (),
                TextureDataRef::Bgra(s) => s.as_ptr() as *const (),
            }
        )
    }
}

fn unwrap_sw_texture(texture: &super::Texture) -> &Texture<'static> {
    match &texture.0 {
        super::TextureInner::Software(texture) => texture,
        #[expect(unreachable_patterns)]
        target => panic!(
            "Incompatible texture {:?} passed to software rasterizer",
            target.variant_name()
        ),
    }
}

const RASTER_CACHE_CONFIGURATION: CacheConfiguration = CacheConfiguration {
    trim_memory_threshold: 8 * 1024 * 1024,
    trim_kept_generations: 3,
};

#[derive(PartialEq, Eq, Hash)]
#[expect(clippy::enum_variant_names)]
enum RasterCacheKey {
    SubsceneTexture {
        subscene: SubsceneCacheKey,
        active_color: BGRA8,
    },
    BlurTexture(Texture<'static>, I26Dot6),
    ScaleTexture {
        texture: Texture<'static>,
        dst_size: Vec2<u32>,
        src_off: Vec2<u32>,
        src_size: Vec2<u32>,
    },
}

enum SubsceneCacheKey {
    External(Weak<dyn ExternalSubscene>),
    Scene(Weak<[SceneNode]>),
}

impl SubsceneCacheKey {
    fn addr(&self) -> usize {
        match self {
            SubsceneCacheKey::External(weak) => Weak::as_ptr(weak).addr(),
            SubsceneCacheKey::Scene(scene) => Weak::as_ptr(scene).addr(),
        }
    }
}

impl PartialEq for SubsceneCacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.addr() == other.addr()
    }
}

impl Eq for SubsceneCacheKey {}

impl Hash for SubsceneCacheKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.addr().hash(state);
    }
}

impl From<&SubsceneKind> for SubsceneCacheKey {
    fn from(value: &SubsceneKind) -> Self {
        match value {
            SubsceneKind::External(external) => SubsceneCacheKey::External(Rc::downgrade(external)),
            SubsceneKind::Scene(scene) => SubsceneCacheKey::Scene(Rc::downgrade(&scene.0)),
        }
    }
}

struct CachedSubsceneTexture((Vec2<i32>, Texture<'static>));

impl CacheValue for CachedSubsceneTexture {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>() + self.0 .1.memory_footprint()
    }
}

struct CachedSubscenePieces {
    pieces: Vec<OutputPiece>,
    bbox: Rect2<i32>,
}

impl CacheValue for CachedSubscenePieces {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of_val::<[_]>(&self.pieces)
            + self
                .pieces
                .iter()
                .map(|piece| match &piece.content {
                    OutputPieceContent::Texture(bitmap) => bitmap.texture.memory_footprint(),
                    OutputPieceContent::Strips(strips) => strips.strips.memory_footprint(),
                    OutputPieceContent::Rect(_) => 0,
                })
                .sum::<usize>()
    }
}

impl CacheValue for BlurOutput {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>() + self.texture.memory_footprint()
    }
}

struct CachedTexture(Texture<'static>);

impl CacheValue for CachedTexture {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>() + self.0.memory_footprint()
    }
}

pub struct Rasterizer {
    blurer: blur::Blurer,
    strip_rasterizer: strip::StripRasterizer,
    // NOTE: This is an `Rc` only to workaround borrowing rules.
    //       No other live references shall exist outside a running render pass.
    cache: Rc<RasterCache>,
}

struct RasterCache(Cache<RasterCacheKey>);

impl Rasterizer {
    pub fn new() -> Self {
        Self {
            blurer: blur::Blurer::new(),
            strip_rasterizer: strip::StripRasterizer::new(),
            cache: Rc::new(RasterCache(Cache::new(RASTER_CACHE_CONFIGURATION))),
        }
    }

    pub fn advance_cache_generation(&mut self) {
        Rc::get_mut(&mut self.cache)
            .expect("live references to `sw::Rasterizer` cache exist outside raster pass or generation advanced during raster pass").0
            .advance_generation();
    }

    pub fn blit(
        &self,
        target: &mut RenderTarget,
        pos: Point2<i32>,
        texture: &Texture,
        color: BGRA8,
    ) {
        match texture.data.as_ref() {
            TextureDataRef::Mono(source) => {
                blit::blit_mono(
                    target.reborrow(),
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color,
                );
            }
            TextureDataRef::Bgra(source) => {
                blit::blit_bgra(
                    target.reborrow(),
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color.a,
                );
            }
        }
    }

    fn blit_texture_filtered(
        &self,
        target: &mut RenderTarget,
        pos: Point2<i32>,
        texture: &Texture,
        filter: Option<BitmapFilter>,
        color: BGRA8,
    ) {
        match (filter, texture.data.as_ref()) {
            (None, _) | (Some(BitmapFilter::ExtractAlpha), TextureDataRef::Mono(_)) => {
                self.blit(target, pos, texture, color)
            }
            (Some(BitmapFilter::ExtractAlpha), TextureDataRef::Bgra(source)) => {
                blit::blit_xxxa_to_bgra(
                    target.reborrow(),
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color,
                );
            }
        }
    }

    fn copy_texture_filtered(
        &self,
        target: &mut RenderTarget,
        pos: Point2<i32>,
        texture: &Texture,
        filter: Option<BitmapFilter>,
        color: BGRA8,
    ) {
        match (texture.data.as_ref(), filter) {
            (TextureDataRef::Mono(mono), Some(BitmapFilter::ExtractAlpha) | None) => {
                blit::cvt_mono_to_bgra(
                    target.reborrow(),
                    mono,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color,
                );
            }
            (TextureDataRef::Bgra(bgra), Some(BitmapFilter::ExtractAlpha)) => {
                blit::cvt_xxxa_to_bgra(
                    target.reborrow(),
                    bgra,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color,
                );
            }
            (TextureDataRef::Bgra(source), None) => {
                blit::cvt_bgra_to_bgra(
                    target.reborrow(),
                    source,
                    texture.width as usize,
                    texture.width as usize,
                    texture.height as usize,
                    pos.x as isize,
                    pos.y as isize,
                    color.a,
                );
            }
        }
    }

    fn scale_texture_uncached(
        &self,
        texture: &Texture,
        dst_size: Vec2<u32>,
        src_off: Vec2<u32>,
        src_size: Vec2<u32>,
    ) -> Texture<'static> {
        match texture.data.as_ref() {
            TextureDataRef::Mono(mono) => unsafe {
                Texture::new_with_uninit(dst_size, |target| {
                    scale::scale_mono(
                        target,
                        mono,
                        texture.width as usize,
                        texture.width,
                        texture.height,
                        Vec2::new(src_off.x as i32, src_off.y as i32),
                        Vec2::new(src_size.x as i32, src_size.y as i32),
                    )
                })
            },
            TextureDataRef::Bgra(bgra) => unsafe {
                Texture::new_with_uninit(dst_size, |target| {
                    scale::scale_bgra(
                        target,
                        bgra,
                        texture.width as usize,
                        texture.width,
                        texture.height,
                        Vec2::new(src_off.x as i32, src_off.y as i32),
                        Vec2::new(src_size.x as i32, src_size.y as i32),
                    )
                })
            },
        }
    }

    pub fn scale_texture(
        &self,
        texture: &Texture<'static>,
        dst_size: Vec2<u32>,
        src_off: Vec2<u32>,
        src_size: Vec2<u32>,
    ) -> Texture<'static> {
        self.cache
            .get_or_scale_texture(texture, dst_size, src_off, src_size, self)
            .clone()
    }
}

struct BlurOutput {
    padding: Vec2<u32>,
    texture: Texture<'static>,
}

impl BlurOutput {
    const EMPTY: Self = Self {
        padding: Vec2::ZERO,
        texture: Texture::EMPTY_MONO,
    };
}

impl Rasterizer {
    fn blur_texture(&mut self, texture: &Texture<'_>, blur_sigma: f32) -> BlurOutput {
        let is_box_blur;
        let kernel = if blur_sigma > 2.0 {
            is_box_blur = true;
            BlurKernel::Box(BoxBlurKernel::from_gaussian_stddev(blur_sigma))
        } else {
            is_box_blur = false;
            BlurKernel::Gaussian(GaussianBlurKernel::new(blur_sigma))
        };

        self.blurer
            .prepare(texture.width as usize, texture.height as usize, kernel);

        let dx = self.blurer.padding() as i32;
        let dy = self.blurer.padding() as i32;
        let width = self.blurer.width();
        let height = self.blurer.height();
        let blurer_front_view = RenderTargetView::new(
            self.blurer.front_mut(),
            width as u32,
            height as u32,
            width as u32,
        );
        match texture.data.as_ref() {
            TextureDataRef::Mono(source) => blit::copy_mono_to_float(
                blurer_front_view,
                source,
                texture.width as usize,
                texture.width as usize,
                texture.height as usize,
                dx as isize,
                dy as isize,
            ),
            TextureDataRef::Bgra(source) => blit::copy_xxxa_to_float(
                blurer_front_view,
                source,
                texture.width as usize,
                texture.width as usize,
                texture.height as usize,
                dx as isize,
                dy as isize,
            ),
        }

        if is_box_blur {
            self.blurer.blur_horizontal();
            self.blurer.blur_horizontal();
            self.blurer.blur_horizontal();
            self.blurer.blur_vertical();
            self.blurer.blur_vertical();
            self.blurer.blur_vertical();
        } else {
            self.blurer.blur_horizontal();
            self.blurer.blur_vertical();
        }

        let result = Texture::new_with(Vec2::new(width as u32, height as u32), |target| {
            blit::copy_float_to_mono(target, self.blurer.front(), width, width, height, 0, 0);
        });

        BlurOutput {
            padding: Vec2::splat(self.blurer.padding() as u32),
            texture: result,
        }
    }
}

impl super::Rasterizer for Rasterizer {
    fn name(&self) -> &'static str {
        "software"
    }

    fn write_debug_info(&self, writer: &mut dyn std::fmt::Write) -> std::fmt::Result {
        let stats = self.cache.0.stats();
        let (footprint_divisor, footprint_suffix) =
            util::human_size_suffix(stats.total_memory_footprint);

        writeln!(writer, "== raster cache stats ==")?;
        writeln!(
            writer,
            "approximate memory footprint: {:.3}{footprint_suffix}B",
            stats.total_memory_footprint as f32 / footprint_divisor as f32
        )?;
        writeln!(writer, "total entries: {}", stats.total_entries)?;
        writeln!(writer, "current generation: {}", stats.generation)?;

        Ok(())
    }

    fn empty_mono_texture(&self) -> super::Texture {
        super::Texture(super::TextureInner::Software(Texture::EMPTY_MONO))
    }

    unsafe fn create_texture_mapped(
        &mut self,
        size: Vec2<u32>,
        format: PixelFormat,
        callback: Box<dyn FnOnce(RenderTargetView<MaybeUninit<u8>>) + '_>,
    ) -> super::Texture {
        super::Texture(super::TextureInner::Software({
            match format {
                PixelFormat::Mono => Texture::new_with_uninit(size, callback),
                PixelFormat::Bgra => {
                    Texture::new_with_uninit::<Premultiplied<BGRA8>>(size, |mut target| {
                        callback(target.as_bytes())
                    })
                }
            }
        }))
    }
}

#[derive(Clone)]
pub struct OutputBitmap<'a> {
    pub texture: Texture<'a>,
    pub filter: Option<BitmapFilter>,
    pub color: BGRA8,
}

impl OutputBitmap<'_> {
    fn compare_by_content(&self) -> bool {
        (self.texture.width() as usize * self.texture.height() as usize) <= 64
    }
}

impl Hash for OutputBitmap<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        if self.compare_by_content() {
            self.texture.hash_content(state);
        } else {
            self.texture.hash(state);
        }
        self.filter.hash(state);
        self.color.hash(state);
    }
}

impl PartialEq for OutputBitmap<'_> {
    fn eq(&self, other: &Self) -> bool {
        (if self.compare_by_content() {
            Texture::eq_content(&self.texture, &other.texture)
        } else {
            Texture::eq(&self.texture, &other.texture)
        }) && self.filter == other.filter
            && self.color == other.color
    }
}

impl Eq for OutputBitmap<'_> {}

#[derive(Clone)]
pub struct OutputStrips {
    pub strips: Rc<Strips>,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct OutputRect {
    pub rect: Rect2S,
    pub color: BGRA8,
}

#[derive(Clone)]
pub enum OutputPieceContent {
    Texture(OutputBitmap<'static>),
    Strips(OutputStrips),
    Rect(OutputRect),
}

#[derive(Clone)]
pub struct OutputPiece {
    pub pos: Point2<i32>,
    pub size: Vec2<u32>,
    pub content: OutputPieceContent,
}

impl OutputPiece {
    fn from_bitmap(bitmap: Bitmap, active_color: BGRA8) -> Self {
        let texture = unwrap_sw_texture(&bitmap.texture);
        Self {
            pos: bitmap.pos,
            size: bitmap.scaled_size,
            content: {
                OutputPieceContent::Texture(OutputBitmap {
                    texture: texture.clone(),
                    filter: bitmap.filter,
                    color: bitmap.color.compute(active_color),
                })
            },
        }
    }
}

impl Rasterizer {
    fn render_scene_pieces_impl(
        &mut self,
        log: &LogContext,
        active_color: BGRA8,
        scene: &[SceneNode],
        cull_rect: Rect2S,
        on_piece: &mut dyn FnMut(&mut Rasterizer, OutputPiece),
    ) -> Result<(), SceneRenderError> {
        for node in scene {
            if !node.bounding_box().intersects(&cull_rect) {
                continue;
            }

            match node {
                SceneNode::Bitmap(bitmap) => {
                    on_piece(self, OutputPiece::from_bitmap(bitmap.clone(), active_color));
                }
                &SceneNode::FilledRect(FilledRect { rect, color }) => {
                    let floored_pos =
                        Point2::new(rect.min.x.floor_to_inner(), rect.min.y.floor_to_inner());
                    on_piece(
                        self,
                        OutputPiece {
                            pos: floored_pos,
                            size: Vec2::new(
                                (rect.max.x - floored_pos.x).ceil_to_inner() as u32,
                                (rect.max.y - floored_pos.y).ceil_to_inner() as u32,
                            ),
                            content: OutputPieceContent::Rect(OutputRect {
                                rect: Rect2S {
                                    min: rect.min - floored_pos.to_vec(),
                                    max: rect.max - floored_pos.to_vec(),
                                },
                                color: color.compute(active_color),
                            }),
                        },
                    );
                }
                SceneNode::FilledOutline(outline) => {
                    let (pos, size, strips) = outline.to_strips(&mut self.strip_rasterizer);
                    on_piece(
                        self,
                        OutputPiece {
                            pos,
                            size,
                            content: OutputPieceContent::Strips(OutputStrips {
                                strips: Rc::new(strips),
                                color: outline.color.compute(active_color),
                            }),
                        },
                    )
                }
                SceneNode::StrokedPolyline(polyline) => {
                    let (pos, size, strips) = polyline.to_strips(&mut self.strip_rasterizer);
                    on_piece(
                        self,
                        OutputPiece {
                            pos,
                            size,
                            content: OutputPieceContent::Strips(OutputStrips {
                                strips: Rc::new(strips),
                                color: polyline.color.compute(active_color),
                            }),
                        },
                    )
                }
                SceneNode::Subscene(subscene) => {
                    let cache = self.cache.clone();
                    let new_active_color = subscene.active_color.compute(active_color);
                    if let SubsceneKind::Scene(scene) = &subscene.kind {
                        let mut can_skip_texture = false;

                        can_skip_texture |= subscene.scene_filter.is_none();
                        // Unblurred `ExtractAlpha` from a mono scene is a no-op.
                        can_skip_texture |= matches!(
                            subscene.scene_filter,
                            Some(SceneFilter::ExtractAlpha { blur_stddev }) if blur_stddev == FixedS::ZERO
                        ) && scene.is_mono();

                        if can_skip_texture {
                            let (pieces, _) = cache.get_or_render_scene_pieces(
                                log,
                                scene,
                                new_active_color,
                                self,
                            )?;

                            for piece in pieces {
                                on_piece(
                                    self,
                                    OutputPiece {
                                        pos: piece.pos + subscene.pos.to_vec(),
                                        ..piece.clone()
                                    },
                                );
                            }
                            continue;
                        }
                    }

                    let (off, mut texture) = cache.get_or_render_subscene_texture(
                        log,
                        &subscene.kind,
                        new_active_color,
                        self,
                    )?;

                    let mut pos = subscene.pos + off;
                    let mut output_filter = None;
                    match subscene.scene_filter {
                        Some(SceneFilter::ExtractAlpha { blur_stddev }) => {
                            if blur_stddev == I26Dot6::ZERO {
                                output_filter = Some(BitmapFilter::ExtractAlpha);
                            } else {
                                let output =
                                    cache.get_or_blur_texture(log, &texture, blur_stddev, self);
                                pos -= Vec2::new(output.padding.x as i32, output.padding.y as i32);
                                texture = output.texture.clone();
                            }
                        }
                        None => {}
                    }

                    on_piece(
                        self,
                        OutputPiece {
                            pos,
                            size: texture.size(),
                            content: OutputPieceContent::Texture(OutputBitmap {
                                texture,
                                filter: output_filter,
                                color: new_active_color,
                            }),
                        },
                    );
                }
            }
        }

        Ok(())
    }

    pub fn render_scene_pieces(
        &mut self,
        log: &LogContext,
        scene: &Scene,
        cull_rect: Rect2S,
        on_piece: &mut dyn FnMut(OutputPiece),
    ) -> Result<(), SceneRenderError> {
        self.render_scene_pieces_impl(
            log,
            BGRA8::MAGENTA,
            &scene.0,
            cull_rect,
            &mut move |_r, p| on_piece(p),
        )
    }

    pub fn render_scene(
        &mut self,
        log: &LogContext,
        target: &mut RenderTarget,
        scene: &Scene,
    ) -> Result<(), super::SceneRenderError> {
        target.buffer.fill(Premultiplied(BGRA8::ZERO));

        let cull_rect = Rect2S::new(
            Point2::ZERO,
            Point2::new(
                FixedS::new(target.width as i32),
                FixedS::new(target.height as i32),
            ),
        );
        self.render_scene_pieces_impl(
            log,
            BGRA8::MAGENTA,
            &scene.0,
            cull_rect,
            &mut |r, piece| piece.blend_to(r, target, piece.pos),
        )?;

        Ok(())
    }

    fn pieces_to_texture(
        &mut self,
        pieces: &[OutputPiece],
        bbox: Rect2<i32>,
    ) -> (Vec2<i32>, Texture<'static>) {
        if bbox.is_empty() {
            return (Vec2::ZERO, Texture::EMPTY_MONO);
        }

        (
            bbox.min.to_vec(),
            Texture::new_with(
                Vec2::new(bbox.width() as u32, bbox.height() as u32),
                |mut view| {
                    for piece in pieces {
                        piece.blend_to(self, &mut view, piece.pos - bbox.min.to_vec());
                    }
                },
            ),
        )
    }
}

impl RasterCache {
    fn get_or_render_scene_pieces(
        &self,
        log: &LogContext,
        scene: &Scene,
        active_color: BGRA8,
        rasterizer: &mut Rasterizer,
    ) -> Result<(&[OutputPiece], Rect2<i32>), super::SceneRenderError> {
        self.0
            .get_or_try_insert_with::<CachedSubscenePieces, _>(
                RasterCacheKey::SubsceneTexture {
                    subscene: SubsceneCacheKey::Scene(Rc::downgrade(&scene.0)),
                    active_color,
                },
                || {
                    trace!(log, "Rendering subscene {scene:?} to pieces");

                    let mut pieces = Vec::new();
                    let mut bbox = Rect2::NOTHING;
                    rasterizer.render_scene_pieces_impl(
                        log,
                        active_color,
                        &scene.0,
                        Rect2S::MAX,
                        &mut |_, piece| {
                            bbox.expand_to_rect(piece.rect());
                            pieces.push(piece);
                        },
                    )?;

                    Ok(CachedSubscenePieces { pieces, bbox })
                },
            )
            .map(|CachedSubscenePieces { pieces, bbox }| (&**pieces, *bbox))
    }

    fn get_or_render_subscene_texture(
        &self,
        log: &LogContext,
        kind: &SubsceneKind,
        new_active_color: BGRA8,
        rasterizer: &mut Rasterizer,
    ) -> Result<(Vec2<i32>, Texture<'static>), super::SceneRenderError> {
        match kind {
            SubsceneKind::External(external) => {
                self.0.get_or_try_insert_with::<CachedSubsceneTexture, _>(
                    RasterCacheKey::SubsceneTexture {
                        subscene: kind.into(),
                        active_color: BGRA8::ZERO,
                    },
                    || {
                        trace!(
                            log,
                            "Rendering external subscene [{:?}]@{:?} to texture",
                            util::fmt_from_fn(|x| external.write_debug_name(x)),
                            Rc::as_ptr(external) as *const ()
                        );

                        let (off, texture) = external
                            .rasterize(rasterizer)
                            .map_err(SceneRenderErrorInner::External)?;
                        Ok(CachedSubsceneTexture((
                            off,
                            unwrap_sw_texture(&texture).clone(),
                        )))
                    },
                )
            }
            SubsceneKind::Scene(child_scene) => {
                self.0.get_or_try_insert_with::<CachedSubsceneTexture, _>(
                    RasterCacheKey::SubsceneTexture {
                        subscene: kind.into(),
                        active_color: BGRA8::ZERO,
                    },
                    || {
                        let (pieces, bbox) = self.get_or_render_scene_pieces(
                            log,
                            child_scene,
                            new_active_color,
                            rasterizer,
                        )?;

                        trace!(
                            log,
                            "Rendering pieces of subscene {child_scene:?} to texture",
                        );

                        Ok(CachedSubsceneTexture(
                            rasterizer.pieces_to_texture(pieces, bbox),
                        ))
                    },
                )
            }
        }
        .map(|CachedSubsceneTexture((offset, texture))| (*offset, texture.clone()))
    }

    fn get_or_blur_texture(
        &self,
        log: &LogContext,
        src: &Texture<'static>,
        stddev: I26Dot6,
        rasterizer: &mut Rasterizer,
    ) -> &BlurOutput {
        if src.width() == 0 || src.height() == 0 {
            return &BlurOutput::EMPTY;
        }

        self.0
            .get_or_insert_with(RasterCacheKey::BlurTexture(src.clone(), stddev), || {
                trace!(log, "Blurring {src:?} with σ={stddev}");
                rasterizer.blur_texture(src, stddev.into_f32())
            })
    }

    pub fn get_or_scale_texture(
        &self,
        texture: &Texture<'static>,
        dst_size: Vec2<u32>,
        src_off: Vec2<u32>,
        src_size: Vec2<u32>,
        rasterizer: &Rasterizer,
    ) -> &Texture<'static> {
        let Ok(CachedTexture(result)) = self.0.get_or_try_insert_with(
            RasterCacheKey::ScaleTexture {
                texture: texture.clone(),
                dst_size,
                src_off,
                src_size,
            },
            || {
                Ok::<_, std::convert::Infallible>(CachedTexture(
                    rasterizer.scale_texture_uncached(texture, dst_size, src_off, src_size),
                ))
            },
        );

        result
    }
}

impl OutputPiece {
    fn blend_to(&self, rasterizer: &mut Rasterizer, target: &mut RenderTarget, pos: Point2<i32>) {
        match &self.content {
            OutputPieceContent::Texture(image) => {
                let texture = if self.size == image.texture.size() {
                    &image.texture
                } else {
                    &rasterizer.scale_texture(
                        &image.texture,
                        self.size,
                        Vec2::ZERO,
                        image.texture.size(),
                    )
                };

                rasterizer.blit_texture_filtered(target, pos, texture, image.filter, image.color);
            }
            OutputPieceContent::Strips(OutputStrips { strips, color }) => {
                let pre = color.premultiply();
                strips.blend_to_at(
                    target.reborrow(),
                    |d, s| *d = pre.mul_alpha(s).blend_over(*d),
                    Vec2::new(pos.x, pos.y),
                );
            }
            &OutputPieceContent::Rect(OutputRect { rect, color }) => {
                fill_rect::<BlendOver, _>(
                    target.reborrow(),
                    rect.translate(Vec2::new(pos.x, pos.y)),
                    color.premultiply(),
                );
            }
        }
    }
}

pub enum OutputImage<'a> {
    Texture(OutputBitmap<'a>),
    Rect(OutputRect),
}

impl OutputImage<'_> {
    pub fn rasterize_to(
        &self,
        rasterizer: &mut Rasterizer,
        target: &mut RenderTarget<'_>,
        offset: Point2<i32>,
    ) {
        match self {
            OutputImage::Texture(bitmap) => {
                rasterizer.copy_texture_filtered(
                    target,
                    offset,
                    &bitmap.texture,
                    bitmap.filter,
                    bitmap.color,
                );
            }
            &OutputImage::Rect(OutputRect { rect, color }) => {
                fill_rect::<BlendSet, _>(
                    target.reborrow(),
                    rect.translate(offset.to_vec()),
                    color.premultiply(),
                );
            }
        }
    }

    pub fn rasterize_to_texture(
        &self,
        rasterizer: &mut Rasterizer,
        size: Vec2<u32>,
    ) -> Texture<'static> {
        Texture::new_with(size, |mut target| {
            self.rasterize_to(rasterizer, &mut target, Point2::ZERO)
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OutputInstanceParameters {
    pub dst_pos: Point2<i32>,
    pub dst_size: Vec2<u32>,
    pub src_off: Vec2<u32>,
    pub src_size: Vec2<u32>,
}

impl From<ClipRectIntersection> for OutputInstanceParameters {
    fn from(value: ClipRectIntersection) -> Self {
        Self {
            dst_pos: value.dst_pos,
            dst_size: value.size,
            src_off: value.min_clip,
            src_size: value.size,
        }
    }
}

pub trait InstancedOutputBuilder<'a> {
    type ImageHandle: Copy;

    fn on_image(&mut self, size: Vec2<u32>, image: OutputImage<'a>) -> Self::ImageHandle;
    fn on_instance(&mut self, image: Self::ImageHandle, params: OutputInstanceParameters);
}

#[derive(Debug)]
struct ClipRectIntersection {
    dst_pos: Point2<i32>,
    min_clip: Vec2<u32>,
    max_clip: Vec2<u32>,
    size: Vec2<u32>,
}

fn clip_pixel_rects(src: Rect2<i32>, clip_rect: Rect2<i32>) -> Option<ClipRectIntersection> {
    if src.is_empty() {
        return None;
    }

    let mut intersection = ClipRectIntersection {
        dst_pos: src.min,
        min_clip: Vec2::ZERO,
        max_clip: Vec2::ZERO,
        size: Vec2::new(src.width() as u32, src.height() as u32),
    };

    if let Ok(left_clip) = u32::try_from(clip_rect.min.x - src.min.x) {
        intersection.dst_pos.x = clip_rect.min.x;
        intersection.min_clip.x = left_clip;
        intersection.size.x = intersection.size.x.checked_sub(left_clip)?;
    }

    if let Ok(top_clip) = u32::try_from(clip_rect.min.y - src.min.y) {
        intersection.dst_pos.y = clip_rect.min.y;
        intersection.min_clip.y = top_clip;
        intersection.size.y = intersection.size.y.checked_sub(top_clip)?;
    }

    if let Ok(right_clip) = u32::try_from(src.max.x - clip_rect.max.x) {
        intersection.max_clip.x = right_clip;
        intersection.size.x = intersection.size.x.checked_sub(right_clip)?;
    }

    if let Ok(bottom_clip) = u32::try_from(src.max.y - clip_rect.max.y) {
        intersection.max_clip.y = bottom_clip;
        intersection.size.y = intersection.size.y.checked_sub(bottom_clip)?;
    }

    if intersection.size.x == 0 || intersection.size.y == 0 {
        return None;
    }

    Some(intersection)
}

impl OutputPiece {
    fn rect(&self) -> Rect2<i32> {
        Rect2::from_min_size(self.pos, Vec2::new(self.size.x as i32, self.size.y as i32))
    }
}

pub fn pieces_to_instanced_images<'a, B: InstancedOutputBuilder<'a>>(
    builder: &mut B,
    pieces: impl Iterator<Item = &'a OutputPiece>,
    clip_rect: Rect2<i32>,
    rasterizer: &mut Rasterizer,
) {
    let mut texture_map = HashMap::<OutputBitmap<'a>, B::ImageHandle>::new();

    for piece in pieces {
        let Some(clip_intersection) = clip_pixel_rects(piece.rect(), clip_rect) else {
            continue;
        };

        let mut try_insert_bitmap = |builder: &mut B, bitmap: OutputBitmap<'a>| {
            let occupied = match texture_map.entry(bitmap) {
                std::collections::hash_map::Entry::Occupied(occupied) => occupied,
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    let bitmap = vacant.key().clone();
                    let handle =
                        builder.on_image(bitmap.texture.size(), OutputImage::Texture(bitmap));
                    vacant.insert_entry(handle)
                }
            };
            *occupied.get()
        };

        match &piece.content {
            OutputPieceContent::Texture(bitmap) => {
                if piece.size == bitmap.texture.size() {
                    let handle = try_insert_bitmap(builder, bitmap.clone());
                    builder.on_instance(handle, OutputInstanceParameters::from(clip_intersection));
                } else {
                    let mut params = OutputInstanceParameters {
                        dst_pos: clip_intersection.dst_pos,
                        dst_size: clip_intersection.size,
                        src_off: Vec2::ZERO,
                        src_size: bitmap.texture.size(),
                    };

                    let handle = if clip_intersection.min_clip != Vec2::ZERO
                        || clip_intersection.max_clip != Vec2::ZERO
                    {
                        // We can't really pass through the scaling if clipped as it might
                        // result in fractional offset/size, so we have to scale the bitmap
                        // here if that's the case.
                        let scaled = rasterizer.scale_texture(
                            &bitmap.texture,
                            piece.size,
                            Vec2::ZERO,
                            bitmap.texture.size(),
                        );
                        params.src_off = clip_intersection.min_clip;
                        params.src_size = clip_intersection.size;
                        try_insert_bitmap(
                            builder,
                            OutputBitmap {
                                texture: scaled,
                                ..*bitmap
                            },
                        )
                    } else {
                        try_insert_bitmap(builder, bitmap.clone())
                    };

                    builder.on_instance(handle, params);
                }
            }
            OutputPieceContent::Strips(strips) => {
                for op in strips.strips.paint_iter() {
                    let pos =
                        piece.pos + Vec2::new(i32::from(op.pos().x), i32::from(op.pos().y)) * 4;
                    let isize = Vec2::new(op.width() as i32, 4);

                    let Some(op_clip_intersection) =
                        clip_pixel_rects(Rect2::from_min_size(pos, isize), clip_rect)
                    else {
                        continue;
                    };

                    match op {
                        StripPaintOp::Copy(copy) => {
                            let texture = copy.to_texture();
                            let handle = try_insert_bitmap(
                                builder,
                                OutputBitmap {
                                    texture,
                                    filter: None,
                                    color: strips.color,
                                },
                            );
                            builder.on_instance(
                                handle,
                                OutputInstanceParameters::from(op_clip_intersection),
                            );
                        }
                        StripPaintOp::Fill(fill) => {
                            let texture = fill.to_vertical_texture();
                            let handle = try_insert_bitmap(
                                builder,
                                OutputBitmap {
                                    texture,
                                    filter: None,
                                    color: strips.color,
                                },
                            );
                            builder.on_instance(
                                handle,
                                OutputInstanceParameters {
                                    dst_pos: op_clip_intersection.dst_pos,
                                    dst_size: op_clip_intersection.size,
                                    src_off: Vec2::new(0, op_clip_intersection.min_clip.y),
                                    src_size: Vec2::new(1, op_clip_intersection.size.y),
                                },
                            );
                        }
                    }
                }
            }
            &OutputPieceContent::Rect(OutputRect { mut rect, color }) => {
                rect.max.x -= (piece.size.x - clip_intersection.size.x) as i32;
                rect.max.y -= (piece.size.y - clip_intersection.size.y) as i32;

                // If the rectangle has any of its sides clipped by at least one pixel then
                // the anti-aliasing on that side goes away.
                if clip_intersection.min_clip.x > 0 {
                    rect.min.x = FixedS::ZERO;
                }
                if clip_intersection.min_clip.y > 0 {
                    rect.min.y = FixedS::ZERO;
                }
                if clip_intersection.max_clip.x > 0 {
                    rect.max.x = rect.max.x.ceil();
                }
                if clip_intersection.max_clip.y > 0 {
                    rect.max.y = rect.max.y.ceil();
                }

                if rect.min.x.is_integer()
                    && rect.min.y.is_integer()
                    && rect.max.x.is_integer()
                    && rect.max.y.is_integer()
                {
                    let bitmap = OutputBitmap {
                        texture: Texture::SINGLE_FILLED_MONO_PIXEL,
                        filter: None,
                        color,
                    };
                    let handle = try_insert_bitmap(builder, bitmap.clone());
                    builder.on_instance(
                        handle,
                        OutputInstanceParameters {
                            dst_pos: clip_intersection.dst_pos,
                            dst_size: clip_intersection.size,
                            src_off: Vec2::ZERO,
                            src_size: bitmap.texture.size(),
                        },
                    );
                } else {
                    let handle = builder.on_image(
                        clip_intersection.size,
                        OutputImage::Rect(OutputRect { rect, color }),
                    );
                    builder.on_instance(
                        handle,
                        OutputInstanceParameters {
                            dst_pos: clip_intersection.dst_pos,
                            dst_size: clip_intersection.size,
                            src_off: Vec2::ZERO,
                            src_size: clip_intersection.size,
                        },
                    );
                }
            }
        }
    }
}
