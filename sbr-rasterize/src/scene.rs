use std::{any::Any, convert::Infallible, fmt::Debug, rc::Rc};

use util::{
    math::{I16Dot16, I26Dot6, Outline, OutlineEvent, OutlineIterExt, Point2, Rect2, Vec2},
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
pub struct Scene(pub(crate) Rc<[SceneNode]>);

impl Scene {
    pub fn empty() -> Self {
        Self(Rc::default())
    }
}

#[derive(Clone)]
pub(crate) enum SceneNode {
    DeferredBitmaps(DeferredBitmaps),
    Bitmap(Bitmap),
    StrokedPolyline(StrokedPolyline),
    FilledOutline(FilledOutline),
    FilledRect(FilledRect),
    Subscene(Subscene),
}

pub struct SceneBuilder {
    nodes: Vec<SceneNode>,
}

impl SceneBuilder {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.nodes.clear();
    }

    pub fn root(&mut self) -> SceneContentBuilder<'_> {
        SceneContentBuilder {
            parent: self,
            current_translation: Vec2S::ZERO,
        }
    }

    pub fn finish(&mut self) -> Scene {
        Scene({
            let mut result = Vec::with_capacity(self.nodes.len());
            result.append(&mut self.nodes);
            result.into()
        })
    }
}

pub struct SceneContentBuilder<'a> {
    parent: &'a mut SceneBuilder,
    current_translation: Vec2S,
}

impl<'a> SceneContentBuilder<'a> {
    pub fn child(&mut self) -> SceneContentBuilder<'_> {
        SceneContentBuilder {
            parent: self.parent,
            ..*self
        }
    }

    pub fn apply_translation(&mut self, translation: Vec2S) -> &mut SceneContentBuilder<'a> {
        self.current_translation += translation;
        self
    }

    pub fn with_translation(&mut self, translation: Vec2S) -> SceneContentBuilder<'_> {
        let mut child = self.child();
        child.apply_translation(translation);
        child
    }

    fn rounded_translation(&self) -> Vec2<i32> {
        Vec2::new(
            self.current_translation.x.round_to_inner(),
            self.current_translation.y.round_to_inner(),
        )
    }

    pub fn deferred_bitmaps(
        &mut self,
        to_bitmaps: Rc<
            dyn Fn(&mut dyn Rasterizer, &(dyn Any + 'static)) -> Result<Vec<Bitmap>, AnyError>,
        >,
    ) {
        self.parent
            .nodes
            .push(SceneNode::DeferredBitmaps(DeferredBitmaps {
                translation: self.rounded_translation(),
                to_bitmaps,
            }));
    }

    pub fn bitmap(&mut self, texture: Texture, filter: Option<BitmapFilter>, color: BGRA8) {
        self.parent.nodes.push(SceneNode::Bitmap(Bitmap {
            pos: self.rounded_translation().to_point(),
            texture,
            filter,
            color,
        }));
    }

    pub fn stroked_polyline(
        &mut self,
        polyline: Vec<Point2<I16Dot16>>,
        width: I16Dot16,
        color: BGRA8,
    ) {
        self.parent
            .nodes
            .push(SceneNode::StrokedPolyline(StrokedPolyline {
                pos: self.current_translation.to_point(),
                polyline,
                width,
                color,
            }));
    }

    pub fn filled_outline(&mut self, outline: impl Outline<f32>, color: BGRA8) {
        let transf = Vec2::new(
            self.current_translation.x.into_f32(),
            self.current_translation.y.into_f32(),
        );

        self.parent
            .nodes
            .push(SceneNode::FilledOutline(FilledOutline {
                events: outline.iter().map_points(|point| point + transf).collect(),
                color,
            }));
    }

    pub fn filled_rect(&mut self, rect: Rect2S, color: BGRA8) {
        self.parent.nodes.push(SceneNode::FilledRect(FilledRect {
            rect: rect.translate(self.current_translation),
            color,
        }));
    }

    pub fn try_subscene<E>(
        &mut self,
        scene_filter: Option<SceneFilter>,
        color: BGRA8,
        content_fn: impl FnOnce(Point2S) -> Result<SubsceneKind, E>,
    ) -> Result<(), E> {
        let floored_pos = Vec2::new(
            self.current_translation.x.floor_to_inner(),
            self.current_translation.y.floor_to_inner(),
        );

        self.parent.nodes.push(SceneNode::Subscene(Subscene {
            pos: floored_pos.to_point(),
            scene_filter,
            kind: content_fn((self.current_translation - floored_pos).to_point())?,
            color,
        }));

        Ok(())
    }

    pub fn subscene(
        &mut self,
        scene_filter: Option<SceneFilter>,
        color: BGRA8,
        content_fn: impl FnOnce(Point2S) -> SubsceneKind,
    ) {
        match self.try_subscene(scene_filter, color, |translation| {
            Ok::<_, Infallible>(content_fn(translation))
        }) {
            Ok(()) => (),
        }
    }
}

// HACK: Instead do proper generic outline rasterization
//       Will need a way to keep the FreeType option most likely.
#[derive(Clone)]
pub(crate) struct DeferredBitmaps {
    pub(crate) translation: Vec2<i32>,
    pub(crate) to_bitmaps:
        Rc<dyn Fn(&mut dyn Rasterizer, &(dyn Any + 'static)) -> Result<Vec<Bitmap>, AnyError>>,
}

#[derive(Clone)]
pub struct Bitmap {
    pub pos: Point2<i32>,
    pub texture: Texture,
    pub filter: Option<BitmapFilter>,
    pub color: BGRA8,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum BitmapFilter {
    ExtractAlpha,
}

#[derive(Debug, Clone)]
pub(crate) struct StrokedPolyline {
    pub(crate) pos: Point2S,
    pub(crate) polyline: Vec<Point2<I16Dot16>>,
    pub(crate) width: I16Dot16,
    pub(crate) color: BGRA8,
}

#[derive(Debug, Clone)]
pub(crate) struct FilledOutline {
    pub(crate) events: Rc<[OutlineEvent<f32>]>,
    pub(crate) color: BGRA8,
}

#[derive(Clone, Copy)]
pub(crate) struct FilledRect {
    pub(crate) rect: Rect2S,
    pub(crate) color: BGRA8,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum SceneFilter {
    ExtractAlpha { blur_stddev: I26Dot6 },
}

#[derive(Clone)]
pub(crate) struct Subscene {
    pub(crate) pos: Point2<i32>,
    pub(crate) kind: SubsceneKind,
    pub(crate) scene_filter: Option<SceneFilter>,
    pub(crate) color: BGRA8,
}

#[derive(Clone)]
pub enum SubsceneKind {
    External(Rc<dyn ExternalSubscene>),
    Scene(Scene),
}

pub trait ExternalSubscene {
    fn bounding_box(&self) -> Rect2S;
    fn rasterize(&self, rasterizer: &mut dyn Rasterizer) -> Result<(Vec2<i32>, Texture), AnyError>;
}

impl StrokedPolyline {
    pub fn to_strips(&self) -> (Point2<i32>, Vec2<u32>, Strips) {
        let mut bbox = Rect2::bounding_box_of_points(self.polyline.iter().copied());
        bbox.expand(self.width, self.width);

        // TODO: I've implemented this or similar logic like 10 times
        //       already can we split this out into a function please??
        let pos16 = Point2::new(
            I16Dot16::from_raw(self.pos.x.into_raw() << 10),
            I16Dot16::from_raw(self.pos.y.into_raw() << 10),
        );
        let input_shift = Vec2::new(bbox.min.x.floor(), bbox.min.y.floor());
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
                p -= input_shift;
                Point2::new(p.x.into_f32(), p.y.into_f32())
            }),
            self.width.into_f32() / 2.,
        );

        (output_pos, output_size, strip_rasterizer.rasterize())
    }
}

impl FilledOutline {
    pub fn to_strips(&self) -> (Point2<i32>, Vec2<u32>, Strips) {
        let bbox = self.events.control_box();

        let output_pos = Point2::new(bbox.min.x.floor() as i32, bbox.min.y.floor() as i32);
        let input_shift = Vec2::new(output_pos.x as f32, output_pos.y as f32);
        let output_size = Vec2::new(
            (bbox.max.x.ceil() as i32 - output_pos.x) as u32,
            (bbox.max.y.ceil() as i32 - output_pos.y) as u32,
        );

        let mut strip_rasterizer = sw::StripRasterizer::new();
        strip_rasterizer.add_outline(self.events.iter().copied().map_points(|mut p| {
            p -= input_shift;
            Point2::new(p.x, p.y)
        }));

        (output_pos, output_size, strip_rasterizer.rasterize())
    }
}
