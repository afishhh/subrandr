use std::{any::Any, hash::Hash, mem::MaybeUninit};

use util::{
    math::{Point2, Vec2},
    rc::{Arc, UniqueArc},
};

use crate::{
    color::{Premultiply, BGRA8},
    rasterizer::SceneRenderErrorInner,
    scene::{Bitmap, BitmapFilter, FilledRect, Rect2S, SceneNode, Vec2S},
    PixelFormat, SceneRenderError,
};

mod blit;
pub(super) mod blur;
use blur::gaussian_sigma_to_box_radius;
mod strip;
pub use strip::*;

trait DrawPixel: Copy + Sized {
    fn put(&mut self, value: Self);
    fn scale_alpha(self, scale: u8) -> Self;
    const PIXEL_FORMAT: PixelFormat;
    fn cast_target_buffer<'a>(buffer: RenderTargetBufferMut<'a>) -> Option<&'a mut [Self]>;
}

impl DrawPixel for BGRA8 {
    fn put(&mut self, value: Self) {
        // TODO: blend_over
        *self = value.premultiply().0;
    }

    fn scale_alpha(self, scale: u8) -> Self {
        self.mul_alpha(scale)
    }

    const PIXEL_FORMAT: PixelFormat = PixelFormat::Bgra;
    fn cast_target_buffer<'a>(buffer: RenderTargetBufferMut<'a>) -> Option<&'a mut [Self]> {
        match buffer {
            RenderTargetBufferMut::Bgra(bgra) => Some(bgra),
            _ => None,
        }
    }
}

impl DrawPixel for u8 {
    fn put(&mut self, value: Self) {
        // Use simple additive blending for monochrome rendering
        // TODO: It's kinda weird to be using these different blending modes for
        //       unannotated primitives like `u8` though, maybe it could be cleaned up?
        *self = self.saturating_add(value);
    }

    fn scale_alpha(self, scale: u8) -> Self {
        crate::color::mul_rgb(self, scale)
    }

    const PIXEL_FORMAT: PixelFormat = PixelFormat::Mono;
    fn cast_target_buffer<'a>(buffer: RenderTargetBufferMut<'a>) -> Option<&'a mut [Self]> {
        match buffer {
            RenderTargetBufferMut::Mono(mono) => Some(mono),
            _ => None,
        }
    }
}

unsafe fn horizontal_line_unchecked<P: DrawPixel>(
    x0: i32,
    x1: i32,
    offset_buffer: &mut [P],
    width: i32,
    color: P,
) {
    for x in x0.clamp(0, width)..x1.clamp(0, width) {
        offset_buffer.get_unchecked_mut(x as usize).put(color);
    }
}

unsafe fn vertical_line_unchecked<P: DrawPixel>(
    y0: i32,
    y1: i32,
    offset_buffer: &mut [P],
    height: i32,
    stride: i32,
    color: P,
) {
    for y in y0.clamp(0, height)..y1.clamp(0, height) {
        offset_buffer
            .get_unchecked_mut((y * stride) as usize)
            .put(color);
    }
}

macro_rules! check_buffer {
    ($what: literal, $buffer: ident, $stride: ident, $height: ident) => {
        if $buffer.len() < $stride as usize * $height as usize {
            panic!(concat!(
                "Buffer passed to rasterize::",
                $what,
                " is too small"
            ))
        }
    };
}

// Scuffed Anti-Aliasing™ (SAA)
fn fill_axis_aligned_antialias_rect<P: DrawPixel>(
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    buffer: &mut [P],
    width: u32,
    height: u32,
    stride: u32,
    color: P,
) {
    check_buffer!("fill_axis_aligned_antialias_rect", buffer, stride, height);

    debug_assert!(x0 <= x1);
    debug_assert!(y0 <= y1);

    const AA_THRESHOLD: f32 = 1. / 256.;

    let (left_aa, full_left) = if (x0 - x0.round()).abs() > AA_THRESHOLD {
        (true, x0.ceil() as i32)
    } else {
        (false, x0.round() as i32)
    };

    let (right_aa, full_right) = if (x1 - x1.round()).abs() > AA_THRESHOLD {
        (true, x1.floor() as i32)
    } else {
        (false, x1.round() as i32)
    };

    let (top_aa_width, full_top) = if (y0 - y0.round()).abs() > AA_THRESHOLD {
        let top_width = 1.0 - y0.fract();
        let top_fill = top_width * 255.;
        let top_y = y0.floor() as i32;
        if top_y >= 0 && top_y < height as i32 {
            unsafe {
                horizontal_line_unchecked(
                    full_left,
                    full_right,
                    &mut buffer[top_y as usize * stride as usize..],
                    width as i32,
                    color.scale_alpha(top_fill as u8),
                );
            }
        }
        (top_width, top_y + 1)
    } else {
        (1.0, y0.round() as i32)
    };

    let (bottom_aa_width, full_bottom) = if (y1 - y1.round()).abs() > AA_THRESHOLD {
        let bottom_width = y1.fract();
        let bottom_fill = bottom_width * 255.;
        let bottom_y = y1.floor() as i32;
        if bottom_y >= 0 && bottom_y < height as i32 {
            unsafe {
                horizontal_line_unchecked(
                    full_left,
                    full_right,
                    &mut buffer[bottom_y as usize * stride as usize..],
                    width as i32,
                    color.scale_alpha(bottom_fill as u8),
                );
            }
        }
        (bottom_width, bottom_y)
    } else {
        (1.0, y1.round() as i32)
    };

    if left_aa {
        let left_fill = (1.0 - x0.fract()) * 255.;
        let left_x = full_left - 1;
        if left_x >= 0 && left_x < width as i32 {
            if top_aa_width < 1.0 && full_top > 0 && full_top < height as i32 {
                buffer[(full_top - 1) as usize * stride as usize + left_x as usize]
                    .put(color.scale_alpha((left_fill * top_aa_width) as u8));
            }

            unsafe {
                vertical_line_unchecked(
                    full_top,
                    full_bottom,
                    &mut buffer[left_x as usize..],
                    height as i32,
                    stride as i32,
                    color.scale_alpha(left_fill as u8),
                );
            }

            if bottom_aa_width < 1.0 && full_bottom >= 0 && full_bottom < height as i32 {
                buffer[full_bottom as usize * stride as usize + left_x as usize]
                    .put(color.scale_alpha((left_fill * bottom_aa_width) as u8));
            }
        }
    }

    if right_aa {
        let right_fill = x1.fract() * 255.;
        let right_x = full_right;
        if right_x >= 0 && right_x < width as i32 {
            if top_aa_width < 1.0 && full_top > 0 && full_top < height as i32 {
                buffer[(full_top - 1) as usize * stride as usize + right_x as usize]
                    .put(color.scale_alpha((right_fill * top_aa_width) as u8));
            }

            unsafe {
                vertical_line_unchecked(
                    full_top,
                    full_bottom,
                    &mut buffer[right_x as usize..],
                    height as i32,
                    stride as i32,
                    color.scale_alpha(right_fill as u8),
                );
            }

            if bottom_aa_width < 1.0 && full_bottom >= 0 && full_bottom < height as i32 {
                buffer[full_bottom as usize * stride as usize + right_x as usize]
                    .put(color.scale_alpha((right_fill * bottom_aa_width) as u8));
            }
        }
    }

    for y in full_top.clamp(0, height as i32)..full_bottom.clamp(0, height as i32) {
        unsafe {
            horizontal_line_unchecked(
                full_left,
                full_right,
                &mut buffer[y as usize * stride as usize..],
                width as i32,
                color,
            );
        }
    }
}

pub struct RenderTarget<'a> {
    buffer: RenderTargetBuffer<'a>,
    width: u32,
    height: u32,
    stride: u32,
}

impl<'a> RenderTarget<'a> {
    fn new_owned_mono(width: u32, height: u32) -> Self {
        Self {
            buffer: {
                let mut uninit = UniqueArc::new_uninit_slice(width as usize * height as usize);
                unsafe {
                    uninit.fill(MaybeUninit::zeroed());
                    RenderTargetBuffer::OwnedMono(UniqueArc::assume_init(uninit))
                }
            },
            width,
            height,
            stride: width,
        }
    }

    fn owned_into_texture(self) -> Texture {
        assert_eq!(self.stride, self.width);

        Texture {
            width: self.width,
            height: self.height,
            data: match self.buffer {
                RenderTargetBuffer::OwnedMono(mono) => {
                    TextureData::OwnedMono(UniqueArc::into_shared(mono))
                }
                _ => panic!("Cannot convert a borrowed RenderTarget into a Texture"),
            },
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

enum RenderTargetBuffer<'a> {
    OwnedMono(UniqueArc<[u8]>),
    BorrowedBgra(&'a mut [BGRA8]),
    BorrowedMono(&'a mut [u8]),
}

impl RenderTargetBuffer<'_> {
    fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::BorrowedBgra(_) => PixelFormat::Bgra,
            Self::OwnedMono(_) | Self::BorrowedMono(_) => PixelFormat::Mono,
        }
    }
}

enum RenderTargetBufferMut<'a> {
    Bgra(&'a mut [BGRA8]),
    Mono(&'a mut [u8]),
}

pub fn create_render_target(
    buffer: &mut [BGRA8],
    width: u32,
    height: u32,
    stride: u32,
) -> super::RenderTarget<'_> {
    assert!(
        buffer.len() >= height as usize * stride as usize,
        "Buffer passed to rasterize::sw::create_render_target is too small!"
    );
    super::RenderTarget(super::RenderTargetInner::Software(RenderTarget {
        buffer: RenderTargetBuffer::BorrowedBgra(buffer),
        width,
        height,
        stride,
    }))
}

pub fn create_render_target_mono(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    stride: u32,
) -> super::RenderTarget<'_> {
    assert!(
        buffer.len() >= height as usize * stride as usize,
        "Buffer passed to rasterize::sw::create_render_target is too small!"
    );
    super::RenderTarget(super::RenderTargetInner::Software(RenderTarget {
        buffer: RenderTargetBuffer::BorrowedMono(buffer),
        width,
        height,
        stride,
    }))
}

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

impl RenderTargetBuffer<'_> {
    fn as_mut(&mut self) -> RenderTargetBufferMut<'_> {
        match self {
            RenderTargetBuffer::OwnedMono(mono) => RenderTargetBufferMut::Mono(mono),
            RenderTargetBuffer::BorrowedBgra(bgra) => RenderTargetBufferMut::Bgra(bgra),
            RenderTargetBuffer::BorrowedMono(mono) => RenderTargetBufferMut::Mono(mono),
        }
    }

    fn unwrap_for<P: DrawPixel>(&mut self) -> &'_ mut [P] {
        // TODO: NLL problem case 3
        let pixel_format = self.pixel_format();
        P::cast_target_buffer( self.as_mut()).unwrap_or_else(|| {
            panic!("Incompatible render target format {:?} passed to software rasterizer (expected: {:?})", pixel_format, P::PIXEL_FORMAT)
        })
    }
}

#[derive(Clone)]
enum TextureData {
    OwnedMono(Arc<[u8]>),
    OwnedBgra(Arc<[BGRA8]>),
}

impl TextureData {
    pub fn as_ref(&self) -> TextureDataRef<'_> {
        match self {
            TextureData::OwnedMono(a8) => TextureDataRef::Mono(a8),
            TextureData::OwnedBgra(bgra8) => TextureDataRef::Bgra(bgra8),
        }
    }
}

enum TextureDataRef<'a> {
    Mono(&'a [u8]),
    Bgra(&'a [BGRA8]),
}

#[derive(Clone)]
pub struct Texture {
    width: u32,
    height: u32,
    data: TextureData,
}

impl Texture {
    unsafe fn new_with_initializer(
        width: u32,
        height: u32,
        format: PixelFormat,
        callback: impl FnOnce(&mut [MaybeUninit<u8>], usize),
    ) -> Self {
        let n_pixels = width as usize * height as usize;

        match format {
            PixelFormat::Mono => {
                let mut data = UniqueArc::new_uninit_slice(n_pixels);

                callback(&mut data, width as usize);
                let init = UniqueArc::assume_init(data);

                Texture {
                    width,
                    height,
                    data: TextureData::OwnedMono(UniqueArc::into_shared(init)),
                }
            }
            PixelFormat::Bgra => {
                let mut data = UniqueArc::<[BGRA8]>::new_uninit_slice(n_pixels);
                let slice = unsafe {
                    std::slice::from_raw_parts_mut(
                        data.as_mut_ptr().cast::<MaybeUninit<u8>>(),
                        data.len() * 4,
                    )
                };

                callback(slice, width as usize * 4);
                let init = UniqueArc::assume_init(data);

                Texture {
                    width,
                    height,
                    data: TextureData::OwnedBgra(UniqueArc::into_shared(init)),
                }
            }
        }
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
        }
    }

    pub(super) fn is_mono(&self) -> bool {
        match &self.data {
            TextureData::OwnedMono(_) => true,
            TextureData::OwnedBgra(_) => false,
        }
    }
}

fn unwrap_sw_texture(texture: &super::Texture) -> &Texture {
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
                    target.buffer.unwrap_for::<BGRA8>(),
                    target.stride as usize,
                    target.width as usize,
                    target.height as usize,
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
                    target.buffer.unwrap_for::<BGRA8>(),
                    target.stride as usize,
                    target.width as usize,
                    target.height as usize,
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
                    target.buffer.unwrap_for::<BGRA8>(),
                    target.stride as usize,
                    target.width as usize,
                    target.height as usize,
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

    pub fn fill_axis_aligned_rect(
        &mut self,
        target: &mut RenderTarget,
        rect: Rect2S,
        color: BGRA8,
    ) {
        // TODO: update fill_axis_aligned_antialias_rect to Fixed
        let rect = Rect2S::to_float(rect);
        fill_axis_aligned_antialias_rect(
            rect.min.x,
            rect.min.y,
            rect.max.x,
            rect.max.y,
            target.buffer.unwrap_for::<BGRA8>(),
            target.width,
            target.height,
            target.stride,
            color,
        );
    }
}

impl super::Rasterizer for Rasterizer {
    fn name(&self) -> &'static str {
        "software"
    }

    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> super::Texture {
        super::Texture(super::TextureInner::Software(
            Texture::new_with_initializer(width, height, format, callback),
        ))
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
        match texture.data.as_ref() {
            TextureDataRef::Mono(source) => blit::copy_mono_to_float(
                self.blurer.front_mut(),
                width,
                width,
                height,
                source,
                texture.width as usize,
                texture.width as usize,
                texture.height as usize,
                dx as isize,
                dy as isize,
            ),
            TextureDataRef::Bgra(source) => blit::copy_bgra_to_float(
                self.blurer.front_mut(),
                width,
                width,
                height,
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

        let mut target =
            RenderTarget::new_owned_mono(self.blurer.width() as u32, self.blurer.height() as u32);

        blit::copy_float_to_mono(
            target.buffer.unwrap_for::<u8>(),
            target.stride as usize,
            target.width as usize,
            target.height as usize,
            self.blurer.front(),
            self.blurer.width(),
            self.blurer.width(),
            self.blurer.height(),
            0,
            0,
        );

        super::BlurOutput {
            padding: Vec2::splat(self.blurer.padding() as u32),
            texture: super::Texture(super::TextureInner::Software(target.owned_into_texture())),
        }
    }

    fn render_scene(
        &mut self,
        target: &mut super::RenderTarget,
        scene: &[SceneNode],
        user_data: &(dyn Any + 'static),
    ) -> Result<(), super::SceneRenderError> {
        let target = unwrap_sw_render_target(target);
        target.buffer.unwrap_for::<BGRA8>().fill(BGRA8::ZERO);

        self.render_scene_pieces_at(
            Vec2::ZERO,
            scene,
            &mut |r, piece| piece.content.blend_to_impl(r, target, piece.pos),
            user_data,
        )?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct OutputBitmap {
    pub texture: Texture,
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
    Texture(OutputBitmap),
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
                    target.buffer.unwrap_for::<BGRA8>(),
                    |d, s| *d = pre.mul_alpha(s).blend_over(*d).0,
                    Vec2::new(pos.x as isize, pos.y as isize),
                    target.width as usize,
                    target.height as usize,
                    target.stride as usize,
                );
            }
            &OutputPieceContent::Rect(OutputRect { rect, color }) => {
                rasterizer.fill_axis_aligned_rect(
                    target,
                    rect.translate(Vec2::new(pos.x as f32, pos.y as f32)),
                    color,
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
