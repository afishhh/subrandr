use std::rc::Rc;

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use thiserror::Error;

use rasterize::color::BGRA8;
use util::math::{I16Dot16, I26Dot6, Point2, Vec2};

use crate::{
    style::{
        self,
        types::{FontSlant, HorizontalAlignment, Ruby, TextStyle},
        CascadingStyleMap, StyleMap,
    },
    text::{
        self,
        layout::{MultilineTextShaper, TextWrapMode, TextWrapOptions},
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
    pub style: Rc<TextStyle>,
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

#[derive(Debug, Clone)]
pub struct LineBoxFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, Rc<TextFragment>)>,
}

#[derive(Debug, Clone)]
pub struct InlineContainerFragment {
    pub fbox: FragmentBox,
    pub lines: Vec<(Vec2L, Rc<LineBoxFragment>)>,
}

#[derive(Debug, Clone)]
pub struct BlockContainerFragment {
    pub fbox: FragmentBox,
    pub children: Vec<(Vec2L, Rc<InlineContainerFragment>)>,
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
    pub style: StyleMap,
    pub contents: Vec<InlineContainer>,
}

#[derive(Default, Debug, Clone)]
pub struct InlineContainer {
    pub style: StyleMap,
    pub contents: Vec<InlineText>,
}

#[derive(Debug, Clone)]
pub struct InlineText {
    pub style: StyleMap,
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
    parent_style: &CascadingStyleMap,
    container: &InlineContainer,
) -> Result<InlineContainerFragment, InlineLayoutError> {
    let font_arena = Rc::new(FontArena::new());

    let container_style = parent_style.push(&container.style);

    let mut shaper = MultilineTextShaper::new();
    let mut last_ruby_base = None;
    for segment in container.contents.iter() {
        let style = container_style.push(&segment.style);
        let matcher = text::FontMatcher::match_all(
            style.get::<style::FontFamily>(),
            text::FontStyle {
                weight: style.get_copy_or::<style::FontWeight, _>(I16Dot16::new(400)),
                italic: match style.get_copy_or_default::<style::FontStyle, _>() {
                    FontSlant::Regular => false,
                    FontSlant::Italic => true,
                },
            },
            style.get_copy_or::<style::FontSize, _>(I26Dot6::new(16) * context.dpi as i32 / 72),
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
        parent_style.get_copy_or::<style::TextAlign, _>(HorizontalAlignment::Left),
        TextWrapOptions {
            mode: parent_style.get_copy_or::<style::TextWrapStyle, _>(TextWrapMode::Normal),
            strictness: parent_style
                .get_copy_or::<style::LineBreak, _>(LineBreakStrictness::Normal),
            word_break: parent_style
                .get_copy_or::<style::WordBreak, _>(LineBreakWordOption::Normal),
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
            let style = container_style
                .push(&container.contents[segment.corresponding_input_segment].style);
            let offset = segment.logical_rect.min - line.bounding_rect.min;

            line_box.children.push((
                offset,
                Rc::new(TextFragment {
                    fbox: FragmentBox {
                        size: segment.logical_rect.size(),
                    },
                    style: Rc::new(TextStyle {
                        color: style.get_copy_or::<style::Color, _>(BGRA8::WHITE),
                        background_color: style
                            .get_copy_or::<style::BackgroundColor, _>(BGRA8::ZERO),
                        font_size: style.get_copy_or::<style::FontSize, _>(I26Dot6::new(16)),
                        // TODO: text decorations should be propagated somewhat differently
                        //       note that this is more the fault of the input
                        //       tree and not this code here
                        decorations: style.get_copy_or_default::<style::TextDecoration, _>(),
                        shadows: style.get::<style::TextShadows>(),
                    }),
                    glyphs: segment.glyphs,
                    _font_arena: font_arena.clone(),
                    baseline_offset: segment.baseline_offset - segment.logical_rect.min,
                }),
            ));
        }

        result.lines.push((offset, Rc::new(line_box)));
    }

    Ok(result)
}

fn layout_block(
    context: &mut LayoutContext,
    constraints: &LayoutConstraints,
    parent_style: &CascadingStyleMap,
    container: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    let container_style = parent_style.push(&container.style);

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
            &container_style,
            child,
        )?;

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
    style: &StyleMap,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    layout_block(context, &constraints, &CascadingStyleMap::new(style), root)
}

// TODO: Once a built-in tofu font is added some tests could be made
//       that use this tofu font as a mock font for reliable metrics.
//       Using system fonts for tests is a bad idea for many reasons.
#[cfg(test)]
mod test {
    use crate::text::FontDb;

    use super::{
        layout,
        style::{self, types::Ruby, StyleMap},
        BlockContainer, FixedL, InlineContainer, InlineText, LayoutConstraints, LayoutContext,
        Vec2L,
    };

    #[test]
    fn does_not_crash() {
        let tree = BlockContainer {
            style: StyleMap::new(),
            contents: vec![
                InlineContainer {
                    style: StyleMap::new(),
                    contents: vec![InlineText {
                        style: StyleMap::new(),
                        text: "hello world".into(),
                        ruby: Ruby::None,
                    }],
                },
                InlineContainer {
                    style: StyleMap::new(),
                    contents: vec![InlineText {
                        style: StyleMap::new(),
                        text: "this is a separate inline container".into(),
                        ruby: Ruby::None,
                    }],
                },
            ],
        };

        let style = {
            let mut s = StyleMap::new();
            s.set::<style::FontFamily>(vec!["Noto Sans".into()]);
            s
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
            &style,
        )
        .unwrap();

        dbg!(fragment);
    }
}
