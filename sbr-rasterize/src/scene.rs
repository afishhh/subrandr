use std::{any::Any, fmt::Debug, rc::Rc, sync::Arc};

use util::{
    math::{I16Dot16, I26Dot6, Point2, Rect2, Vec2},
    AnyError,
};

use crate::{
    color::BGRA8,
    sw::{self, Strips},
    Rasterizer, Texture,
};

pub type FixedS = I26Dot6;
pub type Point2S = Point2<I26Dot6>;
pub type Vec2S = Vec2<I26Dot6>;
pub type Rect2S = Rect2<I26Dot6>;

#[derive(Clone)]
pub enum SceneNode {
    DeferredBitmaps(DeferredBitmaps),
    Bitmap(Bitmap),
    StrokedPolyline(StrokedPolyline),
    FilledRect(FilledRect),
    Subscene(Subscene),
}

// HACK: Instead do proper generic outline rasterization
//       Will need a way to keep the FreeType option most likely.
#[derive(Clone)]
pub struct DeferredBitmaps {
    pub to_bitmaps:
        Rc<dyn Fn(&mut dyn Rasterizer, &(dyn Any + 'static)) -> Result<Vec<Bitmap>, AnyError>>,
}

#[derive(Clone)]
pub struct Bitmap {
    pub pos: Point2<i32>,
    pub texture: Texture,
    pub filter: Option<BitmapFilter>,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy)]
pub enum BitmapFilter {
    ExtractAlpha,
}

#[derive(Debug, Clone)]
pub struct StrokedPolyline {
    pub polyline: Vec<Point2<I16Dot16>>,
    pub width: I16Dot16,
    pub color: BGRA8,
}

#[derive(Clone, Copy)]
pub struct FilledRect {
    pub rect: Rect2S,
    pub color: BGRA8,
}

// TODO: bitmaps are not currently offset when part of a subscene
//       but this is unused so don't care
#[derive(Clone)]
pub struct Subscene {
    pub pos: Point2S,
    pub scene: Arc<[SceneNode]>,
}

impl StrokedPolyline {
    pub fn to_strips(&self, pos: Point2S) -> (Point2<i32>, Vec2<u32>, Strips) {
        let mut bbox = Rect2::bounding_box_of_points(self.polyline.iter().copied());
        bbox.expand(self.width, self.width);

        // TODO: I've implemented this or similar logic like 10 times
        //       already can we split this out into a function please??
        let pos16 = Point2::new(
            I16Dot16::from_raw(pos.x.into_raw() << 10),
            I16Dot16::from_raw(pos.y.into_raw() << 10),
        );
        let input_shift = Vec2::new(
            (bbox.min.x.fract() + pos16.x.fract()).fract() - bbox.min.x,
            (bbox.min.y.fract() + pos16.y.fract()).fract() - bbox.min.y,
        );
        let output_pos = Point2::new(
            (bbox.min.x + pos16.x).floor_to_inner(),
            (bbox.min.y + pos16.y).floor_to_inner(),
        );
        let output_size = Vec2::new(
            ((bbox.max.x + pos16.x.fract()).ceil_to_inner()
                - (bbox.min.x + pos16.x.fract()).floor_to_inner()) as u32,
            ((bbox.max.y + pos16.y.fract()).ceil_to_inner()
                - (bbox.min.y + pos16.y.fract()).floor_to_inner()) as u32,
        );

        let mut strip_rasterizer = sw::StripRasterizer::new();
        strip_rasterizer.stroke_polyline(
            self.polyline.iter().copied().map(|mut p| {
                p += input_shift;
                Point2::new(p.x.into_f32(), p.y.into_f32())
            }),
            self.width.into_f32() / 2.,
        );

        (output_pos, output_size, strip_rasterizer.rasterize())
    }

    pub fn to_bitmap(&self, pos: Point2S, rasterizer: &mut dyn Rasterizer) -> Bitmap {
        let (ipos, size, strips) = self.to_strips(pos);

        let texture = unsafe {
            rasterizer.create_texture_mapped(
                size.x,
                size.y,
                super::PixelFormat::Mono,
                Box::new(|buffer, stride| {
                    buffer.fill(std::mem::MaybeUninit::zeroed());

                    strips.blend_to(
                        buffer,
                        |out, value| {
                            out.write(value);
                        },
                        size.x as usize,
                        size.y as usize,
                        stride,
                    );
                }),
            )
        };

        Bitmap {
            pos: ipos,
            texture,
            filter: None,
            color: self.color,
        }
    }
}
