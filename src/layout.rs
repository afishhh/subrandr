use std::rc::Rc;

use thiserror::Error;

use crate::{
    math::{I26Dot6, Point2, Vec2},
    style::{
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
    pub children: Vec<(Vec2L, InlineContainerFragment)>,
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

#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    pub size: Vec2L,
}

#[derive(Default, Debug, Clone)]
pub struct BlockContainer {
    pub style: ComputedStyle,
    pub contents: Vec<InlineContainer>,
}

#[derive(Default, Debug, Clone)]
pub struct InlineContainer {
    pub style: ComputedStyle,
    pub contents: Vec<InlineText>,
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

fn layout_inline(
    context: &mut LayoutContext,
    constraints: &LayoutConstraints,
    container: &InlineContainer,
) -> Result<InlineContainerFragment, InlineLayoutError> {
    let font_arena = Rc::new(FontArena::new());

    let mut shaper = MultilineTextShaper::new();
    let mut last_ruby_base = None;
    for segment in container.contents.iter() {
        let matcher = text::FontMatcher::match_all(
            segment.style.font_family(),
            text::FontStyle {
                weight: segment.style.font_weight(),
                italic: match segment.style.font_style() {
                    FontSlant::Italic => true,
                    FontSlant::Regular => false,
                },
            },
            segment.style.font_size(),
            context.dpi,
            unsafe { std::mem::transmute::<&FontArena, &'static FontArena>(&font_arena) },
            context.fonts,
        )?;

        match segment.ruby {
            Ruby::None => {
                shaper.add_text(&segment.text, matcher);
            }
            Ruby::Base => {
                last_ruby_base = Some(shaper.add_ruby_base(&segment.text, matcher));
            }
            Ruby::Over => {
                shaper.add_ruby_annotation(
                    last_ruby_base.expect("Ruby::Over without preceding Ruby::Base"),
                    segment.text.clone(),
                    matcher,
                );
                last_ruby_base = None;
            }
        }
    }

    let (lines, total_rect) = shaper.shape(
        container.style.text_align(),
        TextWrapOptions {
            mode: container.style.text_wrap_style(),
            strictness: container.style.line_break(),
            word_break: container.style.word_break(),
        },
        constraints.size.x,
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
            let style = &container.contents[segment.corresponding_input_segment].style;
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
        let fragment = layout_inline(
            context,
            &LayoutConstraints {
                size: Vec2L::new(constraints.size.x, constraints.size.y - result.fbox.size.y),
            },
            child,
        )?;

        result.fbox.size.x = result.fbox.size.x.max(fragment.fbox.size.x);
        result.fbox.size.y += fragment.fbox.size.y;

        result.children.push((child_offset, fragment));
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
    use std::rc::Rc;

    use crate::{
        style::{types::Ruby, ComputedStyle},
        text::FontDb,
    };

    use super::{
        layout, BlockContainer, FixedL, InlineContainer, InlineText, LayoutConstraints,
        LayoutContext, Vec2L,
    };

    #[test]
    fn does_not_crash() {
        let style = {
            let mut result = ComputedStyle::default();
            *result.make_font_family_mut() = Rc::new(["Noto Sans".into()]);
            result
        };

        let tree = BlockContainer {
            style: style.clone(),
            contents: vec![
                InlineContainer {
                    style: style.clone(),
                    contents: vec![InlineText {
                        style: style.clone(),
                        text: "hello world".into(),
                        ruby: Ruby::None,
                    }],
                },
                InlineContainer {
                    style,
                    contents: vec![InlineText {
                        style: ComputedStyle::default(),
                        text: "this is a separate inline container".into(),
                        ruby: Ruby::None,
                    }],
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
