use std::{any::Any, collections::HashMap, hash::Hash, mem::MaybeUninit};

use util::{
    math::{Point2, Rect2, Vec2},
    rc::{Arc, UniqueArc},
    slice_assume_init_mut,
};

use crate::{
    color::{Premultiplied, Premultiply, BGRA8},
    rasterizer::SceneRenderErrorInner,
    scene::{Bitmap, BitmapFilter, FilledRect, FixedS, Rect2S, SceneNode, Vec2S},
    PixelFormat, SceneRenderError,
};

mod blit;
pub(super) mod blur;
mod scale;
use blur::gaussian_sigma_to_box_radius;
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

pub struct RenderTargetView<'a, P> {
    buffer: &'a mut [P],
    width: u32,
    height: u32,
    stride: u32,
}

impl<'a, P> RenderTargetView<'a, P> {
    pub fn new(buffer: &'a mut [P], width: u32, height: u32, stride: u32) -> Self {
        let self_name = std::any::type_name::<Self>();
        let Some(n_pixels) = (height as usize).checked_mul(stride as usize) else {
            panic!("Size passed to {self_name}::new overflows `height * stride`",);
        };
        assert!(
            buffer.len() >= n_pixels,
            "Buffer passed to {self_name}::new is too small",
        );
        assert!(
            width <= stride,
            "width passed to {self_name}::new is larger than stride",
        );

        Self {
            buffer,
            width,
            height,
            stride,
        }
    }

    pub fn empty() -> Self {
        Self {
            buffer: &mut [],
            width: 0,
            height: 0,
            stride: 0,
        }
    }

    pub fn buffer_mut(&mut self) -> &mut [P] {
        self.buffer
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

fn unwrap_sw_render_target<'a, 'b>(
    target: &'a mut super::RenderTarget<'b>,
) -> &'a mut RenderTarget<'b> {
    #[cfg_attr(not(feature = "wgpu"), expect(unreachable_patterns))]
    match &mut target.0 {
        super::RenderTargetInner::Software(target) => target,
        target => panic!(
            "Incompatible render target {:?} passed to software rasterizer (expected: software)",
            target.variant_name()
        ),
    }
}

#[derive(Clone)]
enum TextureData<'a> {
    OwnedMono(Arc<[u8]>),
    OwnedBgra(Arc<[Premultiplied<BGRA8>]>),
    BorrowedMono(&'a [u8]),
}

impl TextureData<'_> {
    pub fn as_ref(&self) -> TextureDataRef<'_> {
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

enum TextureDataRef<'a> {
    Mono(&'a [u8]),
    Bgra(&'a [Premultiplied<BGRA8>]),
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
    width: u32,
    height: u32,
    data: TextureData<'a>,
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

    pub const SINGLE_FILLED_MONO_PIXEL: Texture<'static> = Texture {
        width: 1,
        height: 1,
        data: TextureData::BorrowedMono(&[u8::MAX]),
    };
}

impl Texture<'_> {
    pub fn size(&self) -> Vec2<u32> {
        Vec2::new(self.width, self.height)
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn memory_footprint(&self) -> usize {
        match &self.data {
            TextureData::OwnedMono(mono) => mono.len(),
            TextureData::OwnedBgra(bgra) => bgra.len() * 4,
            TextureData::BorrowedMono(_) => 0,
        }
    }

    pub(super) fn is_mono(&self) -> bool {
        match &self.data {
            TextureData::OwnedMono(_) | TextureData::BorrowedMono(_) => true,
            TextureData::OwnedBgra(_) => false,
        }
    }
}

impl std::fmt::Debug for Texture<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}x{} {} texture",
            self.width,
            self.height,
            match self.data.as_ref() {
                TextureDataRef::Mono(_) => "A8",
                TextureDataRef::Bgra(_) => "BGRA8",
            }
        )
    }
}

fn unwrap_sw_texture(texture: &super::Texture) -> &Texture<'static> {
    #[cfg_attr(not(feature = "wgpu"), expect(unreachable_patterns))]
    match &texture.0 {
        super::TextureInner::Software(texture) => texture,
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

    pub fn blit(
        &self,
        target: &mut RenderTarget,
        dx: i32,
        dy: i32,
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
                    dx as isize,
                    dy as isize,
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
                    dx as isize,
                    dy as isize,
                    color.a,
                );
            }
        }
    }

    pub fn blit_texture_filtered(
        &self,
        target: &mut RenderTarget,
        pos: Point2<i32>,
        texture: &Texture,
        filter: Option<BitmapFilter>,
        color: BGRA8,
    ) {
        match (filter, texture.data.as_ref()) {
            (None, _) | (Some(BitmapFilter::ExtractAlpha), TextureDataRef::Mono(_)) => {
                self.blit(target, pos.x, pos.y, texture, color)
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

    pub fn copy_texture_filtered(
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

    pub fn scale_texture(
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

    pub fn fill_axis_aligned_rect(
        &self,
        target: RenderTargetView<Premultiplied<BGRA8>>,
        rect: Rect2S,
        color: Premultiplied<BGRA8>,
    ) {
        fill_rect::<BlendOver, _>(target, rect, color);
    }
}

impl super::Rasterizer for Rasterizer {
    fn name(&self) -> &'static str {
        "software"
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

    fn blur_texture(&mut self, texture: &super::Texture, blur_sigma: f32) -> super::BlurOutput {
        let texture = unwrap_sw_texture(texture);

        self.blurer.prepare(
            texture.width as usize,
            texture.height as usize,
            gaussian_sigma_to_box_radius(blur_sigma),
        );

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

        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_horizontal();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();
        self.blurer.box_blur_vertical();

        let result = Texture::new_with(Vec2::new(width as u32, height as u32), |target| {
            blit::copy_float_to_mono(target, self.blurer.front(), width, width, height, 0, 0);
        });

        super::BlurOutput {
            padding: Vec2::splat(self.blurer.padding() as u32),
            texture: super::Texture(super::TextureInner::Software(result)),
        }
    }

    fn render_scene(
        &mut self,
        target: &mut super::RenderTarget,
        scene: &[SceneNode],
        user_data: &(dyn Any + 'static),
    ) -> Result<(), super::SceneRenderError> {
        let target = unwrap_sw_render_target(target);
        target.buffer.fill(Premultiplied(BGRA8::ZERO));

        self.render_scene_pieces_at(
            Vec2::ZERO,
            scene,
            &mut |r, piece| piece.content.blend_to_impl(r, target, piece.pos),
            user_data,
        )?;

        Ok(())
    }
}

#[derive(Clone, Hash, PartialEq, Eq)]
pub struct OutputBitmap<'a> {
    pub texture: Texture<'a>,
    pub filter: Option<BitmapFilter>,
    pub color: BGRA8,
}

pub struct OutputStrips {
    pub strips: Strips,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct OutputRect {
    pub rect: Rect2S,
    pub color: BGRA8,
}

pub enum OutputPieceContent {
    Texture(OutputBitmap<'static>),
    Strips(OutputStrips),
    Rect(OutputRect),
}

pub struct OutputPiece {
    pub pos: Point2<i32>,
    pub size: Vec2<u32>,
    pub content: OutputPieceContent,
}

impl OutputPiece {
    fn from_bitmap(bitmap: Bitmap) -> Self {
        let texture = unwrap_sw_texture(&bitmap.texture);
        Self {
            pos: bitmap.pos,
            size: Vec2::new(bitmap.texture.width(), bitmap.texture.height()),
            content: {
                OutputPieceContent::Texture(OutputBitmap {
                    texture: texture.clone(),
                    filter: bitmap.filter,
                    color: bitmap.color,
                })
            },
        }
    }
}

impl Rasterizer {
    fn render_scene_pieces_at(
        &mut self,
        offset: Vec2S,
        scene: &[SceneNode],
        on_piece: &mut dyn FnMut(&mut Rasterizer, OutputPiece),
        user_data: &(dyn Any + 'static),
    ) -> Result<(), SceneRenderError> {
        let current_translation = offset;
        for node in scene {
            match node {
                SceneNode::DeferredBitmaps(bitmaps) => {
                    for bitmap in (bitmaps.to_bitmaps)(self, user_data)
                        .map_err(SceneRenderErrorInner::ToBitmaps)?
                    {
                        on_piece(self, OutputPiece::from_bitmap(bitmap));
                    }
                }
                SceneNode::Bitmap(bitmap) => {
                    on_piece(self, OutputPiece::from_bitmap(bitmap.clone()));
                }
                &SceneNode::FilledRect(FilledRect { rect, color }) => {
                    on_piece(
                        self,
                        OutputPiece {
                            pos: Point2::new(
                                rect.min.x.floor_to_inner(),
                                rect.min.y.floor_to_inner(),
                            ),
                            size: Vec2::new(
                                (rect.max.x - rect.min.x.floor()).ceil_to_inner() as u32,
                                (rect.max.y - rect.min.y.floor()).ceil_to_inner() as u32,
                            ),
                            content: OutputPieceContent::Rect(OutputRect {
                                rect: Rect2S {
                                    min: Point2::new(rect.min.x.fract(), rect.min.y.fract()),
                                    max: Point2::new(
                                        rect.max.x - rect.min.x.floor(),
                                        rect.max.y - rect.min.y.floor(),
                                    ),
                                },
                                color,
                            }),
                        },
                    );
                }
                SceneNode::StrokedPolyline(polyline) => {
                    let (pos, size, strips) = polyline.to_strips(current_translation.to_point());
                    on_piece(
                        self,
                        OutputPiece {
                            pos,
                            size,
                            content: OutputPieceContent::Strips(OutputStrips {
                                strips,
                                color: polyline.color,
                            }),
                        },
                    )
                }
                SceneNode::Subscene(subscene) => self.render_scene_pieces_at(
                    current_translation + subscene.pos.to_vec(),
                    &subscene.scene,
                    on_piece,
                    user_data,
                )?,
            }
        }

        Ok(())
    }

    pub fn render_scene_pieces(
        &mut self,
        scene: &[SceneNode],
        on_piece: &mut dyn FnMut(OutputPiece),
        user_data: &(dyn Any + 'static),
    ) -> Result<(), SceneRenderError> {
        self.render_scene_pieces_at(Vec2::ZERO, scene, &mut move |_r, p| on_piece(p), user_data)
    }
}

impl OutputPieceContent {
    fn blend_to_impl(
        &self,
        rasterizer: &mut Rasterizer,
        target: &mut RenderTarget,
        pos: Point2<i32>,
    ) {
        match self {
            OutputPieceContent::Texture(image) => {
                rasterizer.blit_texture_filtered(
                    target,
                    pos,
                    &image.texture,
                    image.filter,
                    image.color,
                );
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

    pub fn blend_to(
        &self,
        rasterizer: &mut Rasterizer,
        target: &mut super::RenderTarget,
        pos: Point2<i32>,
    ) {
        self.blend_to_impl(rasterizer, unwrap_sw_render_target(target), pos);
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
) {
    let mut texture_map = HashMap::<OutputBitmap<'static>, B::ImageHandle>::new();

    for piece in pieces {
        let Some(clip_intersection) = clip_pixel_rects(piece.rect(), clip_rect) else {
            continue;
        };

        let mut try_insert_bitmap = |bitmap: OutputBitmap<'static>| {
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
                let handle = try_insert_bitmap(bitmap.clone());
                builder.on_instance(handle, OutputInstanceParameters::from(clip_intersection));
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
                            let size = texture.size();
                            let handle = builder.on_image(
                                size,
                                OutputImage::Texture(OutputBitmap {
                                    texture,
                                    filter: None,
                                    color: strips.color,
                                }),
                            );
                            builder.on_instance(
                                handle,
                                OutputInstanceParameters::from(op_clip_intersection),
                            );
                        }
                        StripPaintOp::Fill(fill) => {
                            let texture = fill.to_vertical_texture();
                            let handle = builder.on_image(
                                texture.size(),
                                OutputImage::Texture(OutputBitmap {
                                    texture,
                                    filter: None,
                                    color: strips.color,
                                }),
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
                if let Some(clip_horizontal) = piece.size.x.checked_sub(clip_intersection.size.x) {
                    rect.max.y -= FixedS::new(clip_horizontal as i32);
                }
                if let Some(clip_vertical) = piece.size.y.checked_sub(clip_intersection.size.y) {
                    rect.max.y -= FixedS::new(clip_vertical as i32);
                }

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
                    let handle = try_insert_bitmap(bitmap.clone());
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
                    let handle =
                        builder.on_image(piece.size, OutputImage::Rect(OutputRect { rect, color }));
                    builder.on_instance(handle, OutputInstanceParameters::from(clip_intersection));
                }
            }
        }
    }
}
