use std::rc::Rc;

use crate::{
    color::BGRA8,
    math::{I16Dot16, Point2, Vec2},
    text::{
        self,
        layout::{MultilineTextShaper, TextWrapMode, TextWrapOptions},
        FontArena, FontDb,
    },
    HorizontalAlignment, I26Dot6, Ruby, TextStyle,
};

use icu_segmenter::{LineBreakStrictness, LineBreakWordOption};
use thiserror::Error;

use style::{CascadingStyleMap, FontSlant, StyleMap};

pub mod style;

// Layout engine coordinate space:
// Vec2::x, Point2::x = inline axis
// Vec2::y, Point2::y =  block axis
//
// Note that this whole system does not strictly match CSS, it does
// a few things differently for simplicity, but it works for our purposes.

pub type FixedL = I26Dot6;
pub type Point2L = Point2<FixedL>;
pub type Vec2L = Vec2<FixedL>;

#[derive(Debug)]
pub struct FragmentBox {
    offset: Vec2L,
    size: Vec2L,
}

#[derive(Debug)]
pub enum Fragment {
    Text(TextFragment),
    LineBox(LineBoxFragment),
    InlineContainer(InlineContainerFragment),
    BlockContainer(BlockContainerFragment),
}

#[derive(Debug)]
pub struct TextFragment {
    fbox: FragmentBox,
    style: Rc<TextStyle>,
    // self-referential
    glyphs: text::GlyphString<'static, Rc<str>>,
    font_arena: Rc<FontArena>,
    baseline_offset: Vec2L,
}

#[derive(Debug)]
pub struct LineBoxFragment {
    fbox: FragmentBox,
    children: Vec<Rc<TextFragment>>,
}

#[derive(Debug)]
pub struct InlineContainerFragment {
    fbox: FragmentBox,
    children: Vec<Rc<LineBoxFragment>>,
}

#[derive(Debug)]
pub struct BlockContainerFragment {
    fbox: FragmentBox,
    children: Vec<Rc<InlineContainerFragment>>,
}

#[derive(Debug)]
pub struct LayoutContext<'l, 'a> {
    dpi: u32,
    fonts: &'l mut FontDb<'a>,
}

#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    size: Vec2L,
}

#[derive(Debug, Clone)]
pub struct BlockContainer {
    style: StyleMap,
    contents: Vec<InlineContainer>,
}

#[derive(Debug, Clone)]
pub struct InlineContainer {
    style: StyleMap,
    contents: Vec<InlineText>,
}

#[derive(Debug, Clone)]
pub struct InlineText {
    style: StyleMap,
    text: Rc<str>,
    ruby: Ruby,
}

#[derive(Debug, Error)]
pub enum InlineLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::font_db::SelectError),
    #[error(transparent)]
    TextLayout(#[from] text::layout::LayoutError),
}

fn layout_inline(
    offset: Vec2L,
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
            style
                .get::<style::FontFamily>()
                .map(|f| f.0.as_slice())
                .unwrap_or_default()
                .iter()
                .map(|b| &**b),
            text::FontStyle {
                weight: style.get_unwrap_copy_or::<style::FontWeight>(I16Dot16::new(400)),
                italic: match style.get_copy_or::<style::FontSlant>(FontSlant::Regular) {
                    FontSlant::Regular => false,
                    FontSlant::Italic => true,
                },
            },
            style.get_unwrap_copy_or::<style::FontSize>(I26Dot6::new(16) * context.dpi as i32 / 72),
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
        parent_style.get_unwrap_copy_or::<style::TextAlign>(HorizontalAlignment::Left),
        TextWrapOptions {
            mode: parent_style.get_copy_or::<TextWrapMode>(TextWrapMode::Normal),
            strictness: parent_style
                .get_copy_or::<LineBreakStrictness>(LineBreakStrictness::Normal),
            word_break: parent_style
                .get_copy_or::<LineBreakWordOption>(LineBreakWordOption::Normal),
        },
        constraints.size.x,
        unsafe { std::mem::transmute::<&FontArena, &'static FontArena>(&font_arena) },
        context.fonts,
    )?;

    let mut result = InlineContainerFragment {
        fbox: FragmentBox {
            offset,
            size: total_rect.size(),
        },
        children: Vec::new(),
    };

    for line in lines {
        let mut line_box = LineBoxFragment {
            fbox: FragmentBox {
                offset: line.bounding_rect.min - total_rect.min,
                size: line.bounding_rect.size(),
            },
            children: Vec::new(),
        };

        for segment in line.segments {
            let style = container_style
                .push(&container.contents[segment.corresponding_input_segment].style);
            line_box.children.push(Rc::new(TextFragment {
                fbox: FragmentBox {
                    offset: segment.logical_rect.min - line.bounding_rect.min,
                    size: segment.logical_rect.size(),
                },
                style: Rc::new(TextStyle {
                    color: style.get_unwrap_copy_or::<style::Color>(BGRA8::WHITE),
                    background_color: style
                        .get_unwrap_copy_or::<style::BackgroundColor>(BGRA8::ZERO),
                    // TODO: text decorations should be propagated somewhat differently
                    decorations: crate::TextDecorations::default(),
                    // TODO: text shadows should be propagated somewhat differently
                    shadows: Vec::new(),
                }),
                glyphs: segment.glyphs,
                font_arena: font_arena.clone(),
                baseline_offset: segment.baseline_offset - segment.logical_rect.min,
            }));
        }

        result.children.push(Rc::new(line_box));
    }

    Ok(result)
}

fn layout_block(
    offset: Vec2L,
    context: &mut LayoutContext,
    constraints: &LayoutConstraints,
    parent_style: &CascadingStyleMap,
    container: &BlockContainer,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    let container_style = parent_style.push(&container.style);

    let mut result = BlockContainerFragment {
        fbox: FragmentBox {
            offset,
            size: Vec2L::ZERO,
        },
        children: Vec::new(),
    };

    for child in &container.contents {
        let fragment = layout_inline(
            Vec2L::new(FixedL::ZERO, result.fbox.size.y),
            context,
            &LayoutConstraints {
                size: Vec2L::new(constraints.size.x, constraints.size.y - result.fbox.size.y),
            },
            &container_style,
            child,
        )?;

        result.fbox.size.x = result.fbox.size.x.max(fragment.fbox.size.x);
        result.fbox.size.y += fragment.fbox.size.y;

        result.children.push(fragment.into());
    }

    Ok(result)
}

pub fn layout(
    mut context: LayoutContext,
    constraints: LayoutConstraints,
    root: &BlockContainer,
    style: &StyleMap,
) -> Result<BlockContainerFragment, InlineLayoutError> {
    layout_block(
        Vec2L::ZERO,
        &mut context,
        &constraints,
        &CascadingStyleMap::new(style),
        root,
    )
}

// TODO: Once a built-in tofu font is added some tests could be made
//       that use this tofu font as a mock font for reliable metrics.
//       Using system fonts for tests is a bad idea for many reasons.
#[cfg(test)]
mod test {
    use crate::text::FontDb;

    use super::{
        layout,
        style::{self, StyleMap},
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
                        ruby: crate::Ruby::None,
                    }],
                },
                InlineContainer {
                    style: StyleMap::new(),
                    contents: vec![InlineText {
                        style: StyleMap::new(),
                        text: "this is a separate inline container".into(),
                        ruby: crate::Ruby::None,
                    }],
                },
            ],
        };

        let style = {
            let mut s = StyleMap::new();
            s.set(style::FontFamily(vec!["Noto Sans CJK JP".into()]));
            s
        };

        let fragment = layout(
            LayoutContext {
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
