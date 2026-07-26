use std::fmt::Debug;

use log::{AsLogger, LogContext};
use util::math::{BoolExt, I26Dot6, Number, Point2, Rect2, Vec2};

use crate::{
    style::{
        computed::{Direction, Length, ToPhysicalPixels, WritingMode},
        ComputedStyle,
    },
    text::FontDb,
};

pub type FixedL = I26Dot6;
pub type Point2L = Point2<FixedL>;
pub type Vec2L = Vec2<FixedL>;
pub type Rect2L = Rect2<FixedL>;

#[derive(Debug, Clone, Copy)]
pub struct EdgeExtents {
    pub top: FixedL,
    pub bottom: FixedL,
    pub left: FixedL,
    pub right: FixedL,
}

impl EdgeExtents {
    const ZERO: Self = Self {
        top: FixedL::ZERO,
        bottom: FixedL::ZERO,
        left: FixedL::ZERO,
        right: FixedL::ZERO,
    };

    fn compute_fragmented(
        part: BoxFragmentationPart,
        top: impl FnOnce() -> FixedL,
        bottom: impl FnOnce() -> FixedL,
        left: impl FnOnce() -> FixedL,
        right: impl FnOnce() -> FixedL,
    ) -> Self {
        Self {
            top: part.is_top().then_or_zero(top),
            bottom: part.is_bottom().then_or_zero(bottom),
            left: part.is_leftmost().then_or_zero(left),
            right: part.is_rightmost().then_or_zero(right),
        }
    }

    fn padding_fragmented(part: BoxFragmentationPart, style: &ComputedStyle, dpi: u32) -> Self {
        Self::compute_fragmented(
            part,
            || style.padding_top().to_physical_pixels(dpi),
            || style.padding_bottom().to_physical_pixels(dpi),
            || style.padding_left().to_physical_pixels(dpi),
            || style.padding_right().to_physical_pixels(dpi),
        )
    }

    fn padding(style: &ComputedStyle, dpi: u32) -> Self {
        Self::padding_fragmented(BoxFragmentationPart::FULL, style, dpi)
    }

    fn margins_auto_to_zero_fragmented(
        part: BoxFragmentationPart,
        style: &ComputedStyle,
        dpi: u32,
    ) -> Self {
        Self::compute_fragmented(
            part,
            || {
                style
                    .margin_top()
                    .to_physical_pixels(dpi)
                    .unwrap_or(FixedL::ZERO)
            },
            || {
                style
                    .margin_bottom()
                    .to_physical_pixels(dpi)
                    .unwrap_or(FixedL::ZERO)
            },
            || {
                style
                    .margin_left()
                    .to_physical_pixels(dpi)
                    .unwrap_or(FixedL::ZERO)
            },
            || {
                style
                    .margin_right()
                    .to_physical_pixels(dpi)
                    .unwrap_or(FixedL::ZERO)
            },
        )
    }

    fn margins_auto_to_zero(style: &ComputedStyle, dpi: u32) -> Self {
        Self::margins_auto_to_zero_fragmented(BoxFragmentationPart::FULL, style, dpi)
    }
}

#[derive(Clone, Copy)]
struct BoxFragmentationPart(u8);

impl BoxFragmentationPart {
    const HORIZONTAL_FIRST: Self = Self(0b01);
    const HORIZONTAL_LAST: Self = Self(0b10);
    const HORIZONTAL_FULL: Self = Self(0b11);

    const VERTICAL_FIRST: Self = Self(0b01 << 2);
    const VERTICAL_LAST: Self = Self(0b10 << 2);
    const VERTICAL_FULL: Self = Self(0b11 << 2);

    const FULL: Self = Self(Self::HORIZONTAL_FULL.0 | Self::VERTICAL_FULL.0);

    fn is_top(self) -> bool {
        self.0 & Self::VERTICAL_FIRST.0 != 0
    }

    fn is_bottom(self) -> bool {
        self.0 & Self::VERTICAL_LAST.0 != 0
    }

    fn is_leftmost(self) -> bool {
        self.0 & Self::HORIZONTAL_FIRST.0 != 0
    }

    fn is_rightmost(self) -> bool {
        self.0 & Self::HORIZONTAL_LAST.0 != 0
    }

    fn swap_axes(self) -> Self {
        let horiz = self.0 & Self::HORIZONTAL_FULL.0;
        let vert = self.0 & Self::VERTICAL_FULL.0;
        Self((vert >> 2) | (horiz << 2))
    }
}

impl std::ops::BitOr for BoxFragmentationPart {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for BoxFragmentationPart {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for BoxFragmentationPart {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl Debug for BoxFragmentationPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BoxFragmentationPart(")?;
        let mut first = true;
        for (bit, name) in [
            (Self::VERTICAL_FIRST, "VERTICAL_FIRST"),
            (Self::VERTICAL_LAST, "VERTICAL_LAST"),
            (Self::HORIZONTAL_FIRST, "HORIZONTAL_FIRST"),
            (Self::HORIZONTAL_LAST, "HORIZONTAL_LAST"),
        ] {
            if self.0 & bit.0 != 0 {
                if !first {
                    write!(f, " | ")?;
                } else {
                    first = false;
                }
                write!(f, "{name}")?
            }
        }
        write!(f, ")")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FragmentBox {
    pub content_size: Vec2L,
    pub padding: EdgeExtents,
    pub margin: EdgeExtents,
}

impl FragmentBox {
    pub const ZERO: Self = Self {
        content_size: Vec2L::ZERO,
        padding: EdgeExtents::ZERO,
        margin: EdgeExtents::ZERO,
    };

    const fn new_content_only(content_size: Vec2L) -> Self {
        Self {
            content_size,
            padding: EdgeExtents::ZERO,
            margin: EdgeExtents::ZERO,
        }
    }

    pub fn content_offset(&self) -> Vec2L {
        Vec2L::new(
            self.padding.left + self.margin.left,
            self.padding.top + self.margin.top,
        )
    }

    pub fn padding_box(&self) -> Rect2L {
        Rect2L::from_min_size(
            Point2L::new(self.margin.left, self.margin.top),
            self.content_size
                + Vec2L::new(
                    self.padding.left + self.padding.right,
                    self.padding.top + self.padding.bottom,
                ),
        )
    }

    pub fn border_box(&self) -> Rect2L {
        self.padding_box()
    }

    pub fn margin_box(&self) -> Rect2L {
        let mut result = self.padding_box();
        result.min.x -= self.margin.left;
        result.min.y -= self.margin.top;
        result.max.x += self.margin.right;
        result.max.y += self.margin.bottom;
        result
    }

    pub fn size_for_layout(&self) -> Vec2L {
        self.margin_box().max.to_vec()
    }
}

impl WritingMode {
    #[inline]
    pub(crate) fn is_horizontal(self) -> bool {
        matches!(self, WritingMode::HorizontalTtb)
    }

    #[inline]
    pub(crate) fn is_vertical(self) -> bool {
        !self.is_horizontal()
    }

    fn parallel(self, other: WritingMode) -> bool {
        matches!(
            (self, other),
            (WritingMode::HorizontalTtb, WritingMode::HorizontalTtb)
                | (
                    WritingMode::VerticalRtl | WritingMode::VerticalLtr | WritingMode::SidewaysRtl,
                    WritingMode::VerticalRtl | WritingMode::VerticalLtr | WritingMode::SidewaysRtl,
                )
        )
    }

    fn perpendicular(self, other: WritingMode) -> bool {
        !self.parallel(other)
    }
}

pub(crate) trait Vec2WritingModeExt<T> {
    fn inline(self, writing_mode: WritingMode) -> T;
    fn inline_mut(&mut self, writing_mode: WritingMode) -> &mut T;
    fn block(self, writing_mode: WritingMode) -> T;
    fn block_mut(&mut self, writing_mode: WritingMode) -> &mut T;
}

macro_rules! impl_writing_mode_ext {
    ($for: ident) => {
        impl<T> Vec2WritingModeExt<T> for $for<T> {
            #[inline]
            fn inline(self, writing_mode: WritingMode) -> T {
                if writing_mode.is_horizontal() {
                    self.x
                } else {
                    self.y
                }
            }

            #[inline]
            fn inline_mut(&mut self, writing_mode: WritingMode) -> &mut T {
                if writing_mode.is_horizontal() {
                    &mut self.x
                } else {
                    &mut self.y
                }
            }

            #[inline]
            fn block(self, writing_mode: WritingMode) -> T {
                if writing_mode.is_horizontal() {
                    self.y
                } else {
                    self.x
                }
            }

            #[inline]
            fn block_mut(&mut self, writing_mode: WritingMode) -> &mut T {
                if writing_mode.is_horizontal() {
                    &mut self.y
                } else {
                    &mut self.x
                }
            }
        }
    };
}

impl_writing_mode_ext!(Vec2);
impl_writing_mode_ext!(Point2);

impl FragmentBox {
    pub(crate) fn inline_size(&self, writing_mode: WritingMode) -> FixedL {
        self.margin_box().max.inline(writing_mode)
    }

    fn block_size(&self, writing_mode: WritingMode) -> FixedL {
        self.margin_box().max.block(writing_mode)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Vec2W<T> {
    pub block: T,
    pub inline: T,
}

pub(crate) type Vec2LW = Vec2W<FixedL>;

impl<T> Vec2W<T> {
    pub(crate) const fn new(block: T, inline: T) -> Self {
        Self { block, inline }
    }

    pub(crate) fn from_physical(physical: Vec2<T>, writing_mode: WritingMode) -> Self {
        if writing_mode.is_horizontal() {
            Self::new(physical.y, physical.x)
        } else {
            Self::new(physical.x, physical.y)
        }
    }
}

impl<T: Number> Vec2W<T> {
    pub(crate) const ZERO: Self = Self::new(T::ZERO, T::ZERO);
}

impl<T: Copy> Vec2W<T> {
    pub(crate) fn to_physical(self, writing_mode: WritingMode) -> Vec2<T> {
        match writing_mode {
            WritingMode::HorizontalTtb => Vec2::new(self.inline, self.block),
            WritingMode::VerticalRtl | WritingMode::SidewaysRtl | WritingMode::VerticalLtr => {
                Vec2::new(self.block, self.inline)
            }
        }
    }
}

impl ComputedStyle {
    pub(crate) fn inline_size(&self, writing_mode: WritingMode) -> Option<Length> {
        if writing_mode.is_horizontal() {
            self.width()
        } else {
            self.height()
        }
    }

    pub(crate) fn block_size(&self, writing_mode: WritingMode) -> Option<Length> {
        if writing_mode.is_horizontal() {
            self.height()
        } else {
            self.width()
        }
    }

    pub(crate) fn inline_min_padding(&self, writing_mode: WritingMode) -> Length {
        if writing_mode.is_vertical() {
            self.padding_top()
        } else {
            self.padding_left()
        }
    }

    pub(crate) fn inline_max_padding(&self, writing_mode: WritingMode) -> Length {
        if writing_mode.is_vertical() {
            self.padding_bottom()
        } else {
            self.padding_right()
        }
    }

    pub(crate) fn inline_min_margin(&self, writing_mode: WritingMode) -> Option<Length> {
        if writing_mode.is_vertical() {
            self.margin_top()
        } else {
            self.margin_left()
        }
    }

    pub(crate) fn inline_max_margin(&self, writing_mode: WritingMode) -> Option<Length> {
        if writing_mode.is_vertical() {
            self.margin_bottom()
        } else {
            self.margin_right()
        }
    }

    pub(crate) fn block_min_padding(&self, writing_mode: WritingMode) -> Length {
        if writing_mode.is_vertical() {
            self.padding_left()
        } else {
            self.padding_top()
        }
    }

    pub(crate) fn block_max_padding(&self, writing_mode: WritingMode) -> Length {
        if writing_mode.is_vertical() {
            self.padding_right()
        } else {
            self.padding_bottom()
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    pub(crate) fn inline(writing_mode: WritingMode) -> Self {
        if writing_mode.is_vertical() {
            Self::Y
        } else {
            Self::X
        }
    }

    pub(crate) fn block(writing_mode: WritingMode) -> Self {
        Self::inline(writing_mode).perpendicular()
    }

    pub(crate) fn perpendicular(self) -> Self {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Axes {
    pub x: bool,
    pub y: bool,
}

impl Axes {
    pub(crate) const NONE: Self = Self { x: false, y: false };

    fn inline(self, writing_mode: WritingMode) -> bool {
        Vec2::new(self.x, self.y).inline(writing_mode)
    }

    fn block(self, writing_mode: WritingMode) -> bool {
        Vec2::new(self.x, self.y).block(writing_mode)
    }

    fn block_mut(&mut self, writing_mode: WritingMode) -> &mut bool {
        Vec2::new(&mut self.x, &mut self.y).block(writing_mode)
    }
}

impl From<Axis> for Axes {
    fn from(value: Axis) -> Self {
        match value {
            Axis::X => Axes { x: true, y: false },
            Axis::Y => Axes { x: false, y: true },
        }
    }
}

impl std::ops::BitOr for Axes {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self::Output {
        self.x |= rhs.x;
        self.y |= rhs.y;
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutConstraint {
    Fixed(FixedL),
    MaxContent,
}

#[derive(Debug)]
pub struct LayoutContext<'l> {
    pub log: &'l LogContext<'l>,
    pub dpi: u32,
    pub fonts: &'l mut FontDb,
    pub initial_containing_block_size: Vec2L,
}

impl AsLogger for LayoutContext<'_> {
    fn as_logger(&self) -> &impl log::Logger {
        self.log.as_logger()
    }
}

pub mod inline;
pub use inline::InlineLayoutError;
pub mod block;
pub mod image;

#[derive(Debug)]
pub enum IndependentBox {
    Block(block::BlockContainer),
    Image(image::Image),
    User(UserContainer),
}

impl From<block::BlockContainer> for IndependentBox {
    fn from(value: block::BlockContainer) -> Self {
        Self::Block(value)
    }
}

impl From<image::Image> for IndependentBox {
    fn from(value: image::Image) -> Self {
        Self::Image(value)
    }
}

impl IndependentBox {
    pub fn layout_initial(
        &self,
        lctx: &mut LayoutContext,
    ) -> Result<PartialIndependentBox<'_>, InlineLayoutError> {
        match self {
            IndependentBox::Block(block) => {
                block.layout_initial(lctx).map(PartialIndependentBox::Block)
            }
            IndependentBox::Image(image) => Ok(PartialIndependentBox::Image(image.clone())),
            IndependentBox::User(container) => Ok(PartialIndependentBox::User(container.clone())),
        }
    }
}

pub enum PartialIndependentBox<'a> {
    Block(block::PartialBlockContainer<'a>),
    Image(image::Image),
    User(UserContainer),
}

impl<'a> PartialIndependentBox<'a> {
    pub fn style(&self) -> &ComputedStyle {
        match self {
            PartialIndependentBox::Block(block) => block.style(),
            PartialIndependentBox::Image(image) => &image.style,
            PartialIndependentBox::User(user) => &user.style,
        }
    }

    pub fn measure(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        axes: Axes,
    ) -> Result<Vec2L, InlineLayoutError> {
        let style = self.style();
        let writing_mode = style.writing_mode();

        let outer_edges = Vec2W::new(
            style
                .block_min_padding(writing_mode)
                .to_physical_pixels(lctx.dpi)
                + style
                    .block_max_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi),
            style
                .inline_min_margin(writing_mode)
                .to_physical_pixels(lctx.dpi)
                .unwrap_or(FixedL::ZERO)
                + style
                    .inline_min_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi)
                + style
                    .inline_max_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi)
                + style
                    .inline_max_margin(writing_mode)
                    .to_physical_pixels(lctx.dpi)
                    .unwrap_or(FixedL::ZERO),
        );

        let mut inner_constraints = constraints;
        match &mut inner_constraints.inline_mut(writing_mode) {
            LayoutConstraint::Fixed(fixed) => *fixed -= outer_edges.inline,
            LayoutConstraint::MaxContent => (),
        }
        match &mut inner_constraints.block_mut(writing_mode) {
            LayoutConstraint::Fixed(fixed) => *fixed -= outer_edges.block,
            LayoutConstraint::MaxContent => (),
        }
        let mut result = self.measure_inner(lctx, inner_constraints, axes)?;

        if axes.inline(writing_mode) {
            *result.inline_mut(writing_mode) += outer_edges.inline;
        }
        if axes.block(writing_mode) {
            *result.block_mut(writing_mode) += outer_edges.block;
        }

        Ok(result)
    }

    fn measure_inner(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        axes: Axes,
    ) -> Result<Vec2L, InlineLayoutError> {
        match self {
            PartialIndependentBox::Block(block) => block.measure_inner(lctx, constraints, axes),
            PartialIndependentBox::Image(image) => Ok(image.measure_inner(lctx, constraints, axes)),
            PartialIndependentBox::User(user) => Ok(user.size),
        }
    }

    fn layout(
        &self,
        lctx: &mut LayoutContext,
        outer_inner_inline_size: FixedL,
        margins: EdgeExtents,
        outer_available_block_space: Option<FixedL>,
        outer_writing_mode: WritingMode,
    ) -> Result<IndependentBoxFragment, InlineLayoutError> {
        match self {
            PartialIndependentBox::Block(block) => block
                .layout(
                    lctx,
                    block::FlowLayoutConstraints::Normal {
                        outer_inner_inline_size,
                        outer_available_block_space,
                        outer_writing_mode,
                    },
                    margins,
                )
                .map(IndependentBoxFragment::Block),
            PartialIndependentBox::Image(image) => Ok(IndependentBoxFragment::Image(image.layout(
                lctx,
                Vec2W::new(None, Some(outer_inner_inline_size)).to_physical(outer_writing_mode),
                margins,
            ))),
            PartialIndependentBox::User(user) => Ok(IndependentBoxFragment::Block(
                block::BlockContainerFragment {
                    fbox: FragmentBox {
                        content_size: user.size,
                        padding: EdgeExtents::padding(&user.style, lctx.dpi),
                        margin: margins,
                    },
                    style: user.style.clone(),
                    content: block::BlockContainerFragmentContent::Block(user.content.clone()),
                },
            )),
        }
    }

    fn layout_fixed(
        &self,
        lctx: &mut LayoutContext,
        outer_size: Vec2L,
        margins: EdgeExtents,
    ) -> Result<IndependentBoxFragment, InlineLayoutError> {
        let padding = EdgeExtents::padding(self.style(), lctx.dpi);
        let inner_size = Vec2::new(
            outer_size.x - padding.left - padding.right - margins.left - margins.right,
            outer_size.y - padding.top - padding.bottom - margins.top - margins.bottom,
        );

        match self {
            PartialIndependentBox::Block(block) => block
                .layout(
                    lctx,
                    block::FlowLayoutConstraints::Fixed { inner_size },
                    margins,
                )
                .map(IndependentBoxFragment::Block),
            PartialIndependentBox::Image(image) => Ok(IndependentBoxFragment::Image(image.layout(
                lctx,
                Vec2::new(Some(inner_size.x), Some(inner_size.y)),
                margins,
            ))),
            PartialIndependentBox::User(user) => Ok(IndependentBoxFragment::Block(
                block::BlockContainerFragment {
                    fbox: FragmentBox {
                        content_size: inner_size,
                        padding: EdgeExtents::padding(&user.style, lctx.dpi),
                        margin: margins,
                    },
                    style: user.style.clone(),
                    content: block::BlockContainerFragmentContent::Block(user.content.clone()),
                },
            )),
        }
    }

    pub fn layout_in(
        self,
        lctx: &mut LayoutContext,
        size: Vec2LW,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> Result<IndependentBoxFragment, InlineLayoutError> {
        let inline_sizes =
            self.block_level_inline_sizes(lctx, size.inline, writing_mode, direction)?;
        self.layout(
            lctx,
            inline_sizes.size,
            inline_sizes.margins(writing_mode),
            Some(size.block),
            writing_mode,
        )
    }
}

// TODO: don't clone this whole thing actually
#[derive(Debug, Clone)]
pub enum IndependentBoxFragment {
    Block(block::BlockContainerFragment),
    Image(image::ImageFragment),
}

impl IndependentBoxFragment {
    const EMPTY: Self = Self::Block(block::BlockContainerFragment::EMPTY);

    pub fn fbox(&self) -> &FragmentBox {
        match self {
            IndependentBoxFragment::Block(block) => &block.fbox,
            IndependentBoxFragment::Image(image) => &image.fbox,
        }
    }

    fn baselines(&self, outer_writing_mode: WritingMode) -> Option<inline::BoxBaselineSet> {
        match self {
            IndependentBoxFragment::Block(block) => block.baselines(outer_writing_mode),
            IndependentBoxFragment::Image(image) => image.baselines(outer_writing_mode),
        }
    }
}

impl IndependentBox {
    pub fn layout(
        &self,
        lctx: &mut LayoutContext,
        initial_containing_block_size: Vec2L,
    ) -> Result<IndependentBoxFragment, InlineLayoutError> {
        let partial = self.layout_initial(lctx)?;
        let writing_mode = partial.style().writing_mode();
        let direction = partial.style().direction();
        lctx.initial_containing_block_size = initial_containing_block_size;

        partial.layout_in(
            lctx,
            Vec2W::from_physical(initial_containing_block_size, writing_mode),
            writing_mode,
            direction,
        )
    }
}

#[derive(Debug, Clone)]
pub struct UserContainer {
    style: ComputedStyle,
    size: Vec2L,
    content: Vec<(Vec2L, IndependentBoxFragment)>,
}

#[derive(Debug, Clone)]
pub struct UserContainerBuilder {
    style: ComputedStyle,
    content: Vec<(Vec2L, IndependentBoxFragment)>,
}

impl UserContainerBuilder {
    pub fn new(style: ComputedStyle) -> Self {
        Self {
            style,
            content: Vec::new(),
        }
    }

    pub fn place(
        &mut self,
        lctx: &mut LayoutContext<'_>,
        offset: Vec2L,
        child: PartialIndependentBox,
        size: Vec2L,
    ) -> Result<(), InlineLayoutError> {
        let fragment = child.layout_fixed(
            lctx,
            size,
            EdgeExtents::margins_auto_to_zero(child.style(), lctx.dpi),
        )?;

        self.content.push((offset, fragment));

        Ok(())
    }

    pub fn finish(&mut self, size: Vec2L) -> UserContainer {
        UserContainer {
            style: self.style.clone(),
            size,
            content: std::mem::take(&mut self.content),
        }
    }
}
