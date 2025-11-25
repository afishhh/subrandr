use rasterize::{color::BGRA8, sw::GlyphRasterizer, Rasterizer, RenderTarget};
use thiserror::Error;
use util::math::{I16Dot16, Point2, Rect2, Vec2};

use crate::{
    display::{Drawing, DrawingNode, PaintOp, PositionedDrawing, Text, TextKind},
    layout::{FixedL, Point2L, Rect2L},
    text::{self, GlyphBitmap, GlyphRenderError},
};

#[derive(Debug, Error)]
pub enum RasterError {
    #[error("Failed to render glyph")]
    GlyphRender(#[from] GlyphRenderError),
}

fn text_to_bitmaps(
    rasterizer: &mut dyn Rasterizer,
    glyph_cache: &text::GlyphCache,
    text: &Text<'_>,
) -> Result<Vec<GlyphBitmap>, GlyphRenderError> {
    let blur_stddev = match text.kind {
        crate::display::TextKind::Normal { .. } => FixedL::ZERO,
        crate::display::TextKind::Shadow { blur_stddev, .. } => blur_stddev,
    };

    let mut glyphs = text::render(
        glyph_cache,
        rasterizer,
        text.pos.x.fract(),
        text.pos.y.fract(),
        blur_stddev.into_f32(),
        &mut text.glyphs.iter_glyphs_visual(),
    )?;

    for glyph in &mut glyphs {
        if let TextKind::Shadow { .. } = &text.kind {
            // TODO: Make this part of text::render and cache this
            if !glyph.texture.is_mono() {
                let mut tex = rasterizer
                    .create_mono_texture_rendered(glyph.texture.width(), glyph.texture.height());
                rasterizer.blit_to_mono_texture(&mut tex, 0, 0, &glyph.texture);
                glyph.texture = rasterizer.finalize_texture_render(tex)
            };
        }
    }

    Ok(glyphs)
}

pub struct DrawingBitmap {
    offset: Point2<i32>,
    texture: rasterize::Texture,
}

fn drawing_to_bitmap(
    rasterizer: &mut dyn Rasterizer,
    pos: Point2L,
    drawing: &Drawing,
) -> DrawingBitmap {
    let mut bbox = Rect2::NOTHING;

    for node in &drawing.nodes {
        match node {
            DrawingNode::StrokedPolyline(stroked_polyline) => {
                let mut polyline_bbox =
                    Rect2::bounding_box_of_points(stroked_polyline.polyline.iter().copied());
                polyline_bbox.expand(stroked_polyline.width, stroked_polyline.width);
                bbox.expand_to_rect(polyline_bbox);
            }
        }
    }

    let pos16 = Point2::new(
        I16Dot16::from_raw(pos.x.into_raw() << 10),
        I16Dot16::from_raw(pos.y.into_raw() << 10),
    );
    let texture_size = Vec2::new(
        ((bbox.max.x + pos16.x.fract()).ceil_to_inner()
            - (bbox.min.x + pos16.x.fract()).floor_to_inner()) as u32,
        ((bbox.max.y + pos16.y.fract()).ceil_to_inner()
            - (bbox.min.y + pos16.y.fract()).floor_to_inner()) as u32,
    );
    let final_pos = Point2::new(
        (bbox.min.x + pos16.x).trunc_to_inner(),
        (bbox.min.y + pos16.y).trunc_to_inner(),
    );

    let mut glyph_rasterizer = GlyphRasterizer::new();
    let texture = unsafe {
        rasterizer.create_texture_mapped(
            texture_size.x,
            texture_size.y,
            rasterize::PixelFormat::Bgra,
            Box::new(|buffer, stride| {
                buffer.fill(std::mem::MaybeUninit::zeroed());

                let n_pixels = buffer.len() / 4;
                let pixel_stride = stride / 4;
                let pixels: &mut [BGRA8] =
                    std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast(), n_pixels);

                for node in &drawing.nodes {
                    glyph_rasterizer.reset(texture_size);
                    let color = match node {
                        DrawingNode::StrokedPolyline(polyline) => {
                            glyph_rasterizer.stroke_polyline(
                                polyline.polyline.iter().copied().map(|p| {
                                    Point2::new(
                                        p.x.into_f32() - bbox.min.x.into_f32().min(0.),
                                        p.y.into_f32() - bbox.min.y.into_f32().min(0.),
                                    )
                                }),
                                polyline.width.into_f32() / 2.,
                            );
                            polyline.color
                        }
                    };

                    glyph_rasterizer.rasterize(|y, xs, v| {
                        let row_start = y as usize * pixel_stride;
                        let start = row_start + xs.start as usize;
                        let end = row_start + xs.end as usize;
                        for pixel in pixels.get_unchecked_mut(start..end) {
                            *pixel = color.mul_alpha((v >> 8) as u8).blend_over(*pixel).0;
                        }
                    });
                }
            }),
        )
    };

    DrawingBitmap {
        offset: final_pos,
        texture,
    }
}

pub struct RasterContext<'r, R: Rasterizer + ?Sized> {
    pub rasterizer: &'r mut R,
    pub glyph_cache: &'r text::GlyphCache,
}

pub fn rasterize_to_target(
    ctx: &mut RasterContext<'_, dyn Rasterizer + '_>,
    target: &mut RenderTarget,
    ops: &[PaintOp],
) -> Result<(), RasterError> {
    for op in ops {
        match op {
            PaintOp::Text(text) => {
                let bitmaps = text_to_bitmaps(ctx.rasterizer, ctx.glyph_cache, text)?;
                let color = match text.kind {
                    TextKind::Normal { mono_color } => mono_color,
                    TextKind::Shadow { color, .. } => color,
                };

                let ipos = Point2::new(text.pos.x.floor_to_inner(), text.pos.y.floor_to_inner());
                for bitmap in bitmaps {
                    ctx.rasterizer.blit(
                        target,
                        ipos.x + bitmap.offset.x,
                        ipos.y + bitmap.offset.y,
                        &bitmap.texture,
                        color,
                    );
                }
            }
            PaintOp::Rect(fill) => {
                ctx.rasterizer.fill_axis_aligned_rect(
                    target,
                    Rect2L::to_float(fill.rect),
                    fill.color,
                );
            }
            &PaintOp::Drawing(PositionedDrawing { pos, ref drawing }) => {
                let bitmap = drawing_to_bitmap(ctx.rasterizer, pos, drawing);

                ctx.rasterizer.blit(
                    target,
                    bitmap.offset.x,
                    bitmap.offset.y,
                    &bitmap.texture,
                    BGRA8::WHITE,
                )
            }
        }
    }

    Ok(())
}
