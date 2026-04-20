use std::ffi::c_int;

use log::{info, LogContext};
use rasterize::{
    color::{Premultiplied, BGRA8},
    sw::OutputPiece,
};

use crate::{capi::library::CLibrary, Config, Renderer, SubtitleContext, Subtitles};

mod instanced;

pub struct CRenderer<'lib> {
    lib: &'lib CLibrary,
    config: *const Config,
    pub(super) inner: Renderer,
    rasterizer: rasterize::sw::Rasterizer,
    output_pieces: Vec<OutputPiece>,
    output_images: Vec<instanced::COutputImage<'static>>,
    output_instances: Vec<instanced::COutputInstance<'static>>,
}

impl CRenderer<'_> {
    unsafe fn get_or_init_config<'a>(renderer: *mut CRenderer<'a>, log: &LogContext) -> &'a Config {
        if !(*renderer).config.is_null() {
            &*(*renderer).config
        } else {
            (*renderer).lib.get_or_init_config(log)
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_create(lib: &CLibrary) -> *mut CRenderer<'_> {
    if !lib.did_log_version.get() {
        lib.did_log_version.set(true);
        info!(
            lib.root_logger,
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
        config: std::ptr::null_mut(),
        inner: ctry!(Renderer::new(&lib.root_logger.new_ctx())),
        rasterizer: rasterize::sw::Rasterizer::new(),
        output_pieces: Vec::new(),
        output_images: Vec::new(),
        output_instances: Vec::new(),
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
unsafe extern "C" fn sbr_renderer_set_config(
    renderer: *mut CRenderer,
    config: *const crate::Config,
) {
    (*renderer).config = config;
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
    let log = &(*renderer).lib.root_logger.new_ctx();
    let cfg = CRenderer::get_or_init_config(renderer, log);
    ctry!((*renderer)
        .inner
        .render(log, &*ctx, cfg, t, buffer, width, height, stride));
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_destroy(renderer: *mut CRenderer) {
    drop(Box::from_raw(renderer));
}
