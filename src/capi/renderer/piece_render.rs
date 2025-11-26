use std::ffi::c_int;

use rasterize::color::BGRA8;
use util::math::{Point2, Vec2};

use super::CRenderer;
use crate::{
    raster::{rasterize_to_pieces, OutputPiece, OutputPieceContent},
    SubtitleContext,
};

#[repr(C)]
pub(super) struct COutputPiece {
    pos: Point2<i32>,
    size: Vec2<u32>,
    next: *const COutputPiece,
    content: OutputPieceContent,
}

impl COutputPiece {
    fn from_output_piece(piece: OutputPiece) -> Self {
        Self {
            pos: piece.pos,
            size: piece.size,
            next: std::ptr::null(),
            content: piece.content,
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render_pieces(
    renderer: *mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
) -> *mut CRenderer {
    {
        let renderer = &mut (*renderer);
        assert!(renderer.output_pieces.is_empty(), "Output piece buffer isn't empty, did you forget to call `sbr_piece_raster_pass_finish`?");

        ctry!(renderer.inner.render_to_ops(&*ctx, t, &renderer.rasterizer));
        ctry!(rasterize_to_pieces(
            &mut renderer.rasterizer,
            &mut crate::raster::RasterContext {
                glyph_cache: &renderer.inner.glyph_cache
            },
            renderer.inner.paint_ops(),
            &mut |piece| {
                if piece.size.x == 0 || piece.size.y == 0 {
                    return;
                }

                renderer
                    .output_pieces
                    .push(COutputPiece::from_output_piece(piece));
            }
        ));

        if !renderer.output_pieces.is_empty() {
            let len = renderer.output_pieces.len();
            let mut current = renderer.output_pieces.as_mut_ptr();
            let mut next = current.add(1);
            let end = current.add(len);
            while next != end {
                (*current).next = next;
                current = next;
                next = next.add(1);
            }
        }
    }

    renderer
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_piece_raster_pass_get_pieces(
    renderer: *mut CRenderer,
) -> *const COutputPiece {
    if (*renderer).output_pieces.is_empty() {
        std::ptr::null()
    } else {
        (*renderer).output_pieces.as_ptr()
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_piece_raster_pass_draw_piece(
    renderer: *mut CRenderer,
    piece: *const COutputPiece,
    off_x: i32,
    off_y: i32,
    buffer: *mut BGRA8,
    width: u32,
    height: u32,
    stride: u32,
) -> c_int {
    let rasterizer = &mut (*renderer).rasterizer;
    let mut target = rasterize::sw::create_render_target(
        std::slice::from_raw_parts_mut(buffer, height as usize * stride as usize),
        width,
        height,
        stride,
    );
    (*piece)
        .content
        .rasterize_to(rasterizer, &mut target, Point2::new(off_x, off_y));

    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_piece_raster_pass_finish(renderer: *mut CRenderer) {
    (*renderer).inner.end_raster();
    (*renderer).output_pieces.clear();
}
