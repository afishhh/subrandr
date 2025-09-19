use std::{ops::Range, rc::Rc};

use icu_segmenter::{options::LineBreakOptions, GraphemeClusterSegmenter};
use thiserror::Error;
use util::math::{I26Dot6, Vec2};

use super::{FixedL, FragmentBox, LayoutConstraints, LayoutContext, Vec2L};
use crate::{
    style::{
        computed::{FontSlant, HorizontalAlignment},
        ComputedStyle,
    },
    text::{self, Direction, FontArena, FontMatcher, GlyphString},
};

// This character is used to represent opaque objects nested inside inline text content,
// this includes ruby containers and `inline-block`s.
const OBJECT_REPLACEMENT_CHARACTER: char = '\u{FFFC}';

/// A flat representation of inline content.
///
/// This structure stores a layout tree for inline content in a [`Vec`]
/// alongside an additional [`Vec`] of [`Rc<str>`]s that stores the
/// final text runs on which line breaking and bidi reordering will be
/// performed.
#[derive(Debug, Clone)]
pub struct InlineContent {
    text_runs: Vec<Rc<str>>,
    items: Vec<InlineItem>,
}

impl Default for InlineContent {
    fn default() -> Self {
        Self {
            text_runs: vec![Rc::from("")],
            items: Vec::new(),
        }
    }
}

pub struct InlineContentBuilder {
    text_runs: Vec<String>,
    items: Vec<InlineItem>,
}

impl InlineContentBuilder {
    pub fn new() -> Self {
        Self {
            text_runs: Vec::new(),
            items: Vec::new(),
        }
    }

    pub fn root(&mut self) -> InlineSpanBuilder<'_> {
        if self.text_runs.is_empty() {
            self.text_runs.push(String::new());
        }

        InlineSpanBuilder {
            parent: self,
            run_index: 0,
            span_index: usize::MAX,
            length: 0,
        }
    }

    pub fn finish(&mut self) -> InlineContent {
        InlineContent {
            text_runs: self.text_runs.drain(..).map(|s| s.into()).collect(),
            items: std::mem::take(&mut self.items),
        }
    }
}

pub struct InlineSpanBuilder<'a> {
    parent: &'a mut InlineContentBuilder,
    span_index: usize,
    run_index: usize,
    length: usize,
}

impl<'a> InlineSpanBuilder<'a> {
    fn span_mut(&mut self) -> &mut InlineSpan {
        match &mut self.parent.items[self.span_index] {
            InlineItem::Span(span) => span,
            _ => unreachable!(),
        }
    }

    pub fn push_text(&mut self, content: &str) {
        let text_run = &mut self.parent.text_runs[self.run_index];
        let start = text_run.len();
        text_run.push_str(content);
        self.parent.items.push(InlineItem::Text(InlineText {
            content_range: start..text_run.len(),
        }));
        self.length += 1;
    }

    fn push_object_replacement(&mut self) -> usize {
        let run = &mut self.parent.text_runs[self.run_index];
        let index = run.len();
        run.push(OBJECT_REPLACEMENT_CHARACTER);
        index
    }

    fn push_run(&mut self) -> usize {
        let idx = self.parent.text_runs.len();
        self.parent.text_runs.push(String::new());
        idx
    }

    fn push_span_with(
        &mut self,
        style: ComputedStyle,
        kind: InlineSpanKind,
        run_index: usize,
    ) -> InlineSpanBuilder<'_> {
        let span_index = self.parent.items.len();
        self.parent.items.push(InlineItem::Span(InlineSpan {
            style,
            length: 0,
            kind,
        }));
        self.length += 1;

        InlineSpanBuilder {
            parent: self.parent,
            run_index,
            span_index,
            length: 0,
        }
    }

    pub fn push_span(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.push_span_with(style, InlineSpanKind::Span, self.run_index)
    }

    pub fn push_ruby(&mut self, style: ComputedStyle) -> InlineRubyBuilder<'_> {
        let content_index = self.push_object_replacement();
        InlineRubyBuilder(self.push_span_with(
            style,
            InlineSpanKind::Ruby { content_index },
            self.run_index,
        ))
    }
}

impl<'a> Drop for InlineSpanBuilder<'a> {
    fn drop(&mut self) {
        if self.span_index == usize::MAX {
            return;
        }

        self.span_mut().length = self.length;
    }
}

pub struct InlineRubyBuilder<'a>(InlineSpanBuilder<'a>);

impl<'a> InlineRubyBuilder<'a> {
    fn push(&mut self, style: ComputedStyle, annotation: bool) -> InlineSpanBuilder<'_> {
        if self.0.length % 2 != usize::from(annotation) {
            self.0.push_span(ComputedStyle::DEFAULT);
        }

        let run_index = self.0.push_run();

        self.0.push_span_with(
            style.create_derived(),
            InlineSpanKind::RubyInternal {
                run_index,
                outer_style: style,
            },
            run_index,
        )
    }

    pub fn push_base(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.push(style, false)
    }

    pub fn push_annotation(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.push(style, true)
    }
}

#[derive(Debug, Clone)]
pub enum InlineItem {
    Span(InlineSpan),
    Text(InlineText),
}

#[derive(Debug, Clone)]
pub struct InlineSpan {
    style: ComputedStyle,
    length: usize,
    kind: InlineSpanKind,
}

#[derive(Debug, Clone)]
pub enum InlineSpanKind {
    Span,
    // Contents are interleaved base-annotation pairs of kind `RubyInternal`.
    Ruby {
        content_index: usize,
    },
    RubyInternal {
        run_index: usize,
        outer_style: ComputedStyle,
    },
}

#[derive(Debug, Clone)]
pub struct InlineText {
    content_range: Range<usize>,
}

#[derive(Debug)]
pub struct TextFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    // self-referential
    glyphs: text::GlyphString<'static, std::rc::Rc<str>>,
    _font_arena: util::rc::Rc<FontArena>,
    pub baseline_offset: Vec2L,
}

impl TextFragment {
    pub fn glyphs(&self) -> &text::GlyphString<'_, std::rc::Rc<str>> {
        &self.glyphs
    }
}

#[derive(Debug)]
pub struct RubyFragment {
    #[expect(
        dead_code,
        reason = "ruby fragment box is not used for anything nor implemented yet"
    )]
    pub fbox: FragmentBox,
    #[expect(dead_code, reason = "ruby fragment style is not used for anything yet")]
    pub style: ComputedStyle,
    pub content: Vec<(Vec2L, RubyBaseFragment, Vec2L, RubyAnnotationFragment)>,
}

#[derive(Debug)]
pub struct RubyBaseFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub struct RubyAnnotationFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub enum InlineItemFragment {
    Text(TextFragment),
    Ruby(RubyFragment),
}

type OffsetInlineItemFragmentVec = Vec<(Vec2L, util::rc::Rc<InlineItemFragment>)>;

#[derive(Debug, Clone)]
pub struct LineBoxFragment {
    pub fbox: FragmentBox,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug, Clone)]
pub struct InlineContentFragment {
    pub fbox: FragmentBox,
    pub lines: Vec<(Vec2L, util::rc::Rc<LineBoxFragment>)>,
}

impl InlineContentFragment {
    const EMPTY: Self = Self {
        fbox: FragmentBox { size: Vec2L::ZERO },
        lines: Vec::new(),
    };
}

#[derive(Debug, Error)]
pub enum InlineLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::font_db::SelectError),
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error(transparent)]
    FreeType(#[from] text::FreeTypeError),
}

#[derive(Debug)]
struct InitialShapingResult<'a, 'f> {
    shaped: Vec<ShapedItem<'a, 'f>>,
    break_opportunities: Vec<usize>,
    styles: Vec<(usize, &'a ComputedStyle)>,
    bidi: unicode_bidi::BidiInfo<'a>,
}

#[derive(Debug)]
struct ShapedItem<'a, 'f> {
    range: Range<usize>,
    kind: ShapedItemKind<'a, 'f>,
}

#[derive(Debug)]
enum ShapedItemKind<'a, 'f> {
    Text(ShapedItemText<'f>),
    Ruby(ShapedItemRuby<'a, 'f>),
}

#[derive(Debug)]
struct ShapedItemText<'f> {
    font_matcher: FontMatcher<'f>,
    primary_font: &'f text::Font,
    glyphs: GlyphString<'f, Rc<str>>,
    break_after: bool,
}

#[derive(Debug)]
struct ShapedItemRuby<'a, 'f> {
    style: ComputedStyle,
    base_annotation_pairs: Vec<(ShapedRubyBase<'a, 'f>, ShapedRubyAnnotation<'a, 'f>)>,
}

#[derive(Debug)]
struct ShapedRubyBase<'a, 'f> {
    style: &'a ComputedStyle,
    primary_font: &'f text::Font,
    inner: InitialShapingResult<'a, 'f>,
}

#[derive(Debug)]
struct ShapedRubyAnnotation<'a, 'f> {
    style: &'a ComputedStyle,
    inner: InitialShapingResult<'a, 'f>,
}

fn font_matcher_from_style<'f>(
    style: &ComputedStyle,
    font_arena: &'f FontArena,
    lctx: &mut LayoutContext,
) -> Result<FontMatcher<'f>, InlineLayoutError> {
    text::FontMatcher::match_all(
        style.font_family(),
        text::FontStyle {
            weight: style.font_weight(),
            italic: match style.font_slant() {
                FontSlant::Regular => false,
                FontSlant::Italic => true,
            },
        },
        style.font_size(),
        lctx.dpi,
        font_arena,
        lctx.fonts,
    )
    .map_err(Into::into)
}

fn shape_run_initial<'a, 'f>(
    content: &'a InlineContent,
    run_index: usize,
    item_index: usize,
    end_item_index: &mut usize,
    lctx: &mut LayoutContext,
    font_arena: &'f FontArena,
    compute_break_opportunities: bool,
) -> Result<InitialShapingResult<'a, 'f>, InlineLayoutError> {
    struct QueuedText<'f> {
        matcher: FontMatcher<'f>,
        range: Range<usize>,
    }

    impl<'f> QueuedText<'f> {
        fn flush(
            self,
            text: Rc<str>,
            bidi: &unicode_bidi::BidiInfo,
            font_arena: &'f FontArena,
            lctx: &mut LayoutContext,
            result: &mut Vec<ShapedItem<'_, 'f>>,
        ) -> Result<(), InlineLayoutError> {
            let mut current_paragraph = match bidi
                .paragraphs
                .binary_search_by_key(&self.range.start, |p| p.range.start)
            {
                Ok(i) => i,
                Err(i) => i - 1,
            };

            let mut push = |level: unicode_bidi::Level,
                            range: Range<usize>,
                            break_after: bool|
             -> Result<(), InlineLayoutError> {
                let direction = if level.is_ltr() {
                    Direction::Ltr
                } else {
                    Direction::Rtl
                };

                let glyphs = {
                    let mut buffer = text::ShapingBuffer::new();
                    buffer.reset();
                    buffer.guess_properties();
                    buffer.set_direction(direction.to_horizontal());
                    buffer.add(&text, range.clone());
                    buffer.shape(self.matcher.iterator(), font_arena, lctx.fonts)?
                };

                result.push(ShapedItem {
                    range,
                    kind: ShapedItemKind::Text(ShapedItemText {
                        font_matcher: self.matcher.clone(),
                        primary_font: self.matcher.primary(font_arena, lctx.fonts)?,
                        glyphs: GlyphString::from_glyphs(text.clone(), glyphs, direction),
                        break_after,
                    }),
                });

                Ok(())
            };

            let mut current_level = bidi.levels[self.range.start];
            let mut last = self.range.start;
            let mut was_newline = false;
            for (i, &level) in self.range.clone().zip(&bidi.levels[self.range.clone()]) {
                let paragraph_ended = bidi.paragraphs[current_paragraph].range.end == i;
                let level_changed_or_break = current_level != level || was_newline;
                if paragraph_ended || level_changed_or_break {
                    push(
                        current_level,
                        last..i - usize::from(was_newline),
                        was_newline,
                    )?;
                    last = i;
                    current_paragraph += usize::from(paragraph_ended);
                }
                current_level = level;
                was_newline = text.as_bytes()[i] == b'\n';
            }

            push(
                current_level,
                last..self.range.end - usize::from(was_newline),
                was_newline,
            )
        }
    }

    struct ShapedItemBuilder<'a, 'f, 'l, 'll, 'la> {
        content: &'a InlineContent,
        run_text: &'a Rc<str>,
        bidi: unicode_bidi::BidiInfo<'a>,
        grapheme_cluster_boundaries: Vec<usize>,
        lctx: &'l mut LayoutContext<'ll, 'la>,
        font_arena: &'f FontArena,

        break_opportunities: Vec<usize>,
        shaped: Vec<ShapedItem<'a, 'f>>,
        queued_text: Option<QueuedText<'f>>,
    }

    impl<'a, 'f> ShapedItemBuilder<'a, 'f, '_, '_, '_> {
        fn push_break_opportunity(&mut self, idx: usize) {
            if let Some(&previous) = self.break_opportunities.last() {
                if previous == idx {
                    return;
                }

                debug_assert!(previous < idx);
            }

            self.break_opportunities.push(idx);
        }

        fn compute_text_break_opportunities(&mut self, range: Range<usize>, style: &ComputedStyle) {
            // FIXME: This makes sense conceptually but may fall apart in the presence of
            //        dictionary line segmenters. Some testing has to be done to make sure
            //        this produces correct results.
            //        (for now we can also just not care since I can't even get Firefox
            //         or Chromium to do dictionary based breaking...)
            let padded_start_grapheme_index =
                match self.grapheme_cluster_boundaries.binary_search(&range.start) {
                    Ok(found) => found.saturating_sub(1),
                    Err(left) => left - 1,
                };
            let padded_start = self.grapheme_cluster_boundaries[padded_start_grapheme_index];
            let padded_end = self
                .grapheme_cluster_boundaries
                .get(
                    match self.grapheme_cluster_boundaries[padded_start_grapheme_index..]
                        .binary_search(&range.end)
                    {
                        Ok(found) => padded_start_grapheme_index + found + 1,
                        Err(left) => padded_start_grapheme_index + left + 1,
                    },
                )
                .copied()
                .unwrap_or(self.run_text.len());

            let segmenter = icu_segmenter::LineSegmenter::new_auto({
                let mut options = LineBreakOptions::default();
                options.strictness = Some(style.line_break());
                options.word_option = Some(style.word_break());
                options
            });

            let ignore_after = range.end.min(self.run_text.len() - 1);
            let mut iter = segmenter
                .segment_str(&self.run_text[padded_start..padded_end])
                .map(|idx| idx + padded_start);
            // The first break is going to be either at the start of the string or
            // before our "padding" look-behind character, both of which we want to ignore.
            _ = iter.next();
            for idx in iter {
                if idx > ignore_after {
                    break;
                }

                self.push_break_opportunity(idx);
            }
        }

        fn process_items(
            mut self,
            item_index: usize,
            end_item_index: &mut usize,
            compute_break_opportunities: bool,
        ) -> Result<InitialShapingResult<'a, 'f>, InlineLayoutError> {
            let items = &self.content.items;
            let mut current_item = item_index;
            let mut current_style = const { &ComputedStyle::DEFAULT };
            let mut span_left = usize::MAX;
            let mut span_stack: Vec<(&ComputedStyle, usize)> = Vec::new();
            let mut styles = vec![];

            while let Some(item) = items
                .get(current_item)
                .filter(|_| !span_stack.is_empty() || current_item < *end_item_index)
            {
                span_left -= 1;
                current_item += 1;
                match item {
                    InlineItem::Span(span) => match span.kind {
                        InlineSpanKind::Span | InlineSpanKind::RubyInternal { .. } => {
                            span_stack.push((current_style, span_left));
                            current_style = &span.style;
                            span_left = span.length;
                        }
                        InlineSpanKind::Ruby { content_index } => {
                            if let Some(queued) = self.queued_text.take() {
                                queued.flush(
                                    self.run_text.clone(),
                                    &self.bidi,
                                    self.font_arena,
                                    self.lctx,
                                    &mut self.shaped,
                                )?;
                            }

                            self.shaped.push(ShapedItem {
                                range: content_index..content_index + 1,
                                kind: ShapedItemKind::Ruby(ShapedItemRuby {
                                    style: span.style.clone(),
                                    base_annotation_pairs: {
                                        let mut result = Vec::new();

                                        let mut remaining = span.length;
                                        while remaining > 0 {
                                            let &InlineItem::Span(InlineSpan {
                                                kind:
                                                    InlineSpanKind::RubyInternal {
                                                        run_index,
                                                        outer_style: ref base_style,
                                                    },
                                                ..
                                            }) = &items[current_item]
                                            else {
                                                unreachable!("Illegal ruby base item");
                                            };

                                            let base = ShapedRubyBase {
                                                style: base_style,
                                                primary_font: font_matcher_from_style(
                                                    base_style,
                                                    self.font_arena,
                                                    self.lctx,
                                                )?
                                                .primary(self.font_arena, self.lctx.fonts)?,
                                                inner: shape_run_initial(
                                                    self.content,
                                                    run_index,
                                                    current_item,
                                                    {
                                                        current_item += 1;
                                                        &mut current_item
                                                    },
                                                    self.lctx,
                                                    self.font_arena,
                                                    false,
                                                )?,
                                            };
                                            remaining -= 1;
                                            let annotation = if remaining > 0 {
                                                let &InlineItem::Span(InlineSpan {
                                                    kind:
                                                        InlineSpanKind::RubyInternal {
                                                            run_index,
                                                            outer_style: ref annotation_style,
                                                        },
                                                    ..
                                                }) = &items[current_item]
                                                else {
                                                    unreachable!("Illegal ruby annotation item");
                                                };

                                                let result = shape_run_initial(
                                                    self.content,
                                                    run_index,
                                                    current_item,
                                                    {
                                                        current_item += 1;
                                                        &mut current_item
                                                    },
                                                    self.lctx,
                                                    self.font_arena,
                                                    false,
                                                )?;
                                                remaining -= 1;
                                                ShapedRubyAnnotation {
                                                    style: annotation_style,
                                                    inner: result,
                                                }
                                            } else {
                                                ShapedRubyAnnotation {
                                                    style: const { &ComputedStyle::DEFAULT },
                                                    inner: InitialShapingResult {
                                                        shaped: Vec::new(),
                                                        break_opportunities: Vec::new(),
                                                        styles: Vec::new(),
                                                        bidi: unicode_bidi::BidiInfo::new("", None),
                                                    },
                                                }
                                            };

                                            result.push((base, annotation));
                                        }

                                        result
                                    },
                                }),
                            });

                            if compute_break_opportunities {
                                if content_index != 0 {
                                    self.push_break_opportunity(content_index);
                                }
                                if content_index + 1 != self.run_text.len() {
                                    self.push_break_opportunity(content_index + 1);
                                }
                            }
                        }
                    },
                    InlineItem::Text(text) => {
                        let font_matcher =
                            font_matcher_from_style(current_style, self.font_arena, self.lctx)?;

                        match self.queued_text {
                            Some(ref mut queued)
                                if queued.matcher == font_matcher
                                    && queued.range.end == text.content_range.start =>
                            {
                                queued.range.end = text.content_range.end
                            }
                            Some(queued) => {
                                queued.flush(
                                    self.run_text.clone(),
                                    &self.bidi,
                                    self.font_arena,
                                    self.lctx,
                                    &mut self.shaped,
                                )?;
                                self.queued_text = Some(QueuedText {
                                    matcher: font_matcher,
                                    range: text.content_range.clone(),
                                });
                            }
                            None => {
                                self.queued_text = Some(QueuedText {
                                    matcher: font_matcher,
                                    range: text.content_range.clone(),
                                })
                            }
                        }

                        styles.push((text.content_range.start, current_style));
                        if compute_break_opportunities {
                            self.compute_text_break_opportunities(
                                text.content_range.clone(),
                                current_style,
                            );
                        }
                    }
                }

                while span_left == 0 {
                    let popped = span_stack.pop().unwrap();
                    current_style = popped.0;
                    span_left = popped.1;
                }
            }
            *end_item_index = current_item;

            if let Some(queued) = self.queued_text {
                queued.flush(
                    self.run_text.clone(),
                    &self.bidi,
                    self.font_arena,
                    self.lctx,
                    &mut self.shaped,
                )?;
            }

            debug_assert!(if !compute_break_opportunities {
                self.break_opportunities.is_empty()
            } else {
                true
            });

            Ok(InitialShapingResult {
                shaped: self.shaped,
                break_opportunities: self.break_opportunities,
                styles,
                bidi: self.bidi,
            })
        }
    }

    let run_text = &content.text_runs[run_index];
    ShapedItemBuilder {
        content,
        run_text,
        bidi: unicode_bidi::BidiInfo::new(run_text, None),
        grapheme_cluster_boundaries: {
            let mut result: Vec<usize> = GraphemeClusterSegmenter::new()
                .segment_str(run_text)
                .collect();
            // The segmenter always inserts `text.len()` as a grapheme cluster boundary
            // but we want this list to only include the start indices of graphemes.
            result.pop();
            result
        },
        lctx,
        font_arena,

        break_opportunities: Vec::new(),
        queued_text: None,
        shaped: Vec::new(),
    }
    .process_items(item_index, end_item_index, compute_break_opportunities)
}

struct BreakingContext<'f, 'l, 'a, 'b> {
    layout: &'a mut LayoutContext<'l, 'b>,
    constraints: &'a LayoutConstraints,
    font_arena: &'f FontArena,
    break_opportunities: &'a [usize],
    break_buffer: text::ShapingBuffer,
}

enum BreakOutcome<'a, 'f> {
    BreakSplit(ShapedItem<'a, 'f>),
    BreakAfter,
    None,
}

impl<'a, 'f> ShapedItem<'a, 'f> {
    fn line_break(
        &mut self,
        current_width: &mut FixedL,
        ctx: &mut BreakingContext<'f, '_, '_, '_>,
    ) -> Result<BreakOutcome<'a, 'f>, InlineLayoutError> {
        match &mut self.kind {
            ShapedItemKind::Text(text) => text.line_break(&mut self.range, current_width, ctx),
            ShapedItemKind::Ruby(_) => {
                // TODO: Implement ruby line breaking
                //       It should only allow breaking between distinct base-annotation pairs.
                shaped_item_width(current_width, self);
                Ok(BreakOutcome::None)
            }
        }
    }

    fn forces_line_break_after(&self) -> bool {
        match &self.kind {
            ShapedItemKind::Text(text) => text.break_after,
            ShapedItemKind::Ruby(_) => false,
        }
    }
}

impl<'f> ShapedItemText<'f> {
    fn line_break<'a>(
        &mut self,
        range: &mut Range<usize>,
        current_width: &mut FixedL,
        ctx: &mut BreakingContext<'f, '_, '_, '_>,
    ) -> Result<BreakOutcome<'a, 'f>, InlineLayoutError> {
        let mut glyph_it = self.glyphs.iter_glyphs();
        while let Some(glyph) = glyph_it.next() {
            *current_width += glyph.x_advance;
            if *current_width > ctx.constraints.size.x {
                let opportunities = &ctx.break_opportunities[..=match ctx
                    .break_opportunities
                    .binary_search(&glyph.cluster)
                {
                    Ok(idx) => idx,
                    Err(idx) => idx.saturating_sub(1),
                }];

                // TODO: Also try slightly overflowing break points if these fail
                for &opportunity in opportunities
                    .iter()
                    .rev()
                    .take(3)
                    .take_while(|&&i| i > range.start)
                {
                    if opportunity == range.end {
                        return Ok(BreakOutcome::BreakAfter);
                    }

                    ctx.break_buffer.set_direction(self.glyphs.direction());
                    if let Some((broken, remaining)) = self.glyphs.break_at_if_less_or_eq(
                        opportunity,
                        ctx.constraints.size.x,
                        &mut ctx.break_buffer,
                        self.font_matcher.iterator(),
                        ctx.font_arena,
                        ctx.layout.fonts,
                    )? {
                        drop(glyph_it);

                        let previous_end = range.end;
                        range.end = opportunity;
                        self.glyphs = broken;

                        return Ok(BreakOutcome::BreakSplit(ShapedItem {
                            range: opportunity..previous_end,
                            kind: ShapedItemKind::Text(ShapedItemText {
                                font_matcher: self.font_matcher.clone(),
                                primary_font: self.primary_font,
                                glyphs: remaining,
                                break_after: self.break_after,
                            }),
                        }));
                    }
                }
            }
        }
        drop(glyph_it);

        if self.break_after {
            return Ok(BreakOutcome::BreakAfter);
        }

        Ok(BreakOutcome::None)
    }
}

fn shaped_item_width(result: &mut FixedL, item: &ShapedItem) {
    match &item.kind {
        ShapedItemKind::Text(text) => {
            for glyph in text.glyphs.iter_glyphs() {
                *result += glyph.x_advance;
            }
        }
        ShapedItemKind::Ruby(ruby) => {
            for (base, annotation) in &ruby.base_annotation_pairs {
                let mut base_width = FixedL::ZERO;
                let mut annotation_width = FixedL::ZERO;

                for item in &base.inner.shaped {
                    shaped_item_width(&mut base_width, item);
                }
                for item in &annotation.inner.shaped {
                    shaped_item_width(&mut annotation_width, item);
                }

                *result += base_width.max(annotation_width);
            }
        }
    }
}

fn layout_run_full(
    content: &InlineContent,
    run_index: usize,
    item_index: usize,
    end_item_index: &mut usize,
    align: HorizontalAlignment,
    lctx: &mut LayoutContext,
    constraints: &LayoutConstraints,
) -> Result<InlineContentFragment, InlineLayoutError> {
    fn split_on_style<'s, 'f>(
        range: Range<usize>,
        shaped: &ShapedItemText<'f>,
        styles: &[(usize, &'s ComputedStyle)],
        mut push_section: impl FnMut(&'s ComputedStyle, GlyphString<'f, Rc<str>>),
    ) -> Result<(), InlineLayoutError> {
        let mut glyphs = shaped.glyphs.clone();

        // TODO: Can this code be deduplicated? It seems kinda hard to do so
        //       Maybe the LTR loop could be simplified though...
        //       kind of accidentally made this RTL optimised...
        if !shaped.glyphs.direction().is_reverse() {
            let mut si = match styles.binary_search_by_key(&range.start, |&(start, _)| start) {
                Ok(s) => s,
                Err(s) => s - 1,
            };

            if styles
                .get(si + 1)
                .is_none_or(|&(start, _)| start >= range.end)
            {
                push_section(styles[si].1, glyphs);
                return Ok(());
            }

            let mut i = range.start;
            while i != range.end {
                let end = styles
                    .get(si + 1)
                    .map(|&(next_start, _)| next_start.min(range.end))
                    .unwrap_or(range.end);
                let style = styles[si].1;

                if let Some(section_glyphs) = glyphs.split_off_until_cluster(end) {
                    push_section(style, section_glyphs);
                }

                i = end;
                si += 1;
            }
        } else {
            let mut si = match styles.binary_search_by_key(&range.end, |&(start, _)| start) {
                Ok(s) => s - 1,
                Err(s) => s - 1,
            };

            if styles[si].0 <= range.start {
                push_section(styles[si].1, glyphs);
                return Ok(());
            }

            let mut i = range.end;
            while i != range.start {
                let (start, style) = styles[si];

                if let Some(section_glyphs) = glyphs.split_off_until_cluster(start) {
                    push_section(style, section_glyphs);
                }

                i = start;
                si -= 1;
            }
        };

        Ok(())
    }

    fn reorder(
        shaped: &[ShapedItem],
        bidi: &unicode_bidi::BidiInfo,
        mut push_item: impl FnMut(&ShapedItem) -> Result<(), InlineLayoutError>,
    ) -> Result<(), InlineLayoutError> {
        let line_range = {
            if let (Some(first), Some(last)) = (shaped.first(), shaped.last()) {
                first.range.start..last.range.end
            } else {
                // There's nothing to reorder, at most we'll run into indexing errors if
                // somehow there are paragraphs but not runs so we must bail here.
                return Ok(());
            }
        };

        if bidi.levels[line_range.clone()]
            .iter()
            .all(|level| level.is_ltr())
        {
            // The whole line only consists of LTR levels, hence no bidirectional reodering is
            // needed and we can skip all of this mess.
            for item in shaped {
                push_item(item)?;
            }

            return Ok(());
        }

        let mut visual_runs = Vec::new();
        for paragraph in &bidi.paragraphs {
            if line_range.start <= paragraph.range.start || line_range.end >= paragraph.range.end {
                let (_, mut paragraph_runs) = bidi.visual_runs(
                    paragraph,
                    line_range.start.max(paragraph.range.start)
                        ..line_range.end.min(paragraph.range.end),
                );

                visual_runs.append(&mut paragraph_runs);
            }
        }

        for range in visual_runs {
            let mut push_item_in_range = |item: &ShapedItem| -> Result<(), InlineLayoutError> {
                if range.start <= item.range.start && range.end >= item.range.end {
                    push_item(item)
                } else if let ShapedItemKind::Text(text) = &item.kind {
                    assert!(
                        (range.start > item.range.start) ^ (range.end < item.range.end),
                        "bidi reordering attempted to partially reorder a text item on both sides"
                    );

                    // Cursed code path™
                    // I doubt even god knows whether this works in all cases,
                    // it works in at least one though.
                    // HACK: This can only happen due to bidi rule L1 which may split a line's
                    // trailing whitespace into a separate level run.
                    // Since this may only occur with whitespaces we cheat a little bit here and
                    // just completely unsafely split glyph strings assuming no reshaping is
                    // necessary. Reshaping here would be a bad idea anyway and doesn't make sense.
                    let mut tmp = ShapedItemText {
                        font_matcher: text.font_matcher.clone(),
                        primary_font: text.primary_font,
                        glyphs: text.glyphs.clone(),
                        break_after: false,
                    };
                    if range.start > item.range.start {
                        tmp.glyphs.split_off_until_cluster(range.start);
                        push_item(&ShapedItem {
                            range: range.start..item.range.start,
                            kind: ShapedItemKind::Text(tmp),
                        })
                    } else {
                        debug_assert!(range.end < item.range.end);
                        if let Some(before) = tmp.glyphs.split_off_until_cluster(range.end) {
                            tmp.glyphs = before;
                            push_item(&ShapedItem {
                                range: item.range.start..range.end,
                                kind: ShapedItemKind::Text(tmp),
                            })
                        } else {
                            Ok(())
                        }
                    }
                } else {
                    unreachable!("bidi reordering attempted to partially reorder a non-text item");
                }
            };

            let level = bidi.levels[range.start];
            if level.is_ltr() {
                let start = match shaped.binary_search_by_key(&range.start, |r| r.range.start) {
                    Ok(i) => i,
                    Err(i) => i - 1,
                };

                for item in &shaped[start..] {
                    if item.range.start >= range.end {
                        break;
                    }

                    push_item_in_range(item)?;
                }
            } else {
                let end = match shaped.binary_search_by_key(&range.end, |r| r.range.end) {
                    Ok(i) => i + 1,
                    Err(i) => i,
                };

                for item in shaped[..end].iter().rev() {
                    if item.range.end <= range.start {
                        break;
                    }

                    push_item_in_range(item)?;
                }
            }
        }

        Ok(())
    }

    #[derive(Debug)]
    struct FragmentBuilder<'t> {
        result: InlineContentFragment,
        current_y: FixedL,
        line_align: HorizontalAlignment,
        bidi: unicode_bidi::BidiInfo<'t>,
        styles: Vec<(usize, &'t ComputedStyle)>,
    }

    #[derive(Debug)]
    struct InlineItemFragmentBuilder<'a> {
        output: &'a mut OffsetInlineItemFragmentVec,
        line_ascender: FixedL,
        current_x: FixedL,
    }

    #[derive(Debug, Clone, Copy)]
    struct LineHeightMetrics {
        max_ascender: FixedL,
        max_ruby_overflow_ascender: FixedL,
        min_descender: FixedL,
    }

    #[derive(Debug, Clone, Copy)]
    enum LineHeight {
        Normal,
        Value(FixedL),
    }

    impl LineHeight {
        const RUBY_ANNOTATION: LineHeight = LineHeight::Value(FixedL::ONE);
    }

    impl LineHeightMetrics {
        const ZERO: Self = LineHeightMetrics {
            max_ascender: FixedL::ZERO,
            max_ruby_overflow_ascender: FixedL::ZERO,
            min_descender: FixedL::ZERO,
        };

        fn height(&self) -> FixedL {
            self.max_ascender - self.min_descender
        }

        fn expand_to(&mut self, ascender: FixedL, descender: FixedL) {
            self.max_ascender = self.max_ascender.max(ascender);
            self.min_descender = self.min_descender.min(descender);
        }

        // https://drafts.csswg.org/css-inline/#inline-height
        fn process_item(&mut self, item: &ShapedItem, line_height: LineHeight) {
            match &item.kind {
                ShapedItemKind::Text(text) => match line_height {
                    LineHeight::Normal => {
                        let primary_metrics = text.primary_font.metrics();
                        let half_leading = (primary_metrics.line_gap() / 2).max(FixedL::ZERO);

                        self.expand_to(
                            primary_metrics.ascender + half_leading,
                            primary_metrics.descender - half_leading,
                        );

                        for glyph in text.glyphs.iter_glyphs() {
                            self.expand_to(
                                glyph.font.metrics().ascender + half_leading,
                                glyph.font.metrics().descender - half_leading,
                            );
                        }
                    }
                    LineHeight::Value(value) => {
                        let computed_font_size = text.font_matcher.size() * 96 / 72;
                        let metrics = text.primary_font.metrics();
                        let half_leading = ((computed_font_size * value)
                            - (metrics.ascender - metrics.descender))
                            / 2;
                        self.expand_to(
                            metrics.ascender + half_leading,
                            metrics.descender - half_leading,
                        );
                    }
                },
                ShapedItemKind::Ruby(ruby) => {
                    for (base, annotation) in &ruby.base_annotation_pairs {
                        let mut base_metrics = LineHeightMetrics::ZERO;
                        let mut annotation_metrics = LineHeightMetrics::ZERO;

                        for item in &base.inner.shaped {
                            base_metrics.process_item(item, line_height);
                        }
                        for item in &annotation.inner.shaped {
                            annotation_metrics.process_item(item, LineHeight::RUBY_ANNOTATION);
                        }

                        self.max_ruby_overflow_ascender = self
                            .max_ruby_overflow_ascender
                            .max(base_metrics.max_ascender + annotation_metrics.max_ascender);
                        self.expand_to(base_metrics.max_ascender, base_metrics.min_descender);
                    }
                }
            }
        }
    }

    impl InlineItemFragmentBuilder<'_> {
        // These functions are unsafe because there is a "this FontArena must hold all fonts used by
        // the input items" invarant.
        unsafe fn reorder_and_append(
            &mut self,
            shaped: &[ShapedItem],
            font_arena: util::rc::Rc<FontArena>,
            bidi: &unicode_bidi::BidiInfo,
            styles: &[(usize, &ComputedStyle)],
        ) -> Result<(), InlineLayoutError> {
            reorder(shaped, bidi, |item| match &item.kind {
                ShapedItemKind::Text(text) => {
                    split_on_style(item.range.clone(), text, styles, |style, glyphs| {
                        let font_metrics = text.primary_font.metrics();
                        // https://drafts.csswg.org/css-inline/#valdef-inline-sizing-normal
                        let logical_height = font_metrics.ascender - font_metrics.descender;
                        let logical_width = glyphs.iter_glyphs().map(|g| g.x_advance).sum();
                        let fragment = TextFragment {
                            fbox: FragmentBox {
                                size: Vec2L::new(logical_width, logical_height),
                            },
                            style: style.clone(),
                            glyphs: unsafe {
                                std::mem::transmute::<GlyphString<'_, _>, GlyphString<'static, _>>(
                                    glyphs,
                                )
                            },
                            _font_arena: font_arena.clone(),
                            baseline_offset: Vec2::new(FixedL::ZERO, font_metrics.ascender),
                        };

                        let item_width = fragment.fbox.size.x;
                        self.output.push((
                            Vec2L::new(self.current_x, self.line_ascender - font_metrics.ascender),
                            InlineItemFragment::Text(fragment).into(),
                        ));
                        self.current_x += item_width;
                    })
                }
                ShapedItemKind::Ruby(ruby) => {
                    let mut result = RubyFragment {
                        // TODO: What box should a ruby container fragment have?
                        //       For now we'll just leave it zero-sized.
                        fbox: FragmentBox {
                            size: Vec2::new(FixedL::ZERO, FixedL::ZERO),
                        },
                        style: ruby.style.clone(),
                        content: Vec::new(),
                    };

                    let mut ruby_current_x = FixedL::ZERO;
                    for (base, annotation) in &ruby.base_annotation_pairs {
                        let mut base_width = FixedL::ZERO;
                        let mut annotation_width = FixedL::ZERO;
                        let mut annotation_metrics = LineHeightMetrics::ZERO;
                        for item in &base.inner.shaped {
                            shaped_item_width(&mut base_width, item);
                        }
                        for item in &annotation.inner.shaped {
                            shaped_item_width(&mut annotation_width, item);
                            annotation_metrics.process_item(item, LineHeight::RUBY_ANNOTATION);
                        }

                        let base_font_metrics = base.primary_font.metrics();
                        let base_height = base_font_metrics.ascender - base_font_metrics.descender;
                        let annotation_height = annotation_metrics.height();
                        let signed_half_padding = (annotation_width - base_width) / 2;
                        let base_half_padding = signed_half_padding.max(FixedL::ZERO);
                        let annotation_half_padding = (-signed_half_padding).max(FixedL::ZERO);
                        let ruby_width = base_width.max(annotation_width);

                        let base_offset = Vec2::new(
                            ruby_current_x,
                            self.line_ascender - base_font_metrics.ascender,
                        );
                        // FIXME: Apparently ruby internal boxes are not supposed to use
                        //        inline-sizing sizing. Now this makes sense with the ruby
                        //        annotation box because it creates/is a new line box and
                        //        should logically obey line box sizing rules.
                        //        However I'm not certain what this means for ruby base
                        //        boxes? Should they just fit their contents?
                        let mut base_fragment = RubyBaseFragment {
                            fbox: FragmentBox {
                                size: Vec2::new(ruby_width, base_height),
                            },
                            style: base.style.clone(),
                            children: Vec::new(),
                        };
                        InlineItemFragmentBuilder {
                            output: &mut base_fragment.children,
                            line_ascender: base_font_metrics.ascender,
                            current_x: base_half_padding,
                        }
                        .reorder_and_append(
                            &base.inner.shaped,
                            font_arena.clone(),
                            &base.inner.bidi,
                            &base.inner.styles,
                        )?;

                        let mut annotation_fragment = RubyAnnotationFragment {
                            fbox: FragmentBox {
                                size: Vec2::new(ruby_width, annotation_height),
                            },
                            style: annotation.style.clone(),
                            children: Vec::new(),
                        };
                        InlineItemFragmentBuilder {
                            output: &mut annotation_fragment.children,
                            line_ascender: annotation_metrics.max_ascender,
                            current_x: annotation_half_padding,
                        }
                        .reorder_and_append(
                            &annotation.inner.shaped,
                            font_arena.clone(),
                            &annotation.inner.bidi,
                            &annotation.inner.styles,
                        )?;

                        let annotation_offset = Vec2::new(
                            ruby_current_x,
                            -annotation_metrics.max_ascender - annotation_metrics.min_descender,
                        );

                        ruby_current_x += base_fragment.fbox.size.x;
                        result.content.push((
                            base_offset,
                            base_fragment,
                            annotation_offset,
                            annotation_fragment,
                        ));
                    }

                    self.output.push((
                        Vec2::new(self.current_x, FixedL::ZERO),
                        InlineItemFragment::Ruby(result).into(),
                    ));
                    self.current_x += ruby_current_x;

                    Ok(())
                }
            })?;

            Ok(())
        }
    }

    impl FragmentBuilder<'_> {
        unsafe fn push_line(
            &mut self,
            shaped: &mut [ShapedItem],
            font_arena: util::rc::Rc<FontArena>,
        ) -> Result<(), InlineLayoutError> {
            let mut line_width = FixedL::ZERO;
            let mut line_metrics = LineHeightMetrics::ZERO;
            for item in &*shaped {
                shaped_item_width(&mut line_width, item);
                line_metrics.process_item(item, LineHeight::Normal);
            }

            let line_height = line_metrics.height();
            let mut line_box = LineBoxFragment {
                fbox: FragmentBox {
                    size: { Vec2::new(line_width, line_height) },
                },
                children: Vec::new(),
            };

            {
                InlineItemFragmentBuilder {
                    output: &mut line_box.children,
                    line_ascender: line_metrics.max_ascender,
                    current_x: FixedL::ZERO,
                }
                .reorder_and_append(
                    shaped,
                    font_arena,
                    &self.bidi,
                    &self.styles,
                )?;
            }

            let aligning_x_offset = match self.line_align {
                HorizontalAlignment::Left => I26Dot6::ZERO,
                HorizontalAlignment::Center => -line_width / 2,
                HorizontalAlignment::Right => -line_width,
            };

            // HACK: I don't know why this works... but this appears to be somewhat close to Chromium.
            let ruby_leading = line_metrics
                .max_ruby_overflow_ascender
                .max(line_metrics.max_ascender)
                - line_metrics.max_ascender;
            let ruby_half_leading = ruby_leading / 2;

            self.result.fbox.size.x = self.result.fbox.size.x.max(line_box.fbox.size.x);
            self.current_y += ruby_half_leading;
            self.result.lines.push((
                Vec2L::new(aligning_x_offset, self.current_y),
                line_box.into(),
            ));
            self.current_y += line_height;
            self.result.fbox.size.y = self.current_y;

            Ok(())
        }

        fn finish(self) -> InlineContentFragment {
            let mut fragment = self.result;

            let mut min = FixedL::ZERO;
            for (offset, _) in &fragment.lines {
                min = min.min(offset.x);
            }
            for (offset, _) in &mut fragment.lines {
                offset.x -= min;
            }

            fragment
        }
    }

    let font_arena = util::rc::Rc::new(FontArena::new());

    let InitialShapingResult {
        mut shaped,
        break_opportunities,
        styles,
        bidi,
    } = shape_run_initial(
        content,
        run_index,
        item_index,
        end_item_index,
        lctx,
        &font_arena,
        true,
    )?;

    let mut builder = FragmentBuilder {
        current_y: FixedL::ZERO,
        result: InlineContentFragment::EMPTY,
        line_align: align,
        bidi,
        styles,
    };

    if constraints.size.x != FixedL::MAX && !break_opportunities.is_empty() {
        let mut breaking_context = BreakingContext {
            layout: lctx,
            constraints,
            font_arena: &font_arena,
            break_opportunities: &break_opportunities,
            break_buffer: text::ShapingBuffer::new(),
        };

        'break_loop: loop {
            let mut current_width = FixedL::ZERO;
            'item_loop: for i in 0..shaped.len() {
                let item = &mut shaped[i];
                let remaining = match item.line_break(&mut current_width, &mut breaking_context)? {
                    BreakOutcome::BreakSplit(item) => Some(item),
                    BreakOutcome::BreakAfter => None,
                    BreakOutcome::None => continue 'item_loop,
                };

                unsafe { builder.push_line(&mut shaped[..=i], font_arena.clone())? };

                if let Some(remaining) = remaining {
                    shaped.drain(..i);
                    *shaped.first_mut().unwrap() = remaining;
                } else {
                    shaped.drain(..i + 1);
                }

                continue 'break_loop;
            }

            if !shaped.is_empty() {
                unsafe { builder.push_line(&mut shaped, font_arena.clone())? };
            }
            break;
        }
    } else {
        'break_loop: for i in 0..shaped.len() {
            if shaped[i].forces_line_break_after() {
                unsafe { builder.push_line(&mut shaped[..=i], font_arena.clone())? };
                shaped.drain(..=i);
                continue 'break_loop;
            }
        }

        if !shaped.is_empty() {
            unsafe { builder.push_line(&mut shaped, font_arena.clone())? };
        }
    }

    Ok(builder.finish())
}

pub fn layout<'l, 'a, 'b, 'c>(
    lctx: &'b mut LayoutContext<'l, 'a>,
    content: &'c InlineContent,
    constraints: &LayoutConstraints,
    align: HorizontalAlignment,
) -> Result<InlineContentFragment, InlineLayoutError> {
    layout_run_full(
        content,
        0,
        0,
        &mut content.items.len(),
        align,
        lctx,
        constraints,
    )
}
