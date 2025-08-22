use std::fmt::Debug;

use util::{
    math::{BoolExt, I26Dot6, Point2, Rect2, Vec2},
    rc::Rc,
};

use crate::{
    layout::inline::{InlineContent, InlineContentFragment},
    style::ComputedStyle,
    text::FontDb,
};

// Layout engine coordinate space:
// Vec2::x, Point2::x = inline axis
// Vec2::y, Point2::y =  block axis
//
// Note that this whole system does not strictly match CSS, it does
// a few things differently for simplicity, but it works for our purposes.

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
}

impl FragmentBox {
    const ZERO: Self = Self {
        content_size: Vec2L::ZERO,
        padding: EdgeExtents::ZERO,
    };

    const fn new_content_only(content_size: Vec2L) -> Self {
        Self {
            content_size,
            padding: EdgeExtents::ZERO,
        }
    }

    // TODO: Make a newtype for dpi everywhere
    fn new_styled(content_size: Vec2L, dpi: u32, style: &ComputedStyle) -> Self {
        Self::new_styled_fragmented(content_size, dpi, style, BoxFragmentationPart::FULL)
    }

    fn new_styled_fragmented(
        content_size: Vec2L,
        dpi: u32,
        style: &ComputedStyle,
        part: BoxFragmentationPart,
    ) -> Self {
        Self {
            content_size,
            padding: EdgeExtents::compute_fragmented(
                part,
                || style.padding_top().to_physical_pixels(dpi),
                || style.padding_bottom().to_physical_pixels(dpi),
                || style.padding_left().to_physical_pixels(dpi),
                || style.padding_right().to_physical_pixels(dpi),
            ),
        }
    }

    pub fn content_offset(&self) -> Vec2L {
        Vec2L::new(self.padding.left, self.padding.top)
    }

    pub fn padding_box(&self) -> Rect2L {
        Rect2L::from_min_size(
            Point2L::ZERO,
            self.content_size
                + Vec2L::new(
                    self.padding.left + self.padding.right,
                    self.padding.top + self.padding.bottom,
                ),
        )
    }

    pub fn margin_box(&self) -> Rect2L {
        self.padding_box()
    }

    pub fn size_for_layout(&self) -> Vec2L {
        self.margin_box().max.to_vec()
    }
}

#[derive(Debug, Clone)]
pub struct BlockContainerFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, Rc<InlineContentFragment>)>,
}

impl BlockContainerFragment {
    pub const EMPTY: Self = Self {
        fbox: FragmentBox {
            content_size: Vec2L::ZERO,
            padding: EdgeExtents::ZERO,
        },
        children: Vec::new(),
    };
}

#[derive(Debug)]
pub struct LayoutContext<'l, 'a> {
    pub dpi: u32,
    pub fonts: &'l mut FontDb<'a>,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    pub size: Vec2L,
}

#[derive(Debug, Clone)]
pub struct BlockContainer {
    pub style: ComputedStyle,
    pub contents: Vec<InlineContent>,
}

pub mod inline;
pub use inline::InlineLayoutError;

fn layout_block(
    context: &mut LayoutContext,
    constraints: &LayoutConstraints,
    container: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    let mut result = BlockContainerFragment {
        fbox: FragmentBox::new_styled(Vec2L::ZERO, context.dpi, &container.style),
        children: Vec::new(),
    };

    for child in &container.contents {
        let child_offset = Vec2L::new(FixedL::ZERO, result.fbox.content_size.y);
        let fragment = {
            inline::layout(
                context,
                child,
                &LayoutConstraints {
                    size: Vec2L::new(
                        constraints.size.x,
                        constraints.size.y - result.fbox.content_size.y,
                    ),
                },
                container.style.text_align(),
            )?
        };

        let fragment_size = fragment.fbox.size_for_layout();
        result.fbox.content_size.x = result.fbox.content_size.x.max(fragment_size.x);
        result.fbox.content_size.y += fragment_size.y;

        result.children.push((child_offset, Rc::new(fragment)));
    }

    Ok(result)
}

pub fn layout(
    context: &mut LayoutContext,
    constraints: LayoutConstraints,
    root: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    layout_block(context, &constraints, root)
}

// TODO: Once a built-in tofu font is added some tests could be made
//       that use this tofu font as a mock font for reliable metrics.
//       Using system fonts for tests is a bad idea for many reasons.
#[cfg(test)]
mod test {
    use util::rc_static;

    use super::{
        inline::InlineContentBuilder, layout, BlockContainer, FixedL, LayoutConstraints,
        LayoutContext, Vec2L,
    };
    use crate::{style::ComputedStyle, text::FontDb};

    #[test]
    fn does_not_crash() {
        let text_style = {
            let mut s = ComputedStyle::DEFAULT;
            *s.make_font_family_mut() = rc_static!([rc_static!(str b"Noto Sans")]);
            s
        };

        let tree = BlockContainer {
            style: ComputedStyle::DEFAULT,
            contents: vec![
                {
                    let mut builder = InlineContentBuilder::new();
                    builder
                        .root()
                        .push_span(text_style.clone())
                        .push_text("hello world");
                    builder.finish()
                },
                {
                    let mut builder = InlineContentBuilder::new();
                    builder
                        .root()
                        .push_span(text_style)
                        .push_text("this is a separate inline container");
                    builder.finish()
                },
            ],
        };

        let fragment = layout(
            &mut LayoutContext {
                dpi: 72,
                fonts: &mut FontDb::new(&crate::Subrandr::init()).unwrap(),
            },
            LayoutConstraints {
                size: Vec2L::new(FixedL::new(100), FixedL::new(100)),
            },
            &tree,
        )
        .unwrap();

        dbg!(fragment);
    }
}
