use util::{
    math::{I16Dot16, I26Dot6, Point2, Vec2},
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

#[derive(Debug, Clone, Copy)]
pub struct FragmentBox {
    pub size: Vec2L,
}

#[derive(Debug, Clone)]
pub struct BlockContainerFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, Rc<InlineContentFragment>)>,
}

impl BlockContainerFragment {
    pub const fn empty() -> Self {
        Self {
            fbox: FragmentBox { size: Vec2L::ZERO },
            children: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct LayoutContext<'l, 'a> {
    pub dpi: u32,
    pub fonts: &'l mut FontDb<'a>,
}

impl LayoutContext<'_, '_> {
    fn pixel_scale(&self) -> I16Dot16 {
        I16Dot16::from_quotient(self.dpi as i32, 72)
    }
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
        fbox: FragmentBox { size: Vec2L::ZERO },
        children: Vec::new(),
    };

    for child in &container.contents {
        let child_offset = Vec2L::new(FixedL::ZERO, result.fbox.size.y);
        let fragment = {
            inline::layout(
                context,
                child,
                &LayoutConstraints {
                    size: Vec2L::new(constraints.size.x, constraints.size.y - result.fbox.size.y),
                },
                container.style.text_align(),
            )?
        };

        result.fbox.size.x = result.fbox.size.x.max(fragment.fbox.size.x);
        result.fbox.size.y += fragment.fbox.size.y;

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
