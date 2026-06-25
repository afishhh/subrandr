use std::rc::Rc;

use rasterize::scene::SceneContentBuilder;
use util::math::{I16Dot16, Vec2};

use super::{
    inline::BoxBaselineSet,
    {Axes, FixedL, FragmentBox, LayoutConstraint, LayoutContext, Vec2L},
};
use crate::{
    layout::EdgeExtents,
    style::{
        computed::{ToPhysicalPixels, WritingMode},
        ComputedStyle,
    },
};

// For now all our images have known natural dimensions
#[derive(Debug, Clone, Copy)]
pub struct NaturalDimensions {
    pub width: FixedL,
    pub height: FixedL,
}

impl NaturalDimensions {
    fn aspect_ratio(&self) -> I16Dot16 {
        I16Dot16::from_raw(
            (((self.width.into_raw() as i64) << 22) / ((self.height.into_raw() as i64) << 6))
                as i32,
        )
    }
}

pub trait ImageInner: std::fmt::Debug {
    fn display(&self, builder: &mut SceneContentBuilder, size: Vec2L);
}

#[derive(Debug, Clone)]
pub struct Image {
    pub style: ComputedStyle,
    pub natural_dimensions: NaturalDimensions,
    pub inner: Rc<dyn ImageInner>,
}

impl Image {
    fn fill_in_size(&self, size: Vec2<Option<FixedL>>) -> Vec2L {
        match size {
            Vec2 { x: None, y: None } => Vec2L::new(
                self.natural_dimensions.width,
                self.natural_dimensions.height,
            ),
            Vec2 {
                x: None,
                y: Some(height),
            } => Vec2L::new(height * self.natural_dimensions.aspect_ratio(), height),
            Vec2 {
                x: Some(width),
                y: None,
            } => Vec2L::new(width, width / self.natural_dimensions.aspect_ratio()),
            Vec2 {
                x: Some(width),
                y: Some(height),
            } => Vec2L::new(width, height),
        }
    }

    pub(super) fn measure_inner(
        &self,
        lctx: &mut LayoutContext,
        _constraints: Vec2<LayoutConstraint>,
        _axes: Axes,
    ) -> Vec2L {
        self.fill_in_size(Vec2::new(
            self.style.width().to_physical_pixels(lctx.dpi),
            self.style.height().to_physical_pixels(lctx.dpi),
        ))
    }

    pub(super) fn layout(
        &self,
        lctx: &mut LayoutContext,
        size: Vec2<Option<FixedL>>,
        margins: EdgeExtents,
    ) -> ImageFragment {
        let padding = EdgeExtents::padding(&self.style, lctx.dpi);
        let inner_size = Vec2::new(
            size.x
                .map(|width| width - padding.left - padding.right - margins.left - margins.right),
            size.y
                .map(|height| height - padding.top - padding.bottom - margins.top - margins.bottom),
        );
        ImageFragment {
            fbox: FragmentBox {
                content_size: self.fill_in_size(inner_size),
                padding,
                margin: margins,
            },
            style: self.style.clone(),
            inner: self.inner.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ImageFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub inner: Rc<dyn ImageInner>,
}

impl ImageFragment {
    // TODO: is this correct?
    pub(super) fn baselines(&self, _outer_writing_mode: WritingMode) -> Option<BoxBaselineSet> {
        None
    }
}
