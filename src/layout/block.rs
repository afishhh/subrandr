use util::math::Vec2;

use super::{
    inline::{InlineContent, InlineContentFragment, PartialInline},
    FixedL, FragmentBox, InlineLayoutError, LayoutConstraints, LayoutContext, Vec2L,
};
use crate::style::{
    computed::{BaselineSource, HorizontalAlignment, ToPhysicalPixels},
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
    intrinsic_width: FixedL,
    content: PartialBlockContainerContent<'a>,
}

#[allow(clippy::large_enum_variant)] // shouldn't be moved around much anyway
enum PartialBlockContainerContent<'a> {
    Inline(PartialInline<'a>),
    Block(Vec<PartialBlockContainer<'a>>),
}

impl PartialBlockContainer<'_> {
    pub fn intrinsic_width(&self) -> FixedL {
        self.intrinsic_width
    }

    pub fn layout(
        self,
        lctx: &mut LayoutContext,
        constraints: &LayoutConstraints,
    ) -> Result<BlockContainerFragment, InlineLayoutError> {
        // https://www.w3.org/TR/CSS2/visudet.html#blockwidth
        // 'width' is assumed to be 'auto'
        let width = constraints.size.x
            - (self.style.padding_left() + self.style.padding_right()).to_physical_pixels(lctx.dpi);
        let mut height = FixedL::ZERO;

        let content = match self.content {
            PartialBlockContainerContent::Inline(inline) => {
                let fragment = inline.layout(
                    lctx,
                    &LayoutConstraints {
                        size: Vec2::new(width, FixedL::MAX),
                    },
                )?;
                let x_offset = match self.style.text_align() {
                    HorizontalAlignment::Left => FixedL::ZERO,
                    HorizontalAlignment::Center => (width - fragment.fbox.size_for_layout().x) / 2,
                    HorizontalAlignment::Right => width - fragment.fbox.size_for_layout().x,
                };
                height += fragment.fbox.size_for_layout().y;
                BlockContainerFragmentContent::Inline(Vec2L::new(x_offset, FixedL::ZERO), fragment)
            }
            PartialBlockContainerContent::Block(children) => {
                let mut fragments = Vec::new();
                for child in children {
                    let fragment = child.layout(
                        lctx,
                        &LayoutConstraints {
                            size: Vec2::new(width, constraints.size.y - height),
                        },
                    )?;
                    let off = Vec2L::new(FixedL::ZERO, height);
                    height += fragment.fbox.size_for_layout().y;
                    fragments.push((off, fragment))
                }
                BlockContainerFragmentContent::Block(fragments)
            }
        };

        Ok(BlockContainerFragment {
            fbox: FragmentBox::new_styled(Vec2::new(width, height), lctx.dpi, &self.style),
            style: self.style,
            content,
        })
    }
}

impl std::fmt::Debug for PartialBlockContainer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartialBlockContainer")
            .field("style", &self.style)
            .field("intrinsic_width", &self.intrinsic_width)
            .finish_non_exhaustive()
    }
}

pub fn layout_initial<'a>(
    lctx: &mut LayoutContext,
    container: &'a BlockContainer,
) -> Result<PartialBlockContainer<'a>, InlineLayoutError> {
    let mut intrinsic_content_width = FixedL::ZERO;
    let content = match &container.content {
        BlockContainerContent::Inline(inline) => {
            let inline = super::inline::shape(lctx, inline)?;
            intrinsic_content_width = inline.intrinsic_width(lctx);
            PartialBlockContainerContent::Inline(inline)
        }
        BlockContainerContent::Block(children) => {
            let mut partials = Vec::new();
            for child in children {
                let block = layout_initial(lctx, child)?;
                intrinsic_content_width = intrinsic_content_width.max(block.intrinsic_width);
                partials.push(block);
            }

            PartialBlockContainerContent::Block(partials)
        }
    };

    Ok(PartialBlockContainer {
        style: container.style.clone(),
        intrinsic_width: intrinsic_content_width
            + (container.style.padding_left() + container.style.padding_right())
                .to_physical_pixels(lctx.dpi),
        content,
    })
}

#[cfg_attr(not(all(test, feature = "_layout_tests")), expect(dead_code))]
pub fn layout(
    lctx: &mut LayoutContext,
    constraints: &LayoutConstraints,
    container: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    layout_initial(lctx, container)?.layout(lctx, constraints)
}
