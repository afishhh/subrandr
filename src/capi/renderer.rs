use std::{
    ffi::{c_char, c_int, CStr},
    sync::Arc,
};

use rasterize::color::BGRA8;
use util::{math::I16Dot16, vec_into_parts};

use crate::{
    text::{Face, FontAxisValues, OpenTypeTag},
    Renderer, Subrandr, SubtitleContext, Subtitles,
};

mod piece_render;
use piece_render::COutputPiece;

pub struct CRenderer {
    inner: Renderer<'static>,

    // Textures currently referenced by pieces in `piece_buffer`.
    // (stored here so they are kept alive)
    piece_textures: Vec<rasterize::sw::Texture>,
    piece_buffer_parts: (*mut COutputPiece, usize),
}

impl Drop for CRenderer {
    fn drop(&mut self) {
        unsafe {
            let (ptr, cap) = self.piece_buffer_parts;
            Vec::from_raw_parts(ptr, 0, cap);
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_create(sbr: *mut Subrandr) -> *mut CRenderer {
    Box::into_raw(Box::new(CRenderer {
        inner: ctry!(Renderer::new(&*sbr)),
        piece_textures: Vec::new(),
        piece_buffer_parts: {
            let (ptr, _, cap) = vec_into_parts(Vec::new());
            (ptr, cap)
        },
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
unsafe extern "C" fn sbr_renderer_clear_fonts(renderer: *mut CRenderer) {
    (*renderer).inner.fonts.clear_extra();
}

// This is very unstable: the variable font handling will probably have to change in the future
// to hold a supported weight range
// TODO: add to header and test
// TODO: add width
#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_add_font(
    renderer: *mut CRenderer,
    family: *const c_char,
    weight: f32,
    italic: bool,
    font: *mut Face,
) -> i32 {
    let family = ctrywrap!(
        InvalidArgument("Path is not valid UTF-8"),
        CStr::from_ptr(family).to_str()
    );
    (*renderer).inner.fonts.add_extra(crate::text::FaceInfo {
        family_names: Arc::new([family.into()]),
        width: FontAxisValues::Fixed(I16Dot16::new(100)),
        weight: match weight {
            f if f.is_nan() => (*font).axis(OpenTypeTag::AXIS_WEIGHT).map_or_else(
                || FontAxisValues::Fixed((*font).weight()),
                |axis| FontAxisValues::Range(axis.minimum, axis.maximum),
            ),
            f if (0.0..1000.0).contains(&f) => {
                crate::text::FontAxisValues::Fixed(I16Dot16::from_f32(f))
            }
            _ => cthrow!(InvalidArgument, "Font weight out of range"),
        },
        italic,
        source: crate::text::FontSource::Memory((*font).clone()),
    });

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_destroy(renderer: *mut CRenderer) {
    drop(Box::from_raw(renderer));
}
