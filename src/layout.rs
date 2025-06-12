use util::{
    math::{I16Dot16, I26Dot6, Point2, Vec2},
    rc::Rc,
};

use crate::{
    layout::inline::InlineContentFragment,
    style::{computed::Ruby, ComputedStyle},
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

#[derive(Default, Debug, Clone)]
pub struct BlockContainer {
    pub style: ComputedStyle,
    pub contents: Vec<Vec<InlineText>>,
}

pub mod inline;
pub use inline::InlineLayoutError;

// TODO: remove
#[derive(Debug, Clone)]
pub struct InlineText {
    pub style: ComputedStyle,
    pub text: std::rc::Rc<str>,
    pub ruby: Ruby,
}

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
            let mut builder = inline::InlineContentBuilder::new();
            {
                let mut root = builder.root();
                let mut it = child.iter();
                while let Some(segment) = it.next() {
                    match segment.ruby {
                        Ruby::None => {
                            root.push_span(segment.style.clone())
                                .push_text(&segment.text);
                        }
                        Ruby::Base => {
                            let mut ruby = root.push_ruby(segment.style.clone());
                            ruby.push_base(segment.style.clone())
                                .push_text(&segment.text);
                            if let Some(next) = it.as_slice().first() {
                                if let Ruby::Over = next.ruby {
                                    ruby.push_annotation(next.style.clone())
                                        .push_text(&next.text);
                                    _ = it.next();
                                }
                            }
                        }
                        Ruby::Over => {
                            root.push_ruby(segment.style.clone())
                                .push_annotation(segment.style.clone())
                                .push_text(&segment.text);
                        }
                    }
                }
            }

            inline::layout(
                context,
                &builder.finish(),
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
        layout, BlockContainer, FixedL, InlineText, LayoutConstraints, LayoutContext, Vec2L,
    };
    use crate::{
        style::{computed::Ruby, ComputedStyle},
        text::FontDb,
    };

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
                vec![InlineText {
                    style: text_style.clone(),
                    text: "hello world".into(),
                    ruby: Ruby::None,
                }],
                vec![InlineText {
                    style: text_style,
                    text: "this is a separate inline container".into(),
                    ruby: Ruby::None,
                }],
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
