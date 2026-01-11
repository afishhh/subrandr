use util::math::Vec2;

use super::{
    inline::{InlineContent, InlineContentFragment, PartialInline},
    FixedL, FragmentBox, InlineLayoutError, LayoutConstraints, LayoutContext, Vec2L,
};
use crate::style::{computed::HorizontalAlignment, ComputedStyle};

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
