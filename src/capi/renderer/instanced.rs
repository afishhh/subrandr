use std::{
    ffi::{c_int, c_void},
    marker::PhantomData,
};

use rasterize::{
    color::{Premultiplied, BGRA8},
    sw::{InstancedOutputBuilder, OutputImage, OutputPiece},
};
use util::math::{Point2, Rect2, Vec2};

use super::CRenderer;
use crate::SubtitleContext;

#[repr(C)]
pub(super) struct COutputImage<'a> {
    size: Vec2<u32>,
    user_data: *mut c_void,
    next: *const COutputImage<'a>,
    content: OutputImage<'a>,
}

#[repr(C)]
pub(super) struct COutputInstance<'a> {
    next: *const COutputInstance<'a>,
    base: COutputInstanceBase<'a>,
    dst_pos: Point2<i32>,
    dst_size: Vec2<u32>,
    src_off: Vec2<u32>,
    src_size: Vec2<u32>,
}

union COutputInstanceBase<'a> {
    idx: usize,
    ptr: *const COutputImage<'a>,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render_instanced(
    renderer: *mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
    clip_rect: Rect2<i32>,
    flags: u64,
) -> *mut CRenderer {
    if flags != 0 {
        cthrow!(
            InvalidArgument,
            "non-zero flags parameter passed to `sbr_renderer_render_instanced`"
        );
    }

    assert!(
        (*renderer).output_pieces.is_empty(),
        "Output piece buffer isn't empty, did you forget to call `sbr_instanced_raster_pass_finish`?"
    );

    if !clip_rect.is_empty() {
        let renderer = &mut (*renderer);

        ctry!(renderer
            .inner
            .render_to_scene(&*ctx, t, &renderer.rasterizer));

        ctry!(renderer
            .rasterizer
            .render_scene_pieces(
                renderer.inner.scene(),
                &mut |piece| {
                    if piece.size.x == 0 || piece.size.y == 0 {
                        return;
                    }

                    renderer.output_pieces.push(piece);
                },
                &renderer.inner.glyph_cache,
            )
            // Make sure piece buffer is cleared if rendering fails
            // so the above assertion is not triggered in such a case.
            .inspect_err(|_| renderer.output_pieces.clear()));

        struct CInstancedOutputBuilder<'a, 'o> {
            images: &'o mut Vec<COutputImage<'static>>,
            instances: &'o mut Vec<COutputInstance<'static>>,
            _lifetime: PhantomData<&'a OutputPiece>,
        }

        impl<'o, 'a> InstancedOutputBuilder<'a> for CInstancedOutputBuilder<'o, 'a> {
            type ImageHandle = usize;

            fn on_image(&mut self, size: Vec2<u32>, image: OutputImage<'a>) -> Self::ImageHandle {
                let id = self.images.len();
                self.images.push(COutputImage {
                    size,
                    user_data: std::ptr::null_mut(),
                    next: std::ptr::null(),
                    // erase lifetime
                    content: unsafe { std::mem::transmute(image) },
                });
                id
            }

            fn on_instance(
                &mut self,
                image: Self::ImageHandle,
                params: rasterize::sw::OutputInstanceParameters,
            ) {
                self.instances.push(COutputInstance {
                    dst_pos: params.dst_pos,
                    dst_size: params.dst_size,
                    src_off: params.src_off,
                    src_size: params.src_size,
                    base: COutputInstanceBase { idx: image },
                    next: std::ptr::null(),
                });
            }
        }

        rasterize::sw::pieces_to_instanced_images(
            &mut CInstancedOutputBuilder {
                images: &mut renderer.output_images,
                instances: &mut renderer.output_instances,
                _lifetime: PhantomData,
            },
            renderer.output_pieces.iter(),
            clip_rect,
        );

        if !renderer.output_instances.is_empty() {
            let len = renderer.output_instances.len();
            let mut current = renderer.output_instances.as_mut_ptr();
            let mut next = current.wrapping_add(1);
            let end = current.add(len);
            loop {
                (*current).base.ptr = renderer.output_images.as_ptr().add((*current).base.idx);
                if next == end {
                    break;
                }
                (*current).next = next;
                current = next;
                next = next.wrapping_add(1);
            }
        }
    }

    renderer
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_instanced_raster_pass_get_instances(
    renderer: *mut CRenderer,
) -> *const COutputInstance<'static> {
    if (*renderer).output_instances.is_empty() {
        std::ptr::null()
    } else {
        (*renderer).output_instances.as_ptr()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_output_image_rasterize_into(
    image: *const COutputImage,
    renderer: *mut CRenderer,
    off_x: i32,
    off_y: i32,
    buffer: *mut Premultiplied<BGRA8>,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let rasterizer = &mut (*renderer).rasterizer;
    let mut target = rasterize::sw::RenderTarget::new(
        std::slice::from_raw_parts_mut(buffer, height as usize * stride as usize),
        width,
        height,
        stride,
    );

    (*image)
        .content
        .rasterize_to(rasterizer, &mut target, Point2::new(off_x, off_y));

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_instanced_raster_pass_finish(renderer: *mut CRenderer) {
    (*renderer).inner.end_raster();
    (*renderer).output_instances.clear();
    (*renderer).output_images.clear();
    (*renderer).output_pieces.clear();
}
