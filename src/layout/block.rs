use util::math::Vec2;

use super::{
    inline::{InlineContent, InlineContentFragment, PartialInline},
    FixedL, FragmentBox, InlineLayoutError, LayoutContext, Vec2L,
};
use crate::{
    layout::EdgeExtents,
    style::{
        computed::{BaselineSource, Direction, HorizontalAlignment, ToPhysicalPixels},
        ComputedStyle,
    },
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

    pub fn from_inline(inline: InlineContentFragment) -> Self {
        Self {
            fbox: FragmentBox::new_content_only(inline.fbox.content_size),
            style: ComputedStyle::DEFAULT,
            content: BlockContainerFragmentContent::Inline(Vec2L::ZERO, inline),
        }
    }

    pub(super) fn alphabetic_baseline_from(&self, source: BaselineSource) -> Option<FixedL> {
        // Here so this code blows up if `BaselineSource` is ever extended.
        match source {
            BaselineSource::Last => (),
        }

        match &self.content {
            BlockContainerFragmentContent::Inline(off, inline_content_fragment) => {
                let (line_off, line) = inline_content_fragment.lines.last()?;
                Some(off.y + line_off.y + line.baseline_y)
            }
            BlockContainerFragmentContent::Block(children) => {
                children.iter().rev().find_map(|&(child_off, ref child)| {
                    child
                        .alphabetic_baseline()
                        .map(|child_baseline| child_baseline + child_off.y)
                })
            }
        }
    }

    pub(super) fn alphabetic_baseline(&self) -> Option<FixedL> {
        self.alphabetic_baseline_from(self.style.baseline_source())
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

pub(super) struct BlockInlineSizes {
    margin_left: FixedL,
    width: FixedL,
    margin_right: FixedL,
}

struct BlockComputedInlineSizes {
    margin_left: Option<FixedL>,
    padding_left: FixedL,
    width: Option<FixedL>,
    padding_right: FixedL,
    margin_right: Option<FixedL>,
}

impl BlockInlineSizes {
    // https://www.w3.org/TR/CSS2/visudet.html#blockwidth
    fn compute_for_nonreplaced_block(
        BlockComputedInlineSizes {
            mut margin_left,
            padding_left,
            width,
            padding_right,
            mut margin_right,
        }: BlockComputedInlineSizes,
        containing_block: &ContainingBlock,
    ) -> Self {
        // If 'width' is not 'auto' and 'border-left-width' + 'padding-left' + 'width' + 'padding-right' + 'border-right-width' (plus any of 'margin-left' or 'margin-right' that are not 'auto') is larger than the width of the containing block,
        if let Some(width) = width {
            if margin_left.unwrap_or(FixedL::ZERO)
                + padding_left
                + width
                + padding_right
                + margin_right.unwrap_or(FixedL::ZERO)
                > containing_block.width
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
                match containing_block.style.direction() {
                    // If the 'direction' property of the containing block has the value 'ltr',
                    Direction::Ltr => {
                        used_margin_left = margin_left;
                        // the specified value of 'margin-right' is ignored and the value is calculated so as to make the equality true.
                        used_margin_right =
                            containing_block.width - (base_width + width + margin_left);
                    }
                    // If the value of 'direction' is 'rtl',
                    Direction::Rtl => {
                        used_margin_right = margin_right;
                        // this happens to 'margin-left' instead.
                        used_margin_left =
                            containing_block.width - (base_width + width + margin_right);
                    }
                }
            }
            // If there is exactly one value specified as 'auto', its used value follows from the equality.
            (Some(margin_left), Some(width), None) => {
                used_margin_left = margin_left;
                used_width = width;
                used_margin_right = containing_block.width - (base_width + width + margin_left);
            }
            (None, Some(width), Some(margin_right)) => {
                used_margin_right = margin_right;
                used_width = width;
                used_margin_left = containing_block.width - (base_width + width + margin_right);
            }
            // If 'width' is set to 'auto',
            (_, None, _) => {
                // any other 'auto' values become '0' and 'width' follows from the resulting equality.
                used_margin_left = margin_left.unwrap_or(FixedL::ZERO);
                used_margin_right = margin_right.unwrap_or(FixedL::ZERO);
                used_width =
                    containing_block.width - (base_width + used_margin_left + used_margin_right);
            }
            // If both 'margin-left' and 'margin-right' are 'auto',
            (None, Some(width), None) => {
                used_width = width;
                // their used values are equal.
                let margin = containing_block.width - (base_width + width);
                used_margin_left = margin / 2;
                used_margin_right = margin - used_margin_left;
            }
        }

        BlockInlineSizes {
            margin_left: used_margin_left,
            width: used_width,
            margin_right: used_margin_right,
        }
    }

    // https://www.w3.org/TR/CSS2/visudet.html#shrink-to-fit-float
    fn floating_shrink_to_fit_width(
        lctx: &mut LayoutContext,
        content: &PartialBlockContainerContent,
        margin_left: FixedL,
        padding_left: FixedL,
        padding_right: FixedL,
        margin_right: FixedL,
        containing_block: &ContainingBlock,
    ) -> Result<FixedL, InlineLayoutError> {
        // calculate the preferred width by formatting the content without breaking lines other than where explicit line breaks occur
        let preferred_width = content.max_width(lctx)?;
        // TODO: minimum width
        // Thirdly, find the available width: in this case, this is the width of the containing block minus the used values of 'margin-left', 'border-left-width', 'padding-left', 'padding-right', 'border-right-width', 'margin-right', and the widths of any relevant scroll bars.
        let available_width =
            containing_block.width - margin_left - padding_left - padding_right - margin_right;

        // Then the shrink-to-fit width is: min(max(preferred minimum width, available width), preferred width).
        Ok(std::cmp::min(available_width, preferred_width))
    }

    // https://www.w3.org/TR/CSS2/visudet.html#inlineblock-width
    fn compute_for_nonreplaced_inline(
        lctx: &mut LayoutContext,
        content: &PartialBlockContainerContent,
        BlockComputedInlineSizes {
            margin_left,
            padding_left,
            width,
            padding_right,
            margin_right,
        }: BlockComputedInlineSizes,
        containing_block: &ContainingBlock,
    ) -> Result<Self, InlineLayoutError> {
        // A computed value of 'auto' for 'margin-left' or 'margin-right' becomes a used value of '0'.
        let margin_left = margin_left.unwrap_or(FixedL::ZERO);
        let margin_right = margin_right.unwrap_or(FixedL::ZERO);

        // If 'width' is 'auto', the used value is the shrink-to-fit width as for floating elements.
        let width = match width {
            Some(width) => width,
            None => Self::floating_shrink_to_fit_width(
                lctx,
                content,
                margin_left,
                padding_left,
                padding_right,
                margin_right,
                containing_block,
            )?,
        };

        Ok(Self {
            margin_left,
            width,
            margin_right,
        })
    }
}

impl PartialBlockContainerContent<'_> {
    fn max_width(&self, lctx: &mut LayoutContext) -> Result<FixedL, InlineLayoutError> {
        match self {
            PartialBlockContainerContent::Inline(inline) => inline.max_width(lctx),
            PartialBlockContainerContent::Block(children) => {
                let mut result = FixedL::ZERO;

                for child in children {
                    result = result.max(child.max_width(lctx)?);
                }

                Ok(result)
            }
        }
    }
}

impl PartialBlockContainer<'_> {
    pub(super) fn inline_sizes_internal(
        &self,
        lctx: &mut LayoutContext,
        level: BlockLayoutLevel,
        containing_block: &ContainingBlock,
    ) -> Result<BlockInlineSizes, InlineLayoutError> {
        let computed = BlockComputedInlineSizes {
            margin_left: self.style.margin_left().to_physical_pixels(lctx.dpi),
            padding_left: self.style.padding_left().to_physical_pixels(lctx.dpi),
            width: self.style.width().to_physical_pixels(lctx.dpi),
            padding_right: self.style.padding_right().to_physical_pixels(lctx.dpi),
            margin_right: self.style.margin_right().to_physical_pixels(lctx.dpi),
        };
        let width = match level {
            BlockLayoutLevel::BlockLevel => {
                BlockInlineSizes::compute_for_nonreplaced_block(computed, containing_block)
            }
            BlockLayoutLevel::InlineLevel => BlockInlineSizes::compute_for_nonreplaced_inline(
                lctx,
                &self.content,
                computed,
                containing_block,
            )?,
        };

        Ok(width)
    }

    pub(super) fn inline_size_internal(
        &self,
        lctx: &mut LayoutContext,
        level: BlockLayoutLevel,
        containing_block: &ContainingBlock,
    ) -> Result<FixedL, InlineLayoutError> {
        self.inline_sizes_internal(lctx, level, containing_block)
            .map(|sizes| {
                self.style.padding_left().to_physical_pixels(lctx.dpi)
                    + sizes.margin_left
                    + sizes.width
                    + sizes.margin_right
                    + self.style.padding_right().to_physical_pixels(lctx.dpi)
            })
    }

    pub fn inline_size(
        &self,
        lctx: &mut LayoutContext,
        containing_block: &ContainingBlock,
    ) -> Result<FixedL, InlineLayoutError> {
        self.inline_size_internal(lctx, BlockLayoutLevel::BlockLevel, containing_block)
    }

    fn max_width(&self, lctx: &mut LayoutContext) -> Result<FixedL, InlineLayoutError> {
        let inner_width = match self.style.width().to_physical_pixels(lctx.dpi) {
            Some(width) => width,
            None => self.content.max_width(lctx)?,
        };

        Ok(self
            .style
            .margin_left()
            .to_physical_pixels(lctx.dpi)
            .unwrap_or(FixedL::ZERO)
            + self.style.padding_left().to_physical_pixels(lctx.dpi)
            + inner_width
            + self.style.padding_right().to_physical_pixels(lctx.dpi)
            + self
                .style
                .margin_right()
                .to_physical_pixels(lctx.dpi)
                .unwrap_or(FixedL::ZERO))
    }

    pub(super) fn layout_internal(
        &self,
        lctx: &mut LayoutContext,
        inline_sizes: BlockInlineSizes,
    ) -> Result<BlockContainerFragment, InlineLayoutError> {
        let mut height = FixedL::ZERO;
        let new_containing_block = ContainingBlock {
            style: &self.style,
            width: inline_sizes.width,
        };
        let content = match &self.content {
            PartialBlockContainerContent::Inline(inline) => {
                let fragment = inline.layout(lctx, &new_containing_block)?;
                let x_offset = match self.style.text_align() {
                    HorizontalAlignment::Left => FixedL::ZERO,
                    HorizontalAlignment::Center => {
                        (inline_sizes.width - fragment.fbox.size_for_layout().x) / 2
                    }
                    HorizontalAlignment::Right => {
                        inline_sizes.width - fragment.fbox.size_for_layout().x
                    }
                };
                height += fragment.fbox.size_for_layout().y;
                BlockContainerFragmentContent::Inline(Vec2L::new(x_offset, FixedL::ZERO), fragment)
            }
            PartialBlockContainerContent::Block(children) => {
                let mut fragments = Vec::new();
                for child in children {
                    let child_inline_sizes = child.inline_sizes_internal(
                        lctx,
                        BlockLayoutLevel::BlockLevel,
                        &new_containing_block,
                    )?;
                    let fragment = child.layout_internal(
                        lctx,
                        // both belong to in-flow block-level boxes that participate in the same block formatting context
                        child_inline_sizes,
                    )?;

                    let off = Vec2L::new(FixedL::ZERO, height);
                    height += fragment.fbox.size_for_layout().y;
                    fragments.push((off, fragment));
                }

                BlockContainerFragmentContent::Block(fragments)
            }
        };

        Ok(BlockContainerFragment {
            style: self.style.clone(),
            fbox: FragmentBox {
                content_size: Vec2::new(inline_sizes.width, height),
                padding: EdgeExtents::padding(&self.style, lctx.dpi),
                margin: EdgeExtents {
                    top: FixedL::ZERO,
                    bottom: FixedL::ZERO,
                    left: inline_sizes.margin_left,
                    right: inline_sizes.margin_right,
                },
            },
            content,
        })
    }

    pub fn layout(
        self,
        lctx: &mut LayoutContext,
        containing_block: &ContainingBlock,
    ) -> Result<BlockContainerFragment, InlineLayoutError> {
        let inline_sizes =
            self.inline_sizes_internal(lctx, BlockLayoutLevel::BlockLevel, containing_block)?;
        self.layout_internal(lctx, inline_sizes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockLayoutLevel {
    // block
    BlockLevel,
    // inline-block
    InlineLevel,
}

#[derive(Debug)]
pub struct ContainingBlock<'a> {
    pub(super) style: &'a ComputedStyle,
    pub(super) width: FixedL,
}

impl ContainingBlock<'static> {
    pub fn initial(size: Vec2L) -> Self {
        Self {
            style: const { &ComputedStyle::DEFAULT },
            width: size.x,
        }
    }

    pub fn infinite_initial() -> Self {
        Self {
            style: const { &ComputedStyle::DEFAULT },
            width: FixedL::MAX,
        }
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
    containing_block: &ContainingBlock,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    layout_initial(lctx, container)?.layout(lctx, containing_block)
}
