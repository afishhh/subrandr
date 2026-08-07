use std::{convert::Infallible, fmt::Debug, ops::Mul, rc::Rc};

use util::{
    math::{
        I16Dot16, I26Dot6, Number, Outline, OutlineEvent, OutlineIterExt, Point2, Rect2, Signed,
        Vec2,
    },
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

impl Debug for Scene {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Scene@{:?}", Rc::as_ptr(&self.0) as *const ())
    }
}

impl Scene {
    pub fn empty() -> Self {
        Self(Rc::default())
    }

    pub(crate) fn bounding_box(&self) -> Rect2S {
        let mut bbox = Rect2S::NOTHING;

        for node in self.0.iter() {
            node.expand_bounding_box(&mut bbox);
        }

        bbox
    }

    pub(crate) fn is_mono(&self) -> bool {
        for node in self.0.iter() {
            if !node.is_mono() {
                return false;
            }
        }

        true
    }

    pub fn memory_footprint(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of_val::<[_]>(&*self.0)
            + self
                .0
                .iter()
                .map(|node| match node {
                    SceneNode::Bitmap(bitmap) => bitmap.texture.memory_footprint(),
                    SceneNode::StrokedPolyline(stroked_polyline) => {
                        std::mem::size_of_val::<[_]>(&stroked_polyline.polyline)
                    }
                    SceneNode::FilledRect(_) => 0,
                    SceneNode::FilledOutline(FilledOutline { events, .. }) => {
                        std::mem::size_of_val::<[_]>(events)
                    }
                    SceneNode::Subscene(Subscene {
                        kind: SubsceneKind::External(external),
                        ..
                    }) => std::mem::size_of_val(&**external),
                    SceneNode::Subscene(Subscene {
                        kind: SubsceneKind::Scene(child_scene),
                        ..
                    }) => child_scene.memory_footprint(),
                })
                .sum::<usize>()
    }
}

#[derive(Clone)]
pub(crate) enum SceneNode {
    Bitmap(Bitmap),
    StrokedPolyline(StrokedPolyline),
    FilledOutline(FilledOutline),
    FilledRect(FilledRect),
    Subscene(Subscene),
}

impl SceneNode {
    pub(crate) fn expand_bounding_box(&self, bbox: &mut Rect2S) {
        match self {
            SceneNode::Bitmap(bitmap) => bbox.expand_to_rect(Rect2S::from_min_size(
                Point2::new(FixedS::new(bitmap.pos.x), FixedS::new(bitmap.pos.y)),
                Vec2::new(
                    FixedS::new(bitmap.scaled_size.x as i32),
                    FixedS::new(bitmap.scaled_size.y as i32),
                ),
            )),
            SceneNode::StrokedPolyline(polyline) => polyline
                .polyline
                .iter()
                .copied()
                .map(|p| {
                    p + Vec2::new(
                        I16Dot16::from_raw(polyline.pos.x.into_raw() << 10),
                        I16Dot16::from_raw(polyline.pos.y.into_raw() << 10),
                    )
                })
                .for_each(|p| {
                    bbox.expand_to_point(Point2::new(
                        FixedS::from_raw(p.x.into_raw() >> 10),
                        FixedS::from_raw(p.y.into_raw() >> 10),
                    ))
                }),
            SceneNode::FilledOutline(outline) => {
                let cbox = outline.events.control_box();
                bbox.expand_to_rect(Rect2S::new(
                    Point2::new(cbox.min.x.into(), cbox.min.y.into()),
                    Point2::new(cbox.max.x.into(), cbox.max.y.into()),
                ));
            }
            SceneNode::FilledRect(filled_rect) => {
                bbox.expand_to_rect(filled_rect.rect);
            }
            SceneNode::Subscene(subscene) => bbox.expand_to_rect(subscene.bounding_box()),
        }
    }

    pub(crate) fn bounding_box(&self) -> Rect2S {
        let mut result = Rect2::NOTHING;
        self.expand_bounding_box(&mut result);
        result
    }

    pub(crate) fn is_mono(&self) -> bool {
        match self {
            SceneNode::Bitmap(bitmap) => {
                bitmap.texture.is_mono() && bitmap.color == SceneColor::ACTIVE
            }
            &SceneNode::StrokedPolyline(StrokedPolyline { color, .. })
            | &SceneNode::FilledOutline(FilledOutline { color, .. })
            | &SceneNode::FilledRect(FilledRect { color, .. }) => color == SceneColor::ACTIVE,
            SceneNode::Subscene(subscene) => {
                subscene.active_color == SceneColor::ACTIVE
                    && match (&subscene.scene_filter, &subscene.kind) {
                        (Some(SceneFilter::ExtractAlpha { .. }), _) => true,
                        (None, SubsceneKind::External(_)) => false,
                        (None, SubsceneKind::Scene(scene)) => scene.is_mono(),
                    }
            }
        }
    }
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
            current_rotation: Rotation::None,
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

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rotation {
    #[default]
    None = 0,
    Clockwise90 = 1,
    FlipXY = 2,
    CounterClockwise90 = 3,
}

impl Mul<Rotation> for Rotation {
    type Output = Rotation;

    fn mul(self, rhs: Rotation) -> Self::Output {
        match (self as u8 + rhs as u8) % 4 {
            0 => Rotation::None,
            1 => Rotation::Clockwise90,
            2 => Rotation::FlipXY,
            3 => Rotation::CounterClockwise90,
            _ => unreachable!(),
        }
    }
}

impl Rotation {
    pub fn inverse(self) -> Self {
        match self {
            Rotation::None => Rotation::None,
            Rotation::Clockwise90 => Rotation::CounterClockwise90,
            Rotation::FlipXY => Rotation::FlipXY,
            Rotation::CounterClockwise90 => Rotation::Clockwise90,
        }
    }
}

impl<N: Signed> Mul<Vec2<N>> for Rotation {
    type Output = Vec2<N>;

    fn mul(self, rhs: Vec2<N>) -> Self::Output {
        match self {
            Rotation::None => rhs,
            Rotation::Clockwise90 => Vec2::new(-rhs.y, rhs.x),
            Rotation::FlipXY => Vec2::new(-rhs.x, -rhs.y),
            Rotation::CounterClockwise90 => Vec2::new(rhs.y, -rhs.x),
        }
    }
}

impl<N: Number + Signed> Mul<Rect2<N>> for Rotation {
    type Output = Rect2<N>;

    fn mul(self, rhs: Rect2<N>) -> Self::Output {
        match self {
            Rotation::None => Rect2::new(rhs.min, rhs.max),
            Rotation::Clockwise90 => Rect2::new(
                Point2::new(-rhs.max.y, rhs.min.x),
                Point2::new(-rhs.min.y, rhs.max.x),
            ),
            Rotation::FlipXY => Rect2::new(
                Point2::new(-rhs.max.x, -rhs.max.y),
                Point2::new(-rhs.min.x, -rhs.min.y),
            ),
            Rotation::CounterClockwise90 => Rect2::new(
                Point2::new(rhs.min.y, -rhs.max.x),
                Point2::new(rhs.max.y, -rhs.min.x),
            ),
        }
    }
}

pub struct SceneContentBuilder<'a> {
    parent: &'a mut SceneBuilder,
    current_translation: Vec2S,
    // Our transformation matrix. Yes, it only has four values :)
    current_rotation: Rotation,
}

impl<'a> SceneContentBuilder<'a> {
    pub fn child(&mut self) -> SceneContentBuilder<'_> {
        SceneContentBuilder {
            parent: self.parent,
            ..*self
        }
    }

    pub fn apply_translation(&mut self, translation: Vec2S) -> &mut SceneContentBuilder<'a> {
        self.current_translation += self.current_rotation * translation;
        self
    }

    pub fn apply_rotation(
        &mut self,
        rotation: Rotation,
        origin: Vec2S,
    ) -> &mut SceneContentBuilder<'a> {
        self.apply_translation(origin);
        self.current_rotation = rotation * self.current_rotation;
        self.apply_translation(-origin);
        self
    }

    pub fn with_translation(&mut self, translation: Vec2S) -> SceneContentBuilder<'_> {
        let mut child = self.child();
        child.apply_translation(translation);
        child
    }

    pub fn current_transform(&self) -> Rotation {
        self.current_rotation
    }

    pub fn current_translation(&self) -> Vec2S {
        self.current_translation
    }

    fn rounded_translation(&self) -> Vec2<i32> {
        Vec2::new(
            self.current_translation.x.round_to_inner(),
            self.current_translation.y.round_to_inner(),
        )
    }

    pub fn bitmap(
        &mut self,
        texture: Texture,
        mut scaled_size: Vec2<u32>,
        filter: Option<BitmapFilter>,
        color: impl Into<SceneColor>,
    ) {
        let mut pos = self.rounded_translation().to_point();

        match self.current_rotation {
            Rotation::None => (),
            Rotation::Clockwise90 => {
                pos.x -= scaled_size.y as i32;
                std::mem::swap(&mut scaled_size.x, &mut scaled_size.y);
            }

            Rotation::FlipXY => pos -= Vec2::new(scaled_size.x as i32, scaled_size.y as i32),
            Rotation::CounterClockwise90 => {
                pos.y -= scaled_size.x as i32;
                std::mem::swap(&mut scaled_size.x, &mut scaled_size.y);
            }
        }

        self.parent.nodes.push(SceneNode::Bitmap(Bitmap {
            pos,
            scaled_size,
            texture,
            filter,
            rotation: self.current_rotation,
            color: color.into(),
        }));
    }

    pub fn stroked_polyline(
        &mut self,
        polyline: Vec<Point2<I16Dot16>>,
        width: I16Dot16,
        color: impl Into<SceneColor>,
    ) {
        self.parent
            .nodes
            .push(SceneNode::StrokedPolyline(StrokedPolyline {
                pos: self.current_translation.to_point(),
                polyline,
                width,
                color: color.into(),
            }));
    }

    pub fn filled_outline(
        &mut self,
        outline: impl IntoIterator<Item = OutlineEvent<f32>>,
        color: impl Into<SceneColor>,
    ) {
        let transf = Vec2::new(
            self.current_translation.x.into_f32(),
            self.current_translation.y.into_f32(),
        );

        self.parent
            .nodes
            .push(SceneNode::FilledOutline(FilledOutline {
                events: outline
                    .into_iter()
                    .map_points(|point| {
                        (self.current_rotation * point.to_vec() + transf).to_point()
                    })
                    .collect(),
                color: color.into(),
            }));
    }

    pub fn filled_rect(&mut self, rect: Rect2S, color: impl Into<SceneColor>) {
        self.parent.nodes.push(SceneNode::FilledRect(FilledRect {
            rect: (self.current_rotation * rect).translate(self.current_translation),
            color: color.into(),
        }));
    }

    pub fn try_subscene<E>(
        &mut self,
        scene_filter: Option<SceneFilter>,
        active_color: impl Into<SceneColor>,
        content_fn: impl FnOnce(Point2S, Rotation) -> Result<SubsceneKind, E>,
    ) -> Result<(), E> {
        let floored_pos = Vec2::new(
            self.current_translation.x.floor_to_inner(),
            self.current_translation.y.floor_to_inner(),
        );

        // TODO: We should actually only pass rotation to the callback if
        //       scene_filter is None because the filter is supossed to
        //       operate on a bitmap conceptually.
        //       This doesn't matter for rotations though (with current filters).
        self.parent.nodes.push(SceneNode::Subscene(Subscene {
            pos: floored_pos.to_point(),
            scene_filter,
            kind: content_fn(
                (self.current_translation - floored_pos).to_point(),
                self.current_rotation,
            )?,
            active_color: active_color.into(),
        }));

        Ok(())
    }

    pub fn subscene(
        &mut self,
        scene_filter: Option<SceneFilter>,
        active_color: impl Into<SceneColor>,
        content_fn: impl FnOnce(Point2S, Rotation) -> SubsceneKind,
    ) {
        match self.try_subscene(scene_filter, active_color, |translation, rotation| {
            Ok::<_, Infallible>(content_fn(translation, rotation))
        }) {
            Ok(()) => (),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneColor(BGRA8);

impl SceneColor {
    pub const ACTIVE: Self = Self(BGRA8::new(1, 0, 0, 0));

    pub(crate) fn compute(self, active_color: BGRA8) -> BGRA8 {
        if self == Self::ACTIVE {
            active_color
        } else {
            self.0
        }
    }
}

impl From<BGRA8> for SceneColor {
    fn from(value: BGRA8) -> Self {
        Self(if value.a == 0 { BGRA8::ZERO } else { value })
    }
}

#[derive(Clone)]
pub(crate) struct Bitmap {
    pub(crate) pos: Point2<i32>,
    pub(crate) scaled_size: Vec2<u32>,
    pub(crate) texture: Texture,
    pub(crate) filter: Option<BitmapFilter>,
    pub(crate) rotation: Rotation,
    pub(crate) color: SceneColor,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum BitmapFilter {
    ExtractAlpha,
}

// TODO: Get rid of this
//       It doesn't respect rotation transform and is only used for debug stuff.
#[derive(Debug, Clone)]
pub(crate) struct StrokedPolyline {
    pub(crate) pos: Point2S,
    pub(crate) polyline: Vec<Point2<I16Dot16>>,
    pub(crate) width: I16Dot16,
    pub(crate) color: SceneColor,
}

#[derive(Debug, Clone)]
pub(crate) struct FilledOutline {
    pub(crate) events: Rc<[OutlineEvent<f32>]>,
    pub(crate) color: SceneColor,
}

#[derive(Clone, Copy)]
pub(crate) struct FilledRect {
    pub(crate) rect: Rect2S,
    pub(crate) color: SceneColor,
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
    pub(crate) active_color: SceneColor,
}

#[derive(Clone)]
pub enum SubsceneKind {
    External(Rc<dyn ExternalSubscene>),
    Scene(Scene),
}

pub trait ExternalSubscene {
    fn write_debug_name(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        _ = fmt;
        Ok(())
    }

    fn bounding_box(&self) -> Rect2S;
    fn rasterize(&self, rasterizer: &mut dyn Rasterizer) -> Result<(Vec2<i32>, Texture), AnyError>;
}

impl Subscene {
    pub(crate) fn bounding_box(&self) -> Rect2S {
        let inner_bbox = match &self.kind {
            SubsceneKind::External(external) => external.bounding_box(),
            SubsceneKind::Scene(scene) => scene.bounding_box(),
        };

        if inner_bbox == Rect2S::MAX || inner_bbox == Rect2S::NOTHING {
            return inner_bbox;
        }

        match self.scene_filter {
            Some(SceneFilter::ExtractAlpha { blur_stddev }) => {
                let mut result = inner_bbox;
                let expansion = blur_stddev * 3;
                result.expand(expansion, expansion);
                result
            }
            None => inner_bbox,
        }
        .translate(self.pos.to_vec())
    }
}

impl StrokedPolyline {
    // TODO: Get rid of this. The rounding here is probably wrong anyway...
    //       Part of "finally figure out a sane outline repr" goal.
    pub fn to_strips(
        &self,
        strip_rasterizer: &mut sw::StripRasterizer,
    ) -> (Point2<i32>, Vec2<u32>, Strips) {
        let mut bbox = Rect2::bounding_box_of_points(self.polyline.iter().copied());
        bbox.expand(self.width, self.width);

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
    pub fn to_strips(
        &self,
        strip_rasterizer: &mut sw::StripRasterizer,
    ) -> (Point2<i32>, Vec2<u32>, Strips) {
        let bbox = self.events.control_box();

        let output_pos = Point2::new(bbox.min.x.floor() as i32, bbox.min.y.floor() as i32);
        let input_shift = Vec2::new(output_pos.x as f32, output_pos.y as f32);
        let output_size = Vec2::new(
            (bbox.max.x.ceil() as i32 - output_pos.x) as u32,
            (bbox.max.y.ceil() as i32 - output_pos.y) as u32,
        );

        strip_rasterizer.add_outline(self.events.iter().copied().map_points(|mut p| {
            p -= input_shift;
            Point2::new(p.x, p.y)
        }));

        (output_pos, output_size, strip_rasterizer.rasterize())
    }
}
