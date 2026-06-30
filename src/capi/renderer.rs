use std::{ffi::c_int, ptr::NonNull};

use log::info;
use rasterize::color::{Premultiplied, BGRA8};
use util::math::Rect2;

use crate::{
    capi::{
        instanced_raster::{CInstancedRasterPass, CInstancedRasterPassContext},
        library::CLibrary,
    },
    Renderer, SubtitleContext, Subtitles,
};

pub(super) struct CRenderer {
    pub(super) lib: *const CLibrary,
    pub(super) inner: Renderer,
    pub(super) rasterizer: rasterize::sw::Rasterizer,
    pub(super) instanced_pass: CInstancedRasterPass,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_create(lib: *const CLibrary) -> *mut CRenderer {
    if !(*lib).did_log_version.get() {
        (*lib).did_log_version.set(true);
        info!(
            (*lib).root_logger,
            concat!(
                "subrandr version ",
                env!("CARGO_PKG_VERSION"),
                env!("BUILD_REV_SUFFIX"),
                env!("BUILD_DIRTY")
            )
        );
    }

    Box::into_raw(Box::new(CRenderer {
        lib,
        inner: ctry!(Renderer::new(
            &(*lib).root_logger.new_ctx(),
            (*lib).debug_flags.clone()
        )),
        rasterizer: rasterize::sw::Rasterizer::new(),
        instanced_pass: CInstancedRasterPass::new(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_set_subtitles(
    renderer: *mut CRenderer,
    subtitles: *const Subtitles,
) {
    (*renderer).inner.set_subtitles(subtitles.as_ref());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_did_change(
    renderer: *mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
) -> bool {
    (*renderer).inner.did_change(&*ctx, t)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render(
    renderer: *mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
    buffer: *mut Premultiplied<BGRA8>,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let buffer = std::slice::from_raw_parts_mut(buffer, stride as usize * height as usize);
    let log = &(*(*renderer).lib).root_logger.new_ctx();
    let target = rasterize::sw::RenderTarget::new(buffer, width, height, stride);
    ctry!((*renderer)
        .inner
        .render(log, &*ctx, t, target, &mut (*renderer).rasterizer));
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render_instanced(
    renderer: *mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
    clip_rect: Rect2<i32>,
    flags: u64,
) -> *mut CInstancedRasterPass {
    if clip_rect.is_empty() {
        return &raw mut (*renderer).instanced_pass;
    }

    let log = &(*(*renderer).lib).root_logger.new_ctx();
    let core_renderer = &mut (*renderer).inner;
    let rasterizer = &mut (*renderer).rasterizer;
    ctry!(core_renderer.render_to_scene(log, &*ctx, t, rasterizer));

    (*renderer).instanced_pass.render_scene(
        log,
        rasterizer,
        core_renderer.scene(),
        clip_rect,
        flags,
        CInstancedRasterPassContext::Renderer(NonNull::new(renderer).unwrap()),
    );

    return &raw mut (*renderer).instanced_pass;
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_destroy(renderer: *mut CRenderer) {
    drop(Box::from_raw(renderer));
}
