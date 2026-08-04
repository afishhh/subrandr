use log::error;
use util::math::Vec2;

use super::{
    inline::{BoxBaselineSet, InlineContent, InlineContentFragment, PartialInline},
    Axes, Axis, EdgeExtents, FixedL, FragmentBox, InlineLayoutError, LayoutConstraint,
    LayoutContext, Vec2L, Vec2LW, Vec2W, Vec2WritingModeExt,
};
use crate::style::{
    computed::{BaselineSource, Direction, HorizontalAlignment, ToPhysicalPixels, WritingMode},
    ComputedStyle,
};

#[derive(Debug, Clone)]
pub struct BlockContainer {
    pub style: ComputedStyle,
    pub content: BlockContainerContent,
}

#[derive(Debug, Clone)]
pub enum BlockContainerContent {
    Inline(InlineContent),
    Block(Vec<BlockContainer>),
}

#[derive(Debug)]
pub struct BlockContainerFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub content: BlockContainerFragmentContent,
}

impl BlockContainerFragment {
    pub(super) const EMPTY: Self = Self {
        fbox: FragmentBox::ZERO,
        style: ComputedStyle::DEFAULT,
        content: BlockContainerFragmentContent::Block(Vec::new()),
    };

    pub(crate) fn from_inline(inline: InlineContentFragment) -> Self {
        Self {
            fbox: FragmentBox::new_content_only(inline.fbox.content_size),
            style: ComputedStyle::DEFAULT,
            content: BlockContainerFragmentContent::Inline(Vec2L::ZERO, inline),
        }
    }

    pub(super) fn baselines_from(
        &self,
        source: BaselineSource,
        outer_writing_mode: WritingMode,
    ) -> Option<BoxBaselineSet> {
        // Here so this code blows up if `BaselineSource` is ever extended.
        match source {
            BaselineSource::Last => (),
        }

        let writing_mode = self.style.writing_mode();
        if outer_writing_mode.perpendicular(writing_mode) {
            return None;
        }

        let content_block_off = self.fbox.content_offset().block(writing_mode);
        match &self.content {
            BlockContainerFragmentContent::Inline(off, inline_content_fragment) => {
                let (line_off, line) = inline_content_fragment.lines.last()?;
                Some(inline_content_fragment.line_baselines.offset(
                    content_block_off
                        + off.block(writing_mode)
                        + line_off.block(writing_mode)
                        + line.dominant_baseline_offset,
                ))
            }
            BlockContainerFragmentContent::Block(children) => {
                children.iter().rev().find_map(|&(child_off, ref child)| {
                    child.baselines(outer_writing_mode).map(|child_baseline| {
                        child_baseline.offset(content_block_off + child_off.block(writing_mode))
                    })
                })
            }
        }
    }

    pub(super) fn baselines(&self, outer_writing_mode: WritingMode) -> Option<BoxBaselineSet> {
        self.baselines_from(self.style.baseline_source(), outer_writing_mode)
    }
}

#[derive(Debug)]
pub enum BlockContainerFragmentContent {
    Inline(Vec2L, InlineContentFragment),
    Block(Vec<(Vec2L, BlockContainerFragment)>),
}

pub struct PartialBlockContainer<'a> {
    style: ComputedStyle,
    content: PartialBlockContainerContent<'a>,
}

#[allow(clippy::large_enum_variant)] // shouldn't be moved around much anyway
enum PartialBlockContainerContent<'a> {
    Inline(PartialInline<'a>),
    Block(Vec<PartialBlockContainer<'a>>),
}

#[derive(Debug)]
pub(super) struct BlockInlineSizes {
    pub(super) margin_min: FixedL,
    pub(super) size: FixedL,
    pub(super) margin_max: FixedL,
}

struct BlockComputedInlineSizes {
    margin_min: Option<FixedL>,
    padding_min: FixedL,
    size: Option<FixedL>,
    padding_max: FixedL,
    margin_max: Option<FixedL>,
}

impl BlockComputedInlineSizes {
    fn new(style: &ComputedStyle, writing_mode: WritingMode, dpi: u32) -> Self {
        Self {
            margin_min: style
                .inline_min_margin(writing_mode)
                .to_physical_pixels(dpi),
            padding_min: style
                .inline_min_padding(writing_mode)
                .to_physical_pixels(dpi),
            size: style.inline_size(writing_mode).to_physical_pixels(dpi),
            padding_max: style
                .inline_max_padding(writing_mode)
                .to_physical_pixels(dpi),
            margin_max: style
                .inline_max_margin(writing_mode)
                .to_physical_pixels(dpi),
        }
    }
}

impl BlockInlineSizes {
    // https://www.w3.org/TR/CSS2/visudet.html#blockwidth
    fn compute_for_nonreplaced_block(
        BlockComputedInlineSizes {
            margin_min: mut margin_left,
            padding_min: padding_left,
            size: width,
            padding_max: padding_right,
            margin_max: mut margin_right,
        }: BlockComputedInlineSizes,
        containing_block_width: FixedL,
        containing_block_direction: Direction,
    ) -> Self {
        // If 'width' is not 'auto' and 'border-left-width' + 'padding-left' + 'width' + 'padding-right' + 'border-right-width' (plus any of 'margin-left' or 'margin-right' that are not 'auto') is larger than the width of the containing block,
        if let Some(width) = width {
            if margin_left.unwrap_or(FixedL::ZERO)
                + padding_left
                + width
                + padding_right
                + margin_right.unwrap_or(FixedL::ZERO)
                > containing_block_width
            {
                // then any 'auto' values for 'margin-left' or 'margin-right' are, for the following rules, treated as zero.
                if margin_left.is_none() {
                    margin_left = Some(FixedL::ZERO);
                }
                if margin_right.is_none() {
                    margin_right = Some(FixedL::ZERO);
                }
            }
        }

        let base_width = padding_left + padding_right;
        let (used_margin_left, used_width, used_margin_right);
        match (margin_left, width, margin_right) {
            // If all of the above have a computed value other than 'auto', the values are said to be "over-constrained" and one of the used values will have to be different from its computed value.
            (Some(margin_left), Some(width), Some(margin_right)) => {
                used_width = width;
                match containing_block_direction {
                    // If the 'direction' property of the containing block has the value 'ltr',
                    Direction::Ltr => {
                        used_margin_left = margin_left;
                        // the specified value of 'margin-right' is ignored and the value is calculated so as to make the equality true.
                        used_margin_right =
                            containing_block_width - (base_width + width + margin_left);
                    }
                    // If the value of 'direction' is 'rtl',
                    Direction::Rtl => {
                        used_margin_right = margin_right;
                        // this happens to 'margin-left' instead.
                        used_margin_left =
                            containing_block_width - (base_width + width + margin_right);
                    }
                }
            }
            // If there is exactly one value specified as 'auto', its used value follows from the equality.
            (Some(margin_left), Some(width), None) => {
                used_margin_left = margin_left;
                used_width = width;
                used_margin_right = containing_block_width - (base_width + width + margin_left);
            }
            (None, Some(width), Some(margin_right)) => {
                used_margin_right = margin_right;
                used_width = width;
                used_margin_left = containing_block_width - (base_width + width + margin_right);
            }
            // If 'width' is set to 'auto',
            (_, None, _) => {
                // any other 'auto' values become '0' and 'width' follows from the resulting equality.
                used_margin_left = margin_left.unwrap_or(FixedL::ZERO);
                used_margin_right = margin_right.unwrap_or(FixedL::ZERO);
                used_width =
                    containing_block_width - (base_width + used_margin_left + used_margin_right);
            }
            // If both 'margin-left' and 'margin-right' are 'auto',
            (None, Some(width), None) => {
                used_width = width;
                // their used values are equal.
                let margin = containing_block_width - (base_width + width);
                used_margin_left = margin / 2;
                used_margin_right = margin - used_margin_left;
            }
        }

        BlockInlineSizes {
            margin_min: used_margin_left,
            size: used_width,
            margin_max: used_margin_right,
        }
    }

    // https://www.w3.org/TR/CSS2/visudet.html#shrink-to-fit-float
    fn floating_shrink_to_fit_width(
        lctx: &mut LayoutContext,
        container: &PartialBlockContainer,
        margin_left: FixedL,
        padding_left: FixedL,
        padding_right: FixedL,
        margin_right: FixedL,
        constraints: Vec2<LayoutConstraint>,
        outer_writing_mode: WritingMode,
    ) -> Result<FixedL, InlineLayoutError> {
        // calculate the preferred width by formatting the content without breaking lines other than where explicit line breaks occur
        let mut preferred_measurement_constraints = Vec2W::new(
            constraints.block(outer_writing_mode),
            LayoutConstraint::MaxContent,
        )
        .to_physical(outer_writing_mode);
        if outer_writing_mode.perpendicular(container.style.writing_mode()) {
            // https://drafts.csswg.org/css-writing-modes-3/#orthogonal-layout
            // ^ absolutely brilliant sentence btw
            preferred_measurement_constraints = container.child_measure_constraints(
                lctx,
                preferred_measurement_constraints,
                outer_writing_mode,
            )
        };
        let preferred_width = container
            .measure_inner(
                lctx,
                preferred_measurement_constraints,
                Axes::from(Axis::inline(outer_writing_mode)),
            )?
            .inline(outer_writing_mode);

        // TODO: minimum width
        // Thirdly, find the available width: in this case, this is the width of the containing block minus the used values of 'margin-left', 'border-left-width', 'padding-left', 'padding-right', 'border-right-width', 'margin-right', and the widths of any relevant scroll bars.
        let available_width = match constraints.inline(outer_writing_mode) {
            LayoutConstraint::Fixed(available_inline_space) => {
                available_inline_space - margin_left - padding_left - padding_right - margin_right
            }
            LayoutConstraint::MaxContent => FixedL::MAX,
        };

        // Then the shrink-to-fit width is: min(max(preferred minimum width, available width), preferred width).
        Ok(std::cmp::min(available_width, preferred_width))
    }

    // https://www.w3.org/TR/CSS2/visudet.html#inlineblock-width
    fn compute_for_nonreplaced_inline(
        lctx: &mut LayoutContext,
        container: &PartialBlockContainer,
        BlockComputedInlineSizes {
            margin_min: margin_left,
            padding_min: padding_left,
            size: width,
            padding_max: padding_right,
            margin_max: margin_right,
        }: BlockComputedInlineSizes,
        constraints: Vec2<LayoutConstraint>,
        outer_writing_mode: WritingMode,
    ) -> Result<Self, InlineLayoutError> {
        // A computed value of 'auto' for 'margin-left' or 'margin-right' becomes a used value of '0'.
        let margin_left = margin_left.unwrap_or(FixedL::ZERO);
        let margin_right = margin_right.unwrap_or(FixedL::ZERO);

        // If 'width' is 'auto', the used value is the shrink-to-fit width as for floating elements.
        let width = match width {
            Some(width) => width,
            None => Self::floating_shrink_to_fit_width(
                lctx,
                container,
                margin_left,
                padding_left,
                padding_right,
                margin_right,
                constraints,
                outer_writing_mode,
            )?,
        };

        Ok(Self {
            margin_min: margin_left,
            size: width,
            margin_max: margin_right,
        })
    }

    pub(super) fn margins(&self, writing_mode: WritingMode) -> EdgeExtents {
        if writing_mode.is_horizontal() {
            EdgeExtents {
                top: FixedL::ZERO,
                bottom: FixedL::ZERO,
                left: self.margin_min,
                right: self.margin_max,
            }
        } else {
            EdgeExtents {
                top: self.margin_min,
                bottom: self.margin_max,
                left: FixedL::ZERO,
                right: FixedL::ZERO,
            }
        }
    }
}

// https://drafts.csswg.org/css-writing-modes-3/#orthogonal-auto
pub(super) fn fallback_inline_space_in_orthogonal_flow(
    lctx: &mut LayoutContext,
    orthogonal_writing_mode: WritingMode,
) -> FixedL {
    // In these cases, an additional fallback size is used in place of the available inline space for calculations that require a definite available inline space: this size is the smallest of
    // - the size represented by the containing block’s inner max size (if that is fixed) floored by its inner min size (if that is fixed)
    // TODO: support max-{width,height}
    // - the nearest ancestor scrollport’s inner size if that is fixed, else / capped by its inner max size if that is fixed, floored by its inner min size if that is fixed
    // NOTE: scrollports do not exist in our layout engine right now
    // - the initial containing block’s size
    lctx.initial_containing_block_size
        .inline(orthogonal_writing_mode)
}

impl PartialBlockContainer<'_> {
    fn block_level_inline_sizes(
        &self,
        lctx: &mut LayoutContext,
        containing_block_width: FixedL,
        containing_block_writing_mode: WritingMode,
        containing_block_direction: Direction,
    ) -> Result<BlockInlineSizes, InlineLayoutError> {
        let computed =
            BlockComputedInlineSizes::new(&self.style, containing_block_writing_mode, lctx.dpi);
        Ok(BlockInlineSizes::compute_for_nonreplaced_block(
            computed,
            containing_block_width,
            containing_block_direction,
        ))
    }

    pub(super) fn inline_level_block_sizes(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        outer_writing_mode: WritingMode,
    ) -> Result<BlockInlineSizes, InlineLayoutError> {
        let computed = BlockComputedInlineSizes::new(&self.style, outer_writing_mode, lctx.dpi);
        let width = BlockInlineSizes::compute_for_nonreplaced_inline(
            lctx,
            self,
            computed,
            constraints,
            outer_writing_mode,
        )?;

        Ok(width)
    }

    pub fn measure(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        axes: Axes,
    ) -> Result<Vec2L, InlineLayoutError> {
        let writing_mode = self.style.writing_mode();

        let outer_edges = Vec2W::new(
            self.style
                .block_min_padding(writing_mode)
                .to_physical_pixels(lctx.dpi)
                + self
                    .style
                    .block_max_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi),
            self.style
                .inline_min_margin(writing_mode)
                .to_physical_pixels(lctx.dpi)
                .unwrap_or(FixedL::ZERO)
                + self
                    .style
                    .inline_min_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi)
                + self
                    .style
                    .inline_max_padding(writing_mode)
                    .to_physical_pixels(lctx.dpi)
                + self
                    .style
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

    fn child_measure_constraints(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        outer_writing_mode: WritingMode,
    ) -> Vec2<LayoutConstraint> {
        let child_writing_mode = self.style.writing_mode();
        if outer_writing_mode.parallel(child_writing_mode) {
            // parallel flows don't require special handling
            return constraints;
        }

        // orthogonal flows may need to use a fallback size
        let available_inline_space = match constraints.inline(child_writing_mode) {
            LayoutConstraint::Fixed(fixed) => fixed,
            LayoutConstraint::MaxContent => {
                fallback_inline_space_in_orthogonal_flow(lctx, child_writing_mode)
            }
        };
        Vec2W::new(
            constraints.block(child_writing_mode),
            LayoutConstraint::Fixed(available_inline_space),
        )
        .to_physical(child_writing_mode)
    }

    fn measure_inner(
        &self,
        lctx: &mut LayoutContext,
        constraints: Vec2<LayoutConstraint>,
        mut axes: Axes,
    ) -> Result<Vec2L, InlineLayoutError> {
        let mut fixed = Vec2L::ZERO;
        if let Some(width) = self.style.width().to_physical_pixels(lctx.dpi) {
            fixed.x = width;
            axes.x = false;
        }
        if let Some(height) = self.style.height().to_physical_pixels(lctx.dpi) {
            fixed.y = height;
            axes.y = false;
        }

        if axes == Axes::NONE {
            return Ok(fixed);
        }

        let writing_mode = self.style.writing_mode();
        let auto_axes = axes;
        // If we have a fixed available block size then we need to track child block sizes
        // to update it.
        if matches!(constraints.block(writing_mode), LayoutConstraint::Fixed(_)) {
            *axes.block_mut(writing_mode) = true;
        }

        let auto = match &self.content {
            PartialBlockContainerContent::Inline(inline) => {
                inline.measure(lctx, constraints, axes)?
            }
            PartialBlockContainerContent::Block(children) => {
                let mut current_constraints = constraints;
                let mut result = Vec2LW::ZERO;

                for child in children {
                    let child_constraints =
                        child.child_measure_constraints(lctx, current_constraints, writing_mode);
                    let child_size = child.measure(lctx, child_constraints, axes)?;

                    result.inline = result.inline.max(child_size.inline(writing_mode));
                    result.block += child_size.block(writing_mode);
                    match current_constraints.block_mut(writing_mode) {
                        LayoutConstraint::Fixed(fixed) => *fixed -= child_size.block(writing_mode),
                        LayoutConstraint::MaxContent => (),
                    }
                }

                result.to_physical(writing_mode)
            }
        };

        let mut result = fixed;
        if auto_axes.x {
            result.x = auto.x;
        }
        if auto_axes.y {
            result.y = auto.y;
        }
        Ok(result)
    }

    pub(super) fn layout(
        &self,
        lctx: &mut LayoutContext,
        // Refers to the inner inline size in the parent's (outer) writing mode.
        outer_inner_inline_size: FixedL,
        margins: EdgeExtents,
        outer_available_block_space: Option<FixedL>,
        outer_writing_mode: WritingMode,
    ) -> Result<BlockContainerFragment, InlineLayoutError> {
        let writing_mode = self.style.writing_mode();
        let mut base_inner_size = Vec2W::new(None, None);

        if let Some(explicit_inline_size) = self.style.inline_size(writing_mode) {
            base_inner_size.inline = Some(explicit_inline_size.to_physical_pixels(lctx.dpi));
        }
        if let Some(explicit_block_size) = self.style.block_size(writing_mode) {
            base_inner_size.block = Some(explicit_block_size.to_physical_pixels(lctx.dpi));
        }

        if outer_writing_mode.perpendicular(writing_mode) {
            base_inner_size.block = Some(outer_inner_inline_size);
        } else {
            base_inner_size.inline = Some(outer_inner_inline_size);
        }

        let available_inline_space = base_inner_size.inline.unwrap_or_else(|| {
            assert!(outer_writing_mode.perpendicular(writing_mode));
            outer_available_block_space
                .unwrap_or_else(|| fallback_inline_space_in_orthogonal_flow(lctx, writing_mode))
        });
        let mut available_block_space = base_inner_size.block.or_else(|| {
            if outer_writing_mode.perpendicular(writing_mode) {
                Some(outer_inner_inline_size)
            } else {
                outer_available_block_space
            }
        });

        // https://drafts.csswg.org/css-writing-modes-3/#orthogonal-layout
        // If this block contains only inline children then this will be used for laying
        // them out and the inner inline size will be calculated from the resulting fragment.
        // Otherwise it will be passed to `self.measure_inner` to calculate the inner inline
        // size before laying out children.
        let inner_measure_constraints = Vec2W::new(
            available_block_space.map_or(LayoutConstraint::MaxContent, LayoutConstraint::Fixed),
            LayoutConstraint::Fixed(available_inline_space),
        )
        .to_physical(writing_mode);

        let inner_inline_size;
        let mut auto_block_size = FixedL::ZERO;
        let content = match &self.content {
            PartialBlockContainerContent::Inline(inline) => {
                let inner_writing_mode = inline.root_style().writing_mode();
                if writing_mode != inner_writing_mode {
                    error!(lctx, "Block has different writing mode ({writing_mode:?}) from anonymous root inline child ({inner_writing_mode:?}). This is wrong!");
                }

                let fragment = inline.layout(lctx, inner_measure_constraints)?;

                let content_inline_size = fragment.fbox.inline_size(writing_mode);
                inner_inline_size = base_inner_size
                    .inline
                    .unwrap_or(fragment.fbox.inline_size(writing_mode));
                auto_block_size = fragment.fbox.block_size(writing_mode);

                let inline_offset = match self.style.text_align() {
                    HorizontalAlignment::Left => FixedL::ZERO,
                    HorizontalAlignment::Center => (inner_inline_size - content_inline_size) / 2,
                    HorizontalAlignment::Right => inner_inline_size - content_inline_size,
                };
                BlockContainerFragmentContent::Inline(
                    Vec2LW::new(FixedL::ZERO, inline_offset).to_physical(writing_mode),
                    fragment,
                )
            }
            PartialBlockContainerContent::Block(children) => {
                inner_inline_size = base_inner_size.inline.unwrap_or(
                    self.measure_inner(
                        lctx,
                        inner_measure_constraints,
                        Axes::from(Axis::inline(writing_mode)),
                    )?
                    .inline(writing_mode),
                );

                let mut fragments = Vec::new();
                for child in children {
                    let child_inline_sizes = child.block_level_inline_sizes(
                        lctx,
                        inner_inline_size,
                        writing_mode,
                        self.style.direction(),
                    )?;
                    let child_margins = child_inline_sizes.margins(writing_mode);
                    let fragment = child.layout(
                        lctx,
                        child_inline_sizes.size,
                        child_margins,
                        available_block_space,
                        writing_mode,
                    )?;

                    let mut off = Vec2LW::new(auto_block_size, FixedL::ZERO);
                    auto_block_size += fragment.fbox.block_size(writing_mode);

                    if writing_mode.is_block_reversed() {
                        off.block = -auto_block_size;
                    }

                    if let Some(space) = available_block_space.as_mut() {
                        *space =
                            (*space - fragment.fbox.block_size(writing_mode)).max(FixedL::ZERO);
                    }

                    fragments.push((off.to_physical(writing_mode), fragment));
                }

                if writing_mode.is_block_reversed() {
                    for (off, _) in fragments.iter_mut() {
                        *off.block_mut(writing_mode) += auto_block_size;
                    }
                }

                BlockContainerFragmentContent::Block(fragments)
            }
        };

        let inner_size = Vec2W::new(
            base_inner_size.block.unwrap_or(auto_block_size),
            inner_inline_size,
        );
        Ok(BlockContainerFragment {
            style: self.style.clone(),
            fbox: FragmentBox {
                content_size: inner_size.to_physical(writing_mode),
                padding: EdgeExtents::padding(&self.style, lctx.dpi),
                margin: margins,
            },
            content,
        })
    }

    pub fn layout_in(
        self,
        lctx: &mut LayoutContext,
        size: Vec2LW,
        writing_mode: WritingMode,
        direction: Direction,
    ) -> Result<BlockContainerFragment, InlineLayoutError> {
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

pub fn layout_initial<'a>(
    lctx: &mut LayoutContext,
    container: &'a BlockContainer,
) -> Result<PartialBlockContainer<'a>, InlineLayoutError> {
    let content = match &container.content {
        BlockContainerContent::Inline(inline) => {
            PartialBlockContainerContent::Inline(super::inline::shape(lctx, inline)?)
        }
        BlockContainerContent::Block(children) => {
            let mut partials = Vec::new();
            for child in children {
                partials.push(layout_initial(lctx, child)?);
            }

            PartialBlockContainerContent::Block(partials)
        }
    };

    Ok(PartialBlockContainer {
        style: container.style.clone(),
        content,
    })
}

#[cfg_attr(not(all(test, feature = "_layout_tests")), expect(dead_code))]
pub fn layout(
    lctx: &mut LayoutContext,
    container: &BlockContainer,
    initial_containing_block_size: Vec2L,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    let writing_mode = container.style.writing_mode();
    lctx.initial_containing_block_size = initial_containing_block_size;

    layout_initial(lctx, container)?.layout_in(
        lctx,
        Vec2W::from_physical(initial_containing_block_size, writing_mode),
        writing_mode,
        container.style.direction(),
    )
}
