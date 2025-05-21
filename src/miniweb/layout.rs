use std::rc::Rc;

use thiserror::Error;

use rasterize::color::BGRA8;
use util::math::{I16Dot16, I26Dot6, Point2, Vec2};

use crate::{
    miniweb::style::{
        types::{FontSlant, Ruby},
        ComputedStyle,
    },
    text::{
        self,
        layout::{MultilineTextShaper, TextWrapOptions},
        FontArena, FontDb,
    },
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

#[derive(Debug)]
pub struct TextFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    // self-referential
    glyphs: text::GlyphString<'static, Rc<str>>,
    _font_arena: Rc<FontArena>,
    pub baseline_offset: Vec2L,
}

impl TextFragment {
    pub fn glyphs(&self) -> &text::GlyphString<'_, Rc<str>> {
        &self.glyphs
    }
}

#[derive(Debug)]
pub struct LineBoxFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, TextFragment)>,
}

#[derive(Debug)]
pub struct InlineContainerFragment {
    pub fbox: FragmentBox,
    pub lines: Vec<(Vec2L, LineBoxFragment)>,
}

#[derive(Debug)]
pub struct BlockContainerFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, BlockFragmentChild)>,
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
pub enum BlockFragmentChild {
    Inline(InlineContainerFragment),
    Block(BlockContainerFragment),
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

#[derive(Default, Debug, Clone)]
pub struct BlockContainer {
    pub style: ComputedStyle,
    pub contents: Vec<Container>,
}

#[derive(Default, Debug, Clone)]
pub struct InlineContainer {
    pub style: ComputedStyle,
    // TODO: flatten tree here already
    //       it makes things easier
    pub contents: Vec<InlineChild>,
}

#[derive(Default, Debug, Clone)]
pub struct RubyContainer {
    pub style: ComputedStyle,
    pub contents: Vec<InlineChild>,
}

#[derive(Debug, Clone)]
pub enum InlineChild {
    // Container(InlineContainer),
    Ruby(RubyContainer),
    // AtomicBlock(BlockContainer),
    Text(InlineText),
}

#[derive(Debug, Clone)]
pub enum Container {
    Inline(InlineContainer),
    Block(BlockContainer),
}

#[derive(Debug, Clone)]
pub struct InlineText {
    pub style: ComputedStyle,
    pub text: Rc<str>,
    pub ruby: Ruby,
}

#[derive(Debug, Error)]
pub enum InlineLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::font_db::SelectError),
    #[error(transparent)]
    TextLayout(#[from] text::layout::LayoutError),
}

fn flow_layout_inline(
    context: &mut LayoutContext,
    constraints: &LayoutConstraints,
    container: &InlineContainer,
) -> Result<InlineContainerFragment, InlineLayoutError> {
    let font_arena = Rc::new(FontArena::new());

    let mut shaper = MultilineTextShaper::new();

    let mut styles = vec![];

    fn rec(
        shaper: &mut MultilineTextShaper,
        ctx: &mut LayoutContext,
        styles: &mut Vec<ComputedStyle>,
        font_arena: &'static FontArena,
        child: &InlineChild,
    ) -> Result<(), InlineLayoutError> {
        match child {
            InlineChild::Ruby(ruby) => {}
            InlineChild::Text(text) => {
                let matcher = text::FontMatcher::match_all(
                    text.style.font_family(),
                    text::FontStyle {
                        weight: text.style.font_weight(),
                        italic: match text.style.font_style() {
                            FontSlant::Italic => true,
                            FontSlant::Regular => false,
                        },
                    },
                    text.style.font_size(),
                    ctx.dpi,
                    font_arena,
                    ctx.fonts,
                )?;

                styles.push(text.style.clone());

                match text.ruby {
                    Ruby::None => {
                        shaper.add_text(&text.text, matcher);
                    }
                    Ruby::Base => {
                        // last_ruby_base = Some(shaper.add_ruby_base(&segment.text, matcher));
                    }
                    Ruby::Over => {
                        // shaper.add_ruby_annotation(
                        //     last_ruby_base.expect("Ruby::Over without preceding Ruby::Base"),
                        //     segment.text.clone(),
                        //     matcher,
                        // );
                        // last_ruby_base = None;
                    }
                }
            }
        }

        Ok(())
    }

    for segment in container.contents.iter() {
        rec(
            &mut shaper,
            context,
            &mut styles,
            unsafe { std::mem::transmute::<&FontArena, &'static FontArena>(&font_arena) },
            segment,
        )?
    }

    let (lines, total_rect) = shaper.shape(
        container.style.text_align(),
        TextWrapOptions {
            mode: container.style.text_wrap_style(),
            strictness: container.style.line_break(),
            word_break: container.style.word_break(),
        },
        constraints.size.x,
        text::layout::LineHeight::Normal,
        unsafe { std::mem::transmute::<&FontArena, &'static FontArena>(&font_arena) },
        context.fonts,
    )?;

    let mut result = InlineContainerFragment {
        fbox: FragmentBox {
            size: total_rect.size(),
        },
        lines: Vec::new(),
    };

    for line in lines {
        let offset = line.bounding_rect.min - total_rect.min;
        let mut line_box = LineBoxFragment {
            fbox: FragmentBox {
                size: line.bounding_rect.size(),
            },
            children: Vec::new(),
        };

        for segment in line.segments {
            let style = &styles[segment.corresponding_input_segment];
            let offset = segment.logical_rect.min - line.bounding_rect.min;

            line_box.children.push((
                offset,
                TextFragment {
                    fbox: FragmentBox {
                        size: segment.logical_rect.size(),
                    },
                    style: style.clone(),
                    glyphs: segment.glyphs,
                    _font_arena: font_arena.clone(),
                    baseline_offset: segment.baseline_offset - segment.logical_rect.min,
                },
            ));
        }

        result.lines.push((offset, line_box));
    }

    Ok(result)
}

fn flow_layout_block(
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

        let (fbox, fragment) = match child {
            Container::Inline(inline) => {
                let fragment = flow_layout_inline(
                    context,
                    &LayoutConstraints {
                        size: Vec2L::new(
                            constraints.size.x,
                            constraints.size.y - result.fbox.size.y,
                        ),
                    },
                    inline,
                )?;

                (fragment.fbox, BlockFragmentChild::Inline(fragment))
            }
            Container::Block(block) => {
                let fragment = flow_layout_block(
                    context,
                    &LayoutConstraints {
                        size: Vec2L::new(
                            constraints.size.x,
                            constraints.size.y - result.fbox.size.y,
                        ),
                    },
                    block,
                )?;

                (fragment.fbox, BlockFragmentChild::Block(fragment))
            }
        };

        result.fbox.size.x = result.fbox.size.x.max(fbox.size.x);
        result.fbox.size.y += fbox.size.y;

        result.children.push((child_offset, fragment));
    }

    Ok(result)
}

pub fn layout(
    context: &mut LayoutContext,
    constraints: LayoutConstraints,
    root: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    flow_layout_block(context, &constraints, root)
}

#[derive(Debug)]
pub enum ContainerFragment {
    Inline(InlineContainerFragment),
    Block(BlockContainerFragment),
}

pub fn layout_any(
    context: &mut LayoutContext,
    constraints: LayoutConstraints,
    root: &Container,
) -> Result<ContainerFragment, InlineLayoutError> {
    match root {
        Container::Inline(inline) => {
            flow_layout_inline(context, &constraints, inline).map(ContainerFragment::Inline)
        }
        Container::Block(block) => {
            flow_layout_block(context, &constraints, block).map(ContainerFragment::Block)
        }
    }
}

// TODO: Once a built-in tofu font is added some tests could be made
//       that use this tofu font as a mock font for reliable metrics.
//       Using system fonts for tests is a bad idea for many reasons.
#[cfg(test)]
mod test {
    use std::rc::Rc;

    use crate::{
        miniweb::{
            layout::InlineChild,
            style::{types::Ruby, ComputedStyle},
        },
        text::FontDb,
    };

    use super::{
        layout, BlockContainer, Container, FixedL, InlineContainer, InlineText, LayoutConstraints,
        LayoutContext, Vec2L,
    };

    #[cfg_attr(not(miri), test)]
    fn does_not_crash() {
        let style = {
            let mut result = ComputedStyle::default();
            *result.make_font_family_mut() = Rc::new(["Noto Sans".into()]);
            result
        };

        let tree = BlockContainer {
            style: style.clone(),
            contents: vec![
                Container::Inline(InlineContainer {
                    style: style.clone(),
                    contents: vec![InlineChild::Text(InlineText {
                        style: style.clone(),
                        text: "hello world".into(),
                        ruby: Ruby::None,
                    })],
                }),
                Container::Inline(InlineContainer {
                    style,
                    contents: vec![InlineChild::Text(InlineText {
                        style: ComputedStyle::default(),
                        text: "this is a separate inline container".into(),
                        ruby: Ruby::None,
                    })],
                }),
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
