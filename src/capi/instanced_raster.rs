use std::{
    ffi::{c_int, c_void},
    marker::PhantomData,
    ptr::NonNull,
};

use log::{trace, LogContext};
use rasterize::{
    color::{Premultiplied, BGRA8},
    scene::{FixedS, Rect2S, Scene},
    sw::{self, InstancedOutputBuilder, OutputImage, OutputPiece},
};
use util::math::{Point2, Rect2, Vec2};

use crate::capi::{renderer::CRenderer, CError, ErrorKind};

#[repr(C)]
pub(super) struct COutputImage<'a> {
    size: Vec2<u32>,
    user_data: *mut c_void,
    /* Public fields end here */
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
    /* Public fields end here */
}

union COutputInstanceBase<'a> {
    idx: usize,
    ptr: *const COutputImage<'a>,
}

pub(super) struct CInstancedRasterPass {
    output_pieces: Vec<OutputPiece>,
    output_images: Vec<COutputImage<'static>>,
    output_instances: Vec<COutputInstance<'static>>,
    current: Option<CInstancedRasterPassContext>,
}

pub(super) enum CInstancedRasterPassContext {
    Renderer(NonNull<CRenderer>),
}

impl CInstancedRasterPassContext {
    unsafe fn rasterizer(&self) -> *mut sw::Rasterizer {
        match self {
            CInstancedRasterPassContext::Renderer(renderer) => {
                &raw mut (*renderer.as_ptr()).rasterizer
            }
        }
    }

    unsafe fn finish(self) {
        match self {
            CInstancedRasterPassContext::Renderer(renderer) => {
                let log = &(*(*renderer.as_ptr()).lib).root_logger.new_ctx();
                (*renderer.as_ptr()).inner.end_raster(log);
            }
        }
    }
}

impl CInstancedRasterPass {
    pub(super) unsafe fn new() -> Self {
        Self {
            output_pieces: Vec::new(),
            output_images: Vec::new(),
            output_instances: Vec::new(),
            current: None,
        }
    }

    #[track_caller]
    pub(super) unsafe fn render_scene(
        &mut self,
        log: &LogContext,
        rasterizer: &mut sw::Rasterizer,
        scene: &Scene,
        clip_rect: Rect2<i32>,
        flags: u64,
        context: CInstancedRasterPassContext,
    ) -> Result<(), CError> {
        if flags != 0 {
            return Err(CError::new(
                ErrorKind::InvalidArgument,
                "non-zero flags passed to instanced render",
            ));
        }

        assert!(
            (*self).output_pieces.is_empty(),
            "output piece buffer isn't empty, did you forget to call `sbr_instanced_raster_pass_finish`?"
        );
        assert!(self.current.is_none());

        let cull_rect = Rect2S::new(
            Point2::new(FixedS::new(clip_rect.min.x), FixedS::new(clip_rect.min.y)),
            Point2::new(FixedS::new(clip_rect.max.x), FixedS::new(clip_rect.max.y)),
        );
        rasterizer
            .render_scene_pieces(log, scene, cull_rect, &mut |piece| {
                if piece.size.x == 0 || piece.size.y == 0 {
                    return;
                }

                self.output_pieces.push(piece);
            })
            // Make sure piece buffer is cleared if rendering fails
            // so the above assertion is not triggered in such a case.
            .inspect_err(|_| self.output_pieces.clear())
            .map_err(CError::from_error);

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
                images: &mut self.output_images,
                instances: &mut self.output_instances,
                _lifetime: PhantomData,
            },
            self.output_pieces.iter(),
            clip_rect,
            rasterizer,
        );

        rasterizer.advance_cache_generation();

        trace!(
            log,
            "Rasterized to {} images and {} instances",
            self.output_images.len(),
            self.output_instances.len()
        );

        if !self.output_instances.is_empty() {
            let len = self.output_instances.len();
            let mut current = self.output_instances.as_mut_ptr();
            let mut next = current.wrapping_add(1);
            let end = unsafe { current.add(len) };
            loop {
                unsafe {
                    (*current).base.ptr = self.output_images.as_ptr().add((*current).base.idx);
                    if next == end {
                        break;
                    }
                    (*current).next = next;
                }
                current = next;
                next = next.wrapping_add(1);
            }
        }

        self.current = Some(context);

        Ok(())
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_instanced_raster_pass_get_instances(
    pass: *mut CInstancedRasterPass,
) -> *const COutputInstance<'static> {
    if (*pass).output_instances.is_empty() {
        std::ptr::null()
    } else {
        (*pass).output_instances.as_ptr()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_output_image_rasterize_into(
    image: *const COutputImage,
    pass: *mut CInstancedRasterPass,
    off_x: i32,
    off_y: i32,
    buffer: *mut Premultiplied<BGRA8>,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let rasterizer = &mut *(*pass).current.as_ref().unwrap().rasterizer();
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
unsafe extern "C" fn sbr_instanced_raster_pass_finish(pass: *mut CInstancedRasterPass) {
    (*pass)
        .current
        .take()
        .expect("sbr_instanced_raster_pass_finish called on inactive raster pass")
        .finish();

    (*pass).output_instances.clear();
    (*pass).output_images.clear();
    (*pass).output_pieces.clear();
}
