use std::{
    ffi::{c_int, c_void},
    marker::PhantomData,
};

use rasterize::{
    color::BGRA8,
    sw::{InstancedOutputBuilder, OutputImage, OutputPiece},
};
use util::math::{Point2, Vec2};

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
    pos: Point2<i32>,
    size: Vec2<u32>,
    base: COutputInstanceBase<'a>,
    next: *const COutputInstance<'a>,
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
    flags: u64,
) -> *mut CRenderer {
    if flags != 0 {
        cthrow!(
            InvalidArgument,
            "non-zero flags parameter passed to `sbr_renderer_render_instanced`"
        );
    }

    {
        let renderer = &mut (*renderer);
        assert!(renderer.output_pieces.is_empty(), "Output piece buffer isn't empty, did you forget to call `sbr_piece_raster_pass_finish`?");

        ctry!(renderer
            .inner
            .render_to_scene(&*ctx, t, &renderer.rasterizer));

        ctry!(renderer.rasterizer.render_scene_pieces(
            renderer.inner.scene(),
            &mut |piece| {
                if piece.size.x == 0 || piece.size.y == 0 {
                    return;
                }

                renderer.output_pieces.push(piece);
            },
            &renderer.inner.glyph_cache
        ));

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
                    pos: params.pos,
                    size: params.size,
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
unsafe extern "C" fn sbr_output_image_draw_to(
    image: *const COutputImage,
    renderer: *mut CRenderer,
    off_x: i32,
    off_y: i32,
    buffer: *mut BGRA8,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let rasterizer = &mut (*renderer).rasterizer;
    let mut target = rasterize::sw::RenderTarget::new_borrowed_bgra8(
        std::slice::from_raw_parts_mut(buffer, height as usize * stride as usize),
        width,
        height,
        stride,
    );

    (*image)
        .content
        .draw_to(rasterizer, &mut target, Point2::new(off_x, off_y));

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_instanced_raster_pass_finish(renderer: *mut CRenderer) {
    (*renderer).inner.end_raster();
    (*renderer).output_instances.clear();
    (*renderer).output_images.clear();
    (*renderer).output_pieces.clear();
}
