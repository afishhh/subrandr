use std::mem::ManuallyDrop;

use util::{math::Point2, vec_parts};

use super::CRenderer;
use crate::{raster::PieceCollector, SubtitleContext};

// HACK: the publicity of these types is a temporary hack

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum COutputPieceKind {
    ImageA8 = 0,
    ImageBGRA8 = 1,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COutputPieceImage {
    pos: Point2<i32>,
    width: u32,
    height: u32,
    stride: u32,
    data: COutputPieceImageData,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COutputPieceImageA8 {
    buffer: *const u8,
    color: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COutputPieceImageBGRA8 {
    buffer: *const u32,
    alpha: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union COutputPieceImageData {
    a8: COutputPieceImageA8,
    bgra8: COutputPieceImageBGRA8,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union COutputPieceData {
    image: COutputPieceImage,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct COutputPiece {
    kind: COutputPieceKind,
    data: COutputPieceData,
}

struct PieceWriter<'r> {
    textures: &'r mut Vec<rasterize::sw::Texture>,
    buffer: ManuallyDrop<Vec<COutputPiece>>,
    parts: &'r mut (*mut COutputPiece, usize),
}

impl PieceCollector for PieceWriter<'_> {
    fn emit_image_a8(
        &mut self,
        image: crate::raster::OutputImageA8,
        _rasterizer: &mut rasterize::sw::Rasterizer,
    ) {
        self.buffer.push(COutputPiece {
            kind: COutputPieceKind::ImageA8,
            data: COutputPieceData {
                image: COutputPieceImage {
                    pos: image.pos,
                    width: image.texture.width(),
                    height: image.texture.height(),
                    stride: image.texture.stride(),
                    data: COutputPieceImageData {
                        a8: COutputPieceImageA8 {
                            buffer: image.texture.data_ptr().cast(),
                            color: image.color.to_rgba32(),
                        },
                    },
                },
            },
        });
        self.textures.push(image.texture);
    }

    fn emit_image_bgra8(
        &mut self,
        image: crate::raster::OutputImageBGRA8,
        _rasterizer: &mut rasterize::sw::Rasterizer,
    ) {
        self.buffer.push(COutputPiece {
            kind: COutputPieceKind::ImageBGRA8,
            data: COutputPieceData {
                image: COutputPieceImage {
                    pos: image.pos,
                    width: image.texture.width(),
                    height: image.texture.height(),
                    stride: image.texture.stride(),
                    data: COutputPieceImageData {
                        bgra8: COutputPieceImageBGRA8 {
                            buffer: image.texture.data_ptr().cast(),
                            alpha: image.alpha,
                        },
                    },
                },
            },
        });
        self.textures.push(image.texture);
    }
}

impl Drop for PieceWriter<'_> {
    fn drop(&mut self) {
        let (ptr, _, cap) = vec_parts(&mut self.buffer);
        *self.parts = (ptr, cap);
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_renderer_render_pieces(
    renderer: &mut CRenderer,
    ctx: *const SubtitleContext,
    t: u32,
    flags: i32,
) -> *const COutputPiece {
    if flags != 0 {
        cthrow!(
            InvalidArgument,
            "sbr_renderer_render_pieces does not currently support any flags"
        )
    }

    let (ptr, cap) = renderer.piece_buffer_parts;
    renderer.piece_textures.clear();
    ctry!(renderer.inner.render_pieces(
        &*ctx,
        t,
        &mut PieceWriter {
            textures: &mut renderer.piece_textures,
            buffer: ManuallyDrop::new(Vec::from_raw_parts(ptr, 0, cap)),
            parts: &mut renderer.piece_buffer_parts,
        },
    ));

    renderer.piece_buffer_parts.0.cast_const()
}
