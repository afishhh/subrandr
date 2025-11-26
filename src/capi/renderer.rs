use std::ffi::c_int;

use rasterize::color::BGRA8;

use crate::{Renderer, Subrandr, SubtitleContext, Subtitles};

mod piece_render;

pub struct CRenderer {
    pub(super) inner: Renderer<'static>,
    rasterizer: rasterize::sw::Rasterizer,
    output_pieces: Vec<piece_render::COutputPiece>,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_create(sbr: *mut Subrandr) -> *mut CRenderer {
    Box::into_raw(Box::new(CRenderer {
        inner: ctry!(Renderer::new(&*sbr)),
        rasterizer: rasterize::sw::Rasterizer::new(),
        output_pieces: Vec::new(),
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
    buffer: *mut BGRA8,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let buffer = std::slice::from_raw_parts_mut(buffer, stride as usize * height as usize);
    ctry!((*renderer)
        .inner
        .render(&*ctx, t, buffer, width, height, stride));
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_destroy(renderer: *mut CRenderer) {
    drop(Box::from_raw(renderer));
}
