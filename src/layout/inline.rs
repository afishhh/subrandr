use std::{ops::Range, rc::Rc};

use icu_segmenter::{options::LineBreakOptions, GraphemeClusterSegmenter};
use thiserror::Error;
use util::math::{I26Dot6, Vec2};

use super::{FixedL, FragmentBox, LayoutConstraints, LayoutContext, Vec2L};
use crate::{
    layout::BoxFragmentationPart,
    style::{
        computed::{self, FontSlant, HorizontalAlignment, InlineSizing, WhiteSpaceCollapse},
        ComputedStyle,
    },
    text::{
        self, Direction, Font, FontArena, FontMatcher, FontMetrics, OpenTypeTag, ShapingBuffer,
    },
};

mod glyph_string;
pub use glyph_string::*;

// This character is used to represent opaque objects nested inside inline text content,
// this includes ruby containers and `inline-block`s.
const OBJECT_REPLACEMENT_CHARACTER: char = '\u{FFFC}';
const OBJECT_REPLACEMENT_LENGTH: usize = OBJECT_REPLACEMENT_CHARACTER.len_utf8();

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
    root_style: ComputedStyle,
}

impl Default for InlineContent {
    fn default() -> Self {
        Self {
            text_runs: vec![Rc::from("")],
            items: Vec::new(),
            root_style: ComputedStyle::DEFAULT,
        }
    }
}

pub struct InlineContentBuilder {
    text_runs: Vec<String>,
    items: Vec<InlineItem>,
    root_style: ComputedStyle,
}

impl InlineContentBuilder {
    pub fn new(root_style: ComputedStyle) -> Self {
        Self {
            text_runs: Vec::new(),
            items: Vec::new(),
            root_style,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn root(&mut self) -> InlineSpanBuilder<'_> {
        if self.text_runs.is_empty() {
            self.text_runs.push(String::new());
        }

        InlineSpanBuilder {
            parent: self,
            run_index: 0,
            span_index: usize::MAX,
            last_item_is_text: false,
            length: 0,
        }
    }

    pub fn finish(&mut self) -> InlineContent {
        InlineContent {
            text_runs: self.text_runs.drain(..).map(|s| s.into()).collect(),
            items: std::mem::take(&mut self.items),
            root_style: std::mem::take(&mut self.root_style),
        }
    }
}

pub struct InlineSpanBuilder<'a> {
    parent: &'a mut InlineContentBuilder,
    span_index: usize,
    run_index: usize,
    length: usize,
    last_item_is_text: bool,
}

impl<'a> InlineSpanBuilder<'a> {
    fn span_mut(&mut self) -> &mut InlineSpan {
        match &mut self.parent.items[self.span_index] {
            InlineItem::Span(span) => span,
            _ => unreachable!(),
        }
    }

    fn push_child(&mut self, item: InlineItem) {
        self.parent.items.push(item);
        self.length += 1;
    }

    pub fn current_run_text(&self) -> &str {
        &self.parent.text_runs[self.run_index]
    }

    pub fn push_text(&mut self, content: &str) {
        // `shape_run_initial` assumes `QueuedText` will never end up with an empty range,
        // so make sure we don't emit empty inline text items which could cause exactly that.
        if content.is_empty() {
            return;
        }

        let text_run = &mut self.parent.text_runs[self.run_index];
        let start = text_run.len();
        text_run.push_str(content);

        if self.last_item_is_text {
            let Some(InlineItem::Text(text)) = self.parent.items.last_mut() else {
                unreachable!();
            };
            assert_eq!(text.content_range.end, start);
            text.content_range.end = text_run.len();
        } else {
            let content_range = start..text_run.len();

            self.push_child(InlineItem::Text(InlineText { content_range }));
            self.last_item_is_text = true;
        }
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
        self.push_child(InlineItem::Span(InlineSpan {
            style,
            length: 0,
            kind,
        }));
        self.last_item_is_text = false;

        InlineSpanBuilder {
            parent: self.parent,
            run_index,
            span_index,
            last_item_is_text: false,
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

impl<'a> std::fmt::Write for InlineSpanBuilder<'a> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.push_text(s);
        Ok(())
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
pub struct SpanFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub content: OffsetInlineItemFragmentVec,
}

#[derive(Debug, Clone)]
pub struct TextFragment {
    pub style: ComputedStyle,
    // self-referential
    glyphs: GlyphString<'static>,
    _font_arena: util::rc::Rc<FontArena>,
    pub baseline_offset: Vec2L,
}

impl TextFragment {
    pub fn glyphs(&self) -> &GlyphString<'_> {
        &self.glyphs
    }
}

#[derive(Debug)]
pub struct RubyFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub content: Vec<(Vec2L, RubyBaseFragment, Vec2L, RubyAnnotationFragment)>,
}

#[derive(Debug)]
pub struct RubyBaseFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub struct RubyAnnotationFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    pub primary_font: Font,
    pub baseline_y: FixedL,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug)]
pub enum InlineItemFragment {
    Span(SpanFragment),
    Text(TextFragment),
    Ruby(RubyFragment),
}

type OffsetInlineItemFragmentVec = Vec<(Vec2L, util::rc::Rc<InlineItemFragment>)>;

#[derive(Debug, Clone)]
pub struct LineBoxFragment {
    pub fbox: FragmentBox,
    pub baseline_y: FixedL,
    pub children: OffsetInlineItemFragmentVec,
}

#[derive(Debug, Clone)]
pub struct InlineContentFragment {
    pub fbox: FragmentBox,
    pub style: ComputedStyle,
    // NOTE: due to const reasons this can't be `Font`
    pub primary_font_metrics: FontMetrics,
    pub lines: Vec<(Vec2L, util::rc::Rc<LineBoxFragment>)>,
}

impl InlineContentFragment {
    pub const EMPTY: Self = Self {
        fbox: FragmentBox::ZERO,
        style: ComputedStyle::DEFAULT,
        primary_font_metrics: FontMetrics::ZERO,
        lines: Vec::new(),
    };
}

#[derive(Debug, Error)]
pub enum InlineLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::SelectError),
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error(transparent)]
    FreeType(#[from] text::FreeTypeError),
}

#[derive(Debug)]
struct InitialShapingResult<'a, 'f> {
    shaped: Vec<ShapedItem<'a, 'f>>,
    break_opportunities: Vec<usize>,
    text_leaf_items: Vec<LeafItemRange<'a>>,
    bidi: unicode_bidi::BidiInfo<'a>,
    font_feature_events: Vec<FontFeatureEvent>,
    grapheme_cluster_boundaries: Vec<usize>,
}

impl InitialShapingResult<'_, '_> {
    fn empty() -> Self {
        Self {
            shaped: Vec::new(),
            break_opportunities: Vec::new(),
            text_leaf_items: Vec::new(),
            bidi: unicode_bidi::BidiInfo::new("", None),
            font_feature_events: Vec::new(),
            grapheme_cluster_boundaries: Vec::new(),
        }
    }
}

// TODO: How should reordering affect padding fragmentation?
//       Is the current implementation correct? (everything in visual order)
/// Holds per-span state prepared during shaping and used during further layout to
/// calculate span fragmentation.
#[derive(Debug, Clone, Copy)]
struct SpanState<'a, 'f> {
    style: &'a ComputedStyle,
    primary_font: &'f Font,
    remaining_content_bytes: u32,
    remaining_line_content_bytes: u32,
    seen_first: bool,
    parent: usize,
}

impl<'a, 'f> SpanState<'a, 'f> {
    fn new(style: &'a ComputedStyle, primary_font: &'f Font, parent: usize) -> Self {
        Self {
            style,
            primary_font,
            remaining_content_bytes: 0,
            remaining_line_content_bytes: 0,
            seen_first: false,
            parent,
        }
    }

    fn walk_up(states: &mut [Self], mut span_id: usize, mut callback: impl FnMut(&mut Self)) {
        while span_id != usize::MAX {
            let state = &mut states[span_id];
            callback(state);
            span_id = state.parent;
        }
    }
}

#[derive(Debug, Clone)]
struct LeafItemRange<'a> {
    range: Range<usize>,
    span_id: usize,
    style: &'a ComputedStyle,
}

#[derive(Debug)]
struct ShapedItem<'a, 'f> {
    range: Range<usize>,
    kind: ShapedItemKind<'a, 'f>,
    /// Padding metrics used during line breaking, note that due to bidi
    /// reordering this *may not correspond to the final padding* applied
    /// to these glyphs. In fact, since shaped items don't even correspond
    /// to particular spans, this should be entirely ignored as soon as we
    /// leave line breaking!
    padding: ShapedItemPadding,
}

#[derive(Debug, Clone)]
struct ShapedItemPadding {
    current_padding_left: FixedL,
    current_padding_right: FixedL,
}

impl ShapedItemPadding {
    // Basically placeholder values for when we don't care about this anymore but
    // need to construct a `ShapedItem`.
    // Must only be used after line-breaking when this information is no longer
    // necessary.
    const MAX: Self = Self {
        current_padding_left: FixedL::MAX,
        current_padding_right: FixedL::MAX,
    };

    fn fragment_break(&mut self) -> Self {
        let remainder = Self {
            current_padding_left: FixedL::ZERO,
            ..*self
        };
        self.current_padding_right = FixedL::ZERO;
        remainder
    }
}

#[derive(Debug)]
enum ShapedItemKind<'a, 'f> {
    Text(ShapedItemText<'f>),
    Ruby(ShapedItemRuby<'a, 'f>),
}

#[derive(Debug)]
struct ShapedItemText<'f> {
    font_matcher: FontMatcher<'f>,
    primary_font: &'f Font,
    glyphs: GlyphString<'f>,
    break_after: bool,
}

#[derive(Debug)]
struct ShapedItemRuby<'a, 'f> {
    style: ComputedStyle,
    base_annotation_pairs: Vec<(ShapedRubyBase<'a, 'f>, ShapedRubyAnnotation<'a, 'f>)>,
    span_id: usize,
}

#[derive(Debug)]
struct ShapedRubyBase<'a, 'f> {
    style: &'a ComputedStyle,
    primary_font: &'f Font,
    inner: InitialShapingResult<'a, 'f>,
}

#[derive(Debug)]
struct ShapedRubyAnnotation<'a, 'f> {
    style: &'a ComputedStyle,
    primary_font: &'f Font,
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

fn primary_font_from_style<'f>(
    style: &ComputedStyle,
    font_arena: &'f FontArena,
    lctx: &mut LayoutContext,
) -> Result<&'f Font, InlineLayoutError> {
    font_matcher_from_style(style, font_arena, lctx)?
        .primary(font_arena, lctx.fonts)
        .map_err(Into::into)
}

#[derive(Debug)]

struct FontFeatureEvent {
    utf8_index: usize,
    kind: FontFeatureEventKind,
}

#[derive(Debug)]
enum FontFeatureEventKind {
    Set(OpenTypeTag, u32),
    Reset,
}

fn set_buffer_content_from_range(
    buffer: &mut ShapingBuffer,
    text: &str,
    range: Range<usize>,
    font_feature_events: &[FontFeatureEvent],
    grapheme_cluster_boundaries: &[usize],
) {
    buffer.set_pre_context(&text[..range.start]);

    let next_grapheme_boundary_idx = match grapheme_cluster_boundaries.binary_search(&range.start) {
        Ok(i) => i + 1,
        Err(i) => i,
    };
    let mut next_grapheme_boundary_it = grapheme_cluster_boundaries[next_grapheme_boundary_idx..]
        .iter()
        .copied();

    let mut next_feature_ev_idx = 'feature_idx: {
        let mut idx = match font_feature_events.binary_search_by_key(&range.start, |f| f.utf8_index)
        {
            Ok(i) => i,
            Err(i) => match i.checked_sub(1) {
                Some(prev) => prev,
                None => break 'feature_idx i,
            },
        };

        let initial_cluster_utf8_index = font_feature_events[idx].utf8_index;
        loop {
            let Some(prev_idx) = idx.checked_sub(1) else {
                break;
            };

            if font_feature_events[prev_idx].utf8_index != initial_cluster_utf8_index {
                break;
            }

            idx = prev_idx;
        }

        idx
    };

    let mut current = range.start;
    while current != range.end {
        loop {
            match font_feature_events.get(next_feature_ev_idx) {
                Some(event) if event.utf8_index <= current => {
                    match event.kind {
                        FontFeatureEventKind::Set(tag, value) => buffer.set_feature(tag, value),
                        FontFeatureEventKind::Reset => buffer.reset_features(),
                    }

                    next_feature_ev_idx += 1;
                }
                _ => break,
            }
        }

        let end = next_grapheme_boundary_it
            .next()
            .map_or(range.end, |end| end.min(range.end));
        buffer.add_grapheme(&text[current..end], current);
        current = end;
    }

    buffer.set_post_context(&text[range.end..]);
}

fn shape_run_initial<'a, 'f>(
    content: &'a InlineContent,
    run_index: usize,
    item_index: usize,
    end_item_index: &mut usize,
    lctx: &mut LayoutContext,
    font_arena: &'f FontArena,
    compute_break_opportunities: bool,
    span_state: &mut Vec<SpanState<'a, 'f>>,
    inner_style: &'a ComputedStyle,
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
            font_features_events: &[FontFeatureEvent],
            left_padding: &mut FixedL,
            buffer: &mut ShapingBuffer,
            grapheme_cluster_boundaries: &[usize],
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
                    buffer.guess_properties();
                    buffer.set_direction(direction.to_horizontal());
                    set_buffer_content_from_range(
                        buffer,
                        &text,
                        range.clone(),
                        font_features_events,
                        grapheme_cluster_boundaries,
                    );
                    buffer.shape(self.matcher.iterator(), font_arena, lctx.fonts)?
                };
                buffer.clear();

                result.push(ShapedItem {
                    range: range.clone(),
                    kind: ShapedItemKind::Text(ShapedItemText {
                        font_matcher: self.matcher.clone(),
                        primary_font: self.matcher.primary(font_arena, lctx.fonts)?,
                        glyphs: GlyphString::from_glyphs(text.clone(), range, glyphs, direction),
                        break_after,
                    }),
                    padding: ShapedItemPadding {
                        current_padding_left: *left_padding,
                        current_padding_right: FixedL::ZERO,
                    },
                });
                *left_padding = FixedL::ZERO;

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

    struct ShapedItemBuilder<'a, 'f, 's, 'l, 'll, 'la> {
        content: &'a InlineContent,
        run_text: &'a Rc<str>,
        bidi: unicode_bidi::BidiInfo<'a>,
        grapheme_cluster_boundaries: Vec<usize>,
        lctx: &'l mut LayoutContext<'ll, 'la>,
        font_arena: &'f FontArena,

        break_opportunities: Vec<usize>,
        shaped: Vec<ShapedItem<'a, 'f>>,
        span_state: &'s mut Vec<SpanState<'a, 'f>>,
        shaping_buffer: ShapingBuffer,
        queued_text: Option<QueuedText<'f>>,
        font_feature_events: Vec<FontFeatureEvent>,
        queued_padding: FixedL,
        current_span_id: usize,
        total_content_bytes_added: usize,
    }

    struct SpanStackEntry<'a> {
        parent_style: &'a ComputedStyle,
        span_content_start: usize,
        remaining_children: usize,
    }

    impl<'a, 'f> ShapedItemBuilder<'a, 'f, '_, '_, '_, '_> {
        fn push_break_opportunity(&mut self, idx: usize) {
            if let Some(&previous) = self.break_opportunities.last() {
                if previous == idx {
                    return;
                }

                debug_assert!(previous < idx);
            }

            self.break_opportunities.push(idx);
        }

        /// Compute additional break opportunities created by CSS [White Space
        /// Collapsing and Transformation Rules].
        ///
        /// This is required because `icu_segmenter` only computes break opportunities
        /// required by the Unicode Line Breaking Algorithm but CSS white space rules
        /// can introduce ones not covered by it.
        ///
        /// One example is: in `&ZeroWidthSpace; &ZeroWidthSpace;` there *is* a break
        /// opportunity after the normal space because CSS only considers `' '` and `'\t'`
        /// for break opportunities.
        ///
        /// [White Space Collapsing and Transformation Rules]: https://www.w3.org/TR/css-text-3/#white-space-phase-1
        fn compute_extra_whitespace_break_opportunities(
            &mut self,
            range: Range<usize>,
            style: &ComputedStyle,
        ) {
            match style.white_space_collapse() {
                WhiteSpaceCollapse::Preserve => {
                    let bytes = self.run_text.as_bytes();
                    let mut in_space_run = false;
                    for i in range {
                        if matches!(bytes[i], b' ' | b'\t') {
                            in_space_run = true;
                        } else {
                            if in_space_run {
                                self.push_break_opportunity(i);
                            }
                            in_space_run = false;
                        }
                    }
                }
            }
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
            let iter = segmenter
                .segment_str(&self.run_text[padded_start..padded_end])
                .map(|idx| idx + padded_start);

            let mut last = range.start;
            for idx in iter {
                // The first breaks are going to be either at the start of the string or
                // inside our "padding" look-behind character, both of which we want to ignore.
                if idx < range.start {
                    continue;
                }

                self.compute_extra_whitespace_break_opportunities(last..idx, style);
                last = idx;

                if idx > ignore_after {
                    break;
                }

                self.push_break_opportunity(idx);
            }
        }

        fn handle_span_start(&mut self, style: &'a ComputedStyle) -> Result<(), InlineLayoutError> {
            let left_padding = style.padding_left().to_physical_pixels(self.lctx.dpi);

            if left_padding != FixedL::ZERO {
                // NOTE: When thinking about this padding system, one may stumble upon the consideration:
                //       "what if some segment of text needs to have different (cloned) padding but we
                //        want to shape it along with some preceeding one" or similar.
                //       This cannot happen precisely because any change in padding parameters will also
                //       trigger a `QueuedText::flush` and shaping break.
                //       The only exception is right-side cloned padding which needs to be communicated
                //       via a side-channel because it may differ inside a single `ShapedItem`.
                if let Some(queued) = self.queued_text.take() {
                    queued.flush(
                        self.run_text.clone(),
                        &self.bidi,
                        self.font_arena,
                        self.lctx,
                        &mut self.shaped,
                        &self.font_feature_events,
                        &mut self.queued_padding,
                        &mut self.shaping_buffer,
                        &self.grapheme_cluster_boundaries,
                    )?;
                }

                self.queued_padding += left_padding;
            }

            let next_span_id = self.span_state.len();
            self.span_state.push(SpanState::new(
                style,
                font_matcher_from_style(style, self.font_arena, self.lctx)?
                    .primary(self.font_arena, self.lctx.fonts)?,
                self.current_span_id,
            ));
            self.current_span_id = next_span_id;

            Ok(())
        }

        fn handle_span_end(
            &mut self,
            style: &ComputedStyle,
            entry: &SpanStackEntry,
        ) -> Result<(), InlineLayoutError> {
            let state = &mut self.span_state[self.current_span_id];
            state.remaining_content_bytes =
                (self.total_content_bytes_added - entry.span_content_start) as u32;
            self.current_span_id = state.parent;

            if state.remaining_content_bytes == 0 {
                // FIXME: Padding for spans that have no leaf items is currently ignored.
                //        Some experimentation suggests that browsers tie such spans to the
                //        character immediately preceeding them, thus it should be possible
                //        to place them in an empty leaf text item or something and then fix
                //        the "no glyphs" case on text branch reconstruction.
                let left_padding = style.padding_left().to_physical_pixels(self.lctx.dpi);
                self.queued_padding -= left_padding;
                return Ok(());
            };

            let right_padding = style.padding_right().to_physical_pixels(self.lctx.dpi);

            if right_padding != FixedL::ZERO {
                if let Some(queued) = self.queued_text.take() {
                    queued.flush(
                        self.run_text.clone(),
                        &self.bidi,
                        self.font_arena,
                        self.lctx,
                        &mut self.shaped,
                        &self.font_feature_events,
                        &mut self.queued_padding,
                        &mut self.shaping_buffer,
                        &self.grapheme_cluster_boundaries,
                    )?;
                }

                if let Some(item) = self.shaped.last_mut() {
                    item.padding.current_padding_right += right_padding;
                }
            }

            Ok(())
        }

        fn process_items(
            mut self,
            item_index: usize,
            end_item_index: &mut usize,
            compute_break_opportunities: bool,
            inner_style: &'a ComputedStyle,
        ) -> Result<InitialShapingResult<'a, 'f>, InlineLayoutError> {
            let items = &self.content.items;
            let mut current_item = item_index;
            let mut current_style = inner_style;
            let mut span_left = usize::MAX;
            let mut span_stack: Vec<SpanStackEntry> = Vec::new();
            let mut text_leaf_items = Vec::new();

            while let Some(item) = items
                .get(current_item)
                .filter(|_| !span_stack.is_empty() || current_item < *end_item_index)
            {
                span_left -= 1;
                current_item += 1;
                match item {
                    InlineItem::Span(span) => match span.kind {
                        InlineSpanKind::Span | InlineSpanKind::RubyInternal { .. } => {
                            // TODO: Neither the margin, padding, border properties nor the any properties that do not apply to inline boxes apply to base containers or annotation containers. Additionally, line-height does not apply to annotation containers.
                            // No browser seems to respect this, also this statement is
                            // very weird since padding *does* apply to inline boxes so
                            // I have no clue what's going on in the standard here.
                            self.handle_span_start(&span.style)?;
                            span_stack.push(SpanStackEntry {
                                parent_style: current_style,
                                span_content_start: self.total_content_bytes_added,
                                remaining_children: span_left,
                            });
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
                                    &self.font_feature_events,
                                    &mut self.queued_padding,
                                    &mut self.shaping_buffer,
                                    &self.grapheme_cluster_boundaries,
                                )?;
                            }

                            let content_end = content_index + OBJECT_REPLACEMENT_LENGTH;
                            self.shaped.push(ShapedItem {
                                range: content_index..content_end,
                                kind: ShapedItemKind::Ruby(ShapedItemRuby {
                                    style: span.style.clone(),
                                    span_id: self.current_span_id,
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
                                                    self.span_state,
                                                    base_style,
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
                                                    self.span_state,
                                                    annotation_style,
                                                )?;
                                                remaining -= 1;
                                                ShapedRubyAnnotation {
                                                    style: annotation_style,
                                                    primary_font: primary_font_from_style(
                                                        annotation_style,
                                                        self.font_arena,
                                                        self.lctx,
                                                    )?,
                                                    inner: result,
                                                }
                                            } else {
                                                ShapedRubyAnnotation {
                                                    style: const { &ComputedStyle::DEFAULT },
                                                    primary_font: primary_font_from_style(
                                                        &ComputedStyle::DEFAULT,
                                                        self.font_arena,
                                                        self.lctx,
                                                    )?,
                                                    inner: InitialShapingResult::empty(),
                                                }
                                            };

                                            result.push((base, annotation));
                                        }

                                        result
                                    },
                                }),
                                padding: ShapedItemPadding {
                                    current_padding_left: self.queued_padding,
                                    current_padding_right: FixedL::ZERO,
                                },
                            });
                            self.queued_padding = FixedL::ZERO;
                            self.total_content_bytes_added += OBJECT_REPLACEMENT_LENGTH;

                            if compute_break_opportunities {
                                if content_index != 0 {
                                    self.push_break_opportunity(content_index);
                                }
                                if content_end != self.run_text.len() {
                                    self.push_break_opportunity(content_end);
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
                                    &self.font_feature_events,
                                    &mut self.queued_padding,
                                    &mut self.shaping_buffer,
                                    &self.grapheme_cluster_boundaries,
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

                        let font_feature_settings = current_style.font_feature_settings();
                        for (tag, value) in font_feature_settings.iter() {
                            self.font_feature_events.push(FontFeatureEvent {
                                utf8_index: text.content_range.start,
                                kind: FontFeatureEventKind::Set(tag, value),
                            });
                        }

                        if !font_feature_settings.is_empty() {
                            self.font_feature_events.push(FontFeatureEvent {
                                utf8_index: text.content_range.end,
                                kind: FontFeatureEventKind::Reset,
                            });
                        }

                        text_leaf_items.push(LeafItemRange {
                            range: text.content_range.clone(),
                            span_id: self.current_span_id,
                            style: current_style,
                        });
                        // HACK: This feels hacky but we need to make sure gets done here
                        //       without requiring that the queued text gets flushed.
                        self.total_content_bytes_added += self.run_text[text.content_range.clone()]
                            .bytes()
                            .filter(|&b| b != b'\n')
                            .count();

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
                    self.handle_span_end(current_style, &popped)?;
                    current_style = popped.parent_style;
                    span_left = popped.remaining_children;
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
                    &self.font_feature_events,
                    &mut self.queued_padding,
                    &mut self.shaping_buffer,
                    &self.grapheme_cluster_boundaries,
                )?;
            }

            debug_assert!(compute_break_opportunities || self.break_opportunities.is_empty());

            Ok(InitialShapingResult {
                shaped: self.shaped,
                break_opportunities: self.break_opportunities,
                text_leaf_items,
                bidi: self.bidi,
                font_feature_events: self.font_feature_events,
                grapheme_cluster_boundaries: self.grapheme_cluster_boundaries,
            })
        }
    }

    let run_text = &content.text_runs[run_index];
    let default_para_level = match inner_style.direction() {
        // TODO: `None` or `LTR_LEVEL` here? Picked `None` for now as that's the safest bet.
        computed::Direction::Ltr => None,
        computed::Direction::Rtl => Some(unicode_bidi::RTL_LEVEL),
    };
    ShapedItemBuilder {
        content,
        run_text,
        bidi: unicode_bidi::BidiInfo::new(run_text, default_para_level),
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
        shaped: Vec::new(),
        span_state,
        shaping_buffer: ShapingBuffer::new(),
        queued_text: None,
        font_feature_events: Vec::new(),
        queued_padding: FixedL::ZERO,
        current_span_id: usize::MAX,
        total_content_bytes_added: 0,
    }
    .process_items(
        item_index,
        end_item_index,
        compute_break_opportunities,
        inner_style,
    )
}

struct BreakingContext<'f, 'l, 'a, 'b> {
    layout: &'a mut LayoutContext<'l, 'b>,
    constraints: &'a LayoutConstraints,
    font_arena: &'f FontArena,
    break_opportunities: &'a [usize],
    break_buffer: text::ShapingBuffer,
    font_feature_events: &'a [FontFeatureEvent],
    grapheme_cluster_boundaries: &'a [usize],
}

#[derive(Debug)]
enum BreakOutcome<'a, 'f> {
    BreakSplit(ShapedItem<'a, 'f>),
    BreakAfter,
    BreakBefore,
    None,
}

impl<'a, 'f> ShapedItem<'a, 'f> {
    fn line_break(
        &mut self,
        current_width: &mut FixedL,
        ctx: &mut BreakingContext<'f, '_, '_, '_>,
    ) -> Result<BreakOutcome<'a, 'f>, InlineLayoutError> {
        let can_break_before = *current_width != FixedL::ZERO;
        *current_width += self.padding.current_padding_left;

        if *current_width > ctx.constraints.size.x {
            return Ok(BreakOutcome::BreakBefore);
        }

        match &mut self.kind {
            ShapedItemKind::Text(text) => text.line_break(
                &mut self.range,
                current_width,
                can_break_before,
                ctx,
                &mut self.padding,
            ),
            ShapedItemKind::Ruby(_) => {
                // TODO: Implement proper ruby line breaking
                //       It should only allow breaking between distinct base-annotation pairs.
                shaped_item_width(current_width, self);
                *current_width += self.padding.current_padding_right;
                if *current_width > ctx.constraints.size.x {
                    Ok(BreakOutcome::BreakBefore)
                } else {
                    Ok(BreakOutcome::None)
                }
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
    fn break_opportunity_to_range(&self, opportunity: usize) -> Range<usize> {
        let text = self.glyphs.text();
        let mut break_start_index = opportunity;
        let mut break_end_index = opportunity;
        loop {
            let Some(prev) = break_start_index.checked_sub(1) else {
                break;
            };

            if text.as_bytes()[prev] == b' ' {
                break_start_index = prev;
            } else {
                break;
            }
        }

        while text.as_bytes()[break_end_index] == b' ' {
            break_end_index += 1;
        }

        break_start_index..break_end_index
    }

    fn line_break<'a>(
        &mut self,
        range: &mut Range<usize>,
        current_width: &mut FixedL,
        can_break_before: bool,
        ctx: &mut BreakingContext<'f, '_, '_, '_>,
        padding: &mut ShapedItemPadding,
    ) -> Result<BreakOutcome<'a, 'f>, InlineLayoutError> {
        let initial_x = *current_width;
        let mut glyph_it = self.glyphs.iter_glyphs_logical().peekable();
        while let Some(glyph) = glyph_it.next() {
            *current_width += glyph.x_advance;

            if glyph_it.peek().is_none() {
                *current_width += padding.current_padding_right;
            }

            if *current_width > ctx.constraints.size.x {
                // We want to also consider breaking within the current glyph so let's
                // start looking for break opportunities anywhere before the *next* glyph.
                let glyph_end = if self.glyphs.direction().is_reverse() {
                    glyph.cluster + 1
                } else {
                    glyph_it.peek().map(|g| g.cluster).unwrap_or(range.end)
                };
                let opportunities = &ctx.break_opportunities[..match ctx
                    .break_opportunities
                    .binary_search(&glyph_end)
                {
                    Ok(idx) => idx + 1,
                    Err(idx) => idx,
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

                    // FIXME: This takes care of collapsing whitespace in text, but
                    //        if text with a space at the end is followed by a ruby
                    //        container then that space will remain after line-breaking.
                    let break_range = self.break_opportunity_to_range(opportunity);
                    if let Some((broken, remaining)) = self.glyphs.break_around(
                        break_range,
                        ctx.constraints.size.x - initial_x,
                        &mut ctx.break_buffer,
                        ctx.font_feature_events,
                        ctx.grapheme_cluster_boundaries,
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
                            padding: padding.fragment_break(),
                        }));
                    }
                }

                // We failed to break inside the string and we're *not* the first item on the line,
                // so we can try breaking before the whole text run instead.
                if can_break_before {
                    return Ok(BreakOutcome::BreakBefore);
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
    *result += item.padding.current_padding_left;
    *result += item.padding.current_padding_right;
    match &item.kind {
        ShapedItemKind::Text(text) => {
            for glyph in text.glyphs.iter_glyphs_visual() {
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

// unsafe because `font_arena` must hold all fonts required by `initial_shaping_result`
unsafe fn layout_run_full<'a, 'f>(
    content: &'a InlineContent,
    initial_shaping_result: InitialShapingResult<'a, 'f>,
    span_state: Vec<SpanState<'a, 'f>>,
    lctx: &mut LayoutContext,
    constraints: &LayoutConstraints,
    font_arena: util::rc::Rc<FontArena>,
) -> Result<InlineContentFragment, InlineLayoutError> {
    fn split_on_leaves<'s, 'f>(
        range: Range<usize>,
        shaped: &ShapedItemText<'f>,
        leaves: &[LeafItemRange<'s>],
        mut push_section: impl FnMut(
            &LeafItemRange<'s>,
            GlyphString<'f>,
            Range<usize>,
        ) -> Result<(), InlineLayoutError>,
    ) -> Result<(), InlineLayoutError> {
        let mut glyphs = shaped.glyphs.clone();

        // TODO: Simplify this code, it's kind of hacky at the moment.
        if !glyphs.direction().is_reverse() {
            let mut si = match leaves.binary_search_by_key(&range.start, |l| l.range.start) {
                Ok(s) => s,
                Err(s) => s - 1,
            };

            if leaves
                .get(si + 1)
                .is_none_or(|l| l.range.start >= range.end)
            {
                push_section(&leaves[si], glyphs, range.clone())?;
                return Ok(());
            }

            let mut i = range.start;
            while i != range.end {
                let end = leaves
                    .get(si + 1)
                    .map(|l| l.range.start.min(range.end))
                    .unwrap_or(range.end);

                if let Some(section_glyphs) = glyphs.split_off_visual_start(end) {
                    push_section(&leaves[si], section_glyphs, i..end)?;
                }

                i = end;
                si += 1;
            }
        } else {
            let mut si = match leaves.binary_search_by_key(&range.end, |l| l.range.start) {
                Ok(s) => s.saturating_sub(1),
                Err(s) => s - 1,
            };

            if leaves[si].range.start <= range.start {
                push_section(&leaves[si], glyphs, range.clone())?;
                return Ok(());
            }

            let mut i = range.end;
            while i != range.start {
                let ref leaf @ LeafItemRange {
                    range: Range { start, .. },
                    ..
                } = leaves[si];
                let start = start.max(range.start);

                if let Some(section_glyphs) = glyphs.split_off_visual_start(start) {
                    push_section(leaf, section_glyphs, start..i)?;
                }

                i = start;
                si = match si.checked_sub(1) {
                    Some(i) => i,
                    None => {
                        debug_assert_eq!(i, range.start);
                        break;
                    }
                }
            }
        };

        Ok(())
    }

    fn reorder<'a>(
        shaped: &[ShapedItem<'a, '_>],
        bidi: &'a unicode_bidi::BidiInfo<'a>,
        mut push_item: impl FnMut(&ShapedItem<'a, '_>) -> Result<(), InlineLayoutError>,
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
            let start = line_range.start.max(paragraph.range.start);
            let end = line_range.end.min(paragraph.range.end);
            if start < end {
                let (_, mut paragraph_runs) = bidi.visual_runs(paragraph, start..end);
                visual_runs.append(&mut paragraph_runs);
            }
        }

        for range in visual_runs {
            let mut push_item_in_range =
                |item: &ShapedItem<'a, '_>| -> Result<(), InlineLayoutError> {
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
                            tmp.glyphs.split_off_visual_start(range.start);
                            push_item(&ShapedItem {
                                range: range.start..item.range.end,
                                kind: ShapedItemKind::Text(tmp),
                                padding: ShapedItemPadding::MAX,
                            })
                        } else {
                            debug_assert!(range.end < item.range.end);
                            if let Some(before) = tmp.glyphs.split_off_visual_start(range.end) {
                                tmp.glyphs = before;
                                push_item(&ShapedItem {
                                    range: item.range.start..range.end,
                                    kind: ShapedItemKind::Text(tmp),
                                    padding: ShapedItemPadding::MAX,
                                })
                            } else {
                                Ok(())
                            }
                        }
                    } else {
                        unreachable!(
                            "bidi reordering attempted to partially reorder a non-text item"
                        );
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
                    Err(i) => i + 1,
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
    struct FragmentBuilder<'t, 'f> {
        result: InlineContentFragment,
        current_y: FixedL,
        line_align: HorizontalAlignment,
        bidi: unicode_bidi::BidiInfo<'t>,
        text_leaf_items: &'t [LeafItemRange<'t>],
        dpi: u32,
        content: &'t InlineContent,
        span_state: Vec<SpanState<'t, 'f>>,
    }

    #[derive(Debug)]
    struct InlineItemFragmentBuilder<'o, 'a> {
        output: &'o mut OffsetInlineItemFragmentVec,
        min_ruby_y: &'o mut FixedL,
        line_metrics: LineHeightMetrics,
        current_top_y: FixedL,
        current_x: FixedL,
        content: &'a InlineContent,
        dpi: u32,
    }

    #[derive(Debug, Clone, Copy)]
    struct LineHeightMetrics {
        max_ascender: FixedL,
        min_descender: FixedL,
    }

    #[derive(Debug, Clone, Copy)]
    enum LineHeight {
        Normal,
        Value(FixedL),
    }

    impl LineHeight {
        const ONE: Self = Self::Value(FixedL::ONE);
        const RUBY_ANNOTATION: Self = Self::ONE;
    }

    impl LineHeightMetrics {
        const ZERO: Self = LineHeightMetrics {
            max_ascender: FixedL::ZERO,
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

                        for glyph in text.glyphs.iter_glyphs_visual() {
                            self.expand_to(
                                glyph.font.metrics().ascender + half_leading,
                                glyph.font.metrics().descender - half_leading,
                            );
                        }
                    }
                    LineHeight::Value(value) => {
                        let computed_font_size =
                            text.font_matcher.size() * text.font_matcher.dpi() as i32 / 72;
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
                    for (base, _) in &ruby.base_annotation_pairs {
                        for item in &base.inner.shaped {
                            self.process_item(item, line_height);
                        }
                    }
                }
            }
        }
    }

    struct InlineBoxSizingMetrics {
        top_y_offset: FixedL,
        box_height: FixedL,
        ascender: FixedL,
    }

    impl<'o, 'a> InlineItemFragmentBuilder<'o, 'a> {
        fn child_builder<'o2>(
            &mut self,
            output: &'o2 mut OffsetInlineItemFragmentVec,
            min_ruby_y: &'o2 mut FixedL,
            line_metrics: LineHeightMetrics,
            top_y: FixedL,
            current_x: FixedL,
        ) -> InlineItemFragmentBuilder<'o2, 'a> {
            InlineItemFragmentBuilder {
                output,
                min_ruby_y,
                line_metrics,
                current_top_y: top_y,
                current_x,
                dpi: self.dpi,
                content: self.content,
            }
        }

        fn compute_inline_box_sizing_metrics(
            &self,
            style: &ComputedStyle,
            font_metrics: &FontMetrics,
        ) -> InlineBoxSizingMetrics {
            // https://drafts.csswg.org/css-inline/#line-fill
            let (ascender, descender) = match style.inline_sizing() {
                InlineSizing::Normal => (font_metrics.ascender, font_metrics.descender),
                InlineSizing::Stretch => (
                    self.line_metrics.max_ascender,
                    self.line_metrics.min_descender,
                ),
            };

            InlineBoxSizingMetrics {
                top_y_offset: self.current_top_y - ascender,
                box_height: ascender - descender,
                ascender,
            }
        }

        fn rebuild_leaf_branch(
            &mut self,
            mut span_id: usize,
            mut inner_width: FixedL,
            leaf: util::rc::Rc<InlineItemFragment>,
            content_len: usize,
            // NOTE: I tried putting this in `InlineItemFragmentBuilder` but lifetime hell
            //       ensued. Maybe try taming that at some point in the future.
            span_state: &mut [SpanState<'_, '_>],
        ) -> (util::rc::Rc<InlineItemFragment>, FixedL, FixedL) {
            let mut result = leaf;
            let mut y_correction = FixedL::ZERO;

            // NOTE: can't use `SpanState::walk_up` because of `result` moving shenanigans
            while span_id != usize::MAX {
                let state = &mut span_state[span_id];
                let font_metrics = state.primary_font.metrics();

                let InlineBoxSizingMetrics {
                    top_y_offset,
                    box_height: logical_height,
                    ascender: _,
                } = self.compute_inline_box_sizing_metrics(state.style, font_metrics);

                let mut part = BoxFragmentationPart::VERTICAL_FULL;
                if !state.seen_first {
                    part |= BoxFragmentationPart::HORIZONTAL_FIRST;
                    state.seen_first = true;
                }
                state.remaining_line_content_bytes -= content_len as u32;
                if state.remaining_content_bytes == 0 && state.remaining_line_content_bytes == 0 {
                    part |= BoxFragmentationPart::HORIZONTAL_LAST;
                }

                let fbox = FragmentBox::new_styled_fragmented(
                    Vec2L::new(inner_width, logical_height),
                    self.dpi,
                    state.style,
                    part,
                );
                inner_width = fbox.size_for_layout().x;
                result = util::rc::Rc::new(InlineItemFragment::Span(SpanFragment {
                    content: vec![(
                        Vec2L::new(FixedL::ZERO, y_correction - top_y_offset),
                        result,
                    )],
                    fbox,
                    style: state.style.clone(),
                    primary_font: state.primary_font.clone(),
                }));
                y_correction = top_y_offset - fbox.content_offset().y;
                span_id = state.parent;
            }

            (result, inner_width, y_correction)
        }

        // These functions are unsafe because there is a "this FontArena must hold all fonts used by
        // the input items" invarant.
        unsafe fn reorder_and_append(
            &mut self,
            shaped: &[ShapedItem<'a, '_>],
            font_arena: util::rc::Rc<FontArena>,
            bidi: &'a unicode_bidi::BidiInfo<'a>,
            text_leaf_items: &'a [LeafItemRange<'a>],
            span_state: &mut [SpanState<'_, '_>],
        ) -> Result<(), InlineLayoutError> {
            reorder(shaped, bidi, |item| match &item.kind {
                ShapedItemKind::Text(text) => split_on_leaves(
                    item.range.clone(),
                    text,
                    text_leaf_items,
                    |leaf, glyphs, range| {
                        let inner_width: FixedL =
                            glyphs.iter_glyphs_visual().map(|g| g.x_advance).sum();
                        let fragment = TextFragment {
                            style: leaf.style.clone(),
                            glyphs: unsafe {
                                std::mem::transmute::<GlyphString<'_>, GlyphString<'static>>(glyphs)
                            },
                            _font_arena: font_arena.clone(),
                            baseline_offset: Vec2::new(FixedL::ZERO, self.current_top_y),
                        };

                        let (fragment, width, y_correction) = self.rebuild_leaf_branch(
                            leaf.span_id,
                            inner_width,
                            InlineItemFragment::Text(fragment).into(),
                            range.len(),
                            span_state,
                        );
                        self.output
                            .push((Vec2L::new(self.current_x, y_correction), fragment));
                        self.current_x += width;

                        Ok(())
                    },
                ),
                ShapedItemKind::Ruby(ruby) => {
                    let mut result = RubyFragment {
                        // TODO: What box should a ruby container fragment have?
                        //       For now we'll just leave it zero-sized.
                        fbox: FragmentBox::ZERO,
                        style: ruby.style.clone(),
                        content: Vec::new(),
                    };

                    let mut ruby_current_x = FixedL::ZERO;
                    for (base, annotation) in &ruby.base_annotation_pairs {
                        let mut base_width = FixedL::ZERO;
                        let mut base_metrics = LineHeightMetrics::ZERO;
                        let mut annotation_width = FixedL::ZERO;
                        let mut annotation_metrics = LineHeightMetrics::ZERO;
                        for item in &base.inner.shaped {
                            shaped_item_width(&mut base_width, item);
                            base_metrics.process_item(item, LineHeight::ONE);
                        }
                        for item in &annotation.inner.shaped {
                            shaped_item_width(&mut annotation_width, item);
                            annotation_metrics.process_item(item, LineHeight::RUBY_ANNOTATION);
                        }

                        let base_font_metrics = base.primary_font.metrics();
                        let InlineBoxSizingMetrics {
                            top_y_offset: mut base_y_offset,
                            box_height: mut base_height,
                            ascender: mut base_box_ascender,
                        } = self.compute_inline_box_sizing_metrics(base.style, base_font_metrics);
                        let annotation_height = annotation_metrics.height();
                        let annotation_y_offset = self.current_top_y
                            - base_metrics.max_ascender
                            - annotation_metrics.max_ascender
                            + annotation_metrics.min_descender;
                        let signed_half_padding = (annotation_width - base_width) / 2;
                        let base_half_padding = signed_half_padding.max(FixedL::ZERO);
                        let annotation_half_padding = (-signed_half_padding).max(FixedL::ZERO);
                        let ruby_width = base_width.max(annotation_width);

                        // HACK: This is to make ruby base and annotation backgrounds not overlap.
                        //       The correct solution is block layout (#111) or a subrandr-specific
                        //       style property.
                        let background_overlap = (annotation_y_offset - base_y_offset
                            + annotation_height)
                            .max(FixedL::ZERO);
                        base_y_offset += background_overlap;
                        base_box_ascender -= background_overlap;
                        base_height -= background_overlap;

                        // FIXME: Apparently ruby internal boxes are not supposed to use
                        //        inline-sizing sizing. Now this makes sense with the ruby
                        //        annotation box because it creates/is a new line box and
                        //        should logically obey line box sizing rules.
                        //        However I'm not certain what this means for ruby base
                        //        boxes? Should they just fit their contents?
                        let mut base_fragment = RubyBaseFragment {
                            fbox: FragmentBox::new_styled(
                                Vec2::new(ruby_width, base_height),
                                self.dpi,
                                base.style,
                            ),
                            style: base.style.clone(),
                            primary_font: base.primary_font.clone(),
                            children: Vec::new(),
                        };

                        self.child_builder(
                            &mut base_fragment.children,
                            // TODO: ruby nested in base
                            &mut { FixedL::ZERO },
                            self.line_metrics,
                            base_box_ascender,
                            base_half_padding,
                        )
                        .reorder_and_append(
                            &base.inner.shaped,
                            font_arena.clone(),
                            &base.inner.bidi,
                            &base.inner.text_leaf_items,
                            span_state,
                        )?;

                        let mut annotation_fragment = RubyAnnotationFragment {
                            fbox: FragmentBox::new_styled(
                                Vec2::new(ruby_width, annotation_height),
                                self.dpi,
                                annotation.style,
                            ),
                            style: annotation.style.clone(),
                            primary_font: annotation.primary_font.clone(),
                            baseline_y: annotation_metrics.max_ascender,
                            children: Vec::new(),
                        };

                        self.child_builder(
                            &mut annotation_fragment.children,
                            // TODO: ruby nested in annotation
                            &mut { FixedL::ZERO },
                            annotation_metrics,
                            annotation_metrics.max_ascender,
                            annotation_half_padding,
                        )
                        .reorder_and_append(
                            &annotation.inner.shaped,
                            font_arena.clone(),
                            &annotation.inner.bidi,
                            &annotation.inner.text_leaf_items,
                            span_state,
                        )?;

                        let width_for_layout = base_fragment.fbox.size_for_layout().x;
                        *self.min_ruby_y = (*self.min_ruby_y).min(annotation_y_offset);
                        result.content.push((
                            Vec2::new(ruby_current_x, base_y_offset),
                            base_fragment,
                            Vec2::new(ruby_current_x, annotation_y_offset),
                            annotation_fragment,
                        ));
                        ruby_current_x += width_for_layout;
                    }

                    let (fragment, width, y_correction) = self.rebuild_leaf_branch(
                        ruby.span_id,
                        ruby_current_x,
                        InlineItemFragment::Ruby(result).into(),
                        OBJECT_REPLACEMENT_LENGTH,
                        span_state,
                    );
                    self.output
                        .push((Vec2L::new(self.current_x, y_correction), fragment));
                    self.current_x += width;

                    Ok(())
                }
            })?;

            Ok(())
        }
    }

    impl<'t, 'f> FragmentBuilder<'t, 'f> {
        fn split_on_leaves_for_fragmentation(
            item: &ShapedItem<'t, 'f>,
            leaves: &[LeafItemRange],
            mut on_leaf: impl FnMut(usize, Range<usize>),
        ) {
            match &item.kind {
                ShapedItemKind::Text(_) => {
                    // TODO: deduplicate this with split_on_leaves rtl branch
                    //       (this is the same thing sans splitting glyphs)
                    let mut si =
                        match leaves.binary_search_by_key(&item.range.end, |l| l.range.start) {
                            Ok(s) => s.saturating_sub(1),
                            Err(s) => s - 1,
                        };

                    if leaves[si].range.start <= item.range.start {
                        return on_leaf(leaves[si].span_id, item.range.clone());
                    }

                    let mut i = item.range.end;
                    while i != item.range.start {
                        let ref leaf @ LeafItemRange {
                            range: Range { start, .. },
                            ..
                        } = leaves[si];
                        let start = start.max(item.range.start);

                        on_leaf(leaf.span_id, start..i);

                        i = start;
                        si = si.wrapping_sub(1);
                    }
                }
                ShapedItemKind::Ruby(ruby) => on_leaf(ruby.span_id, item.range.clone()),
            }
        }

        fn update_line_fragmentation_state_pre(
            &mut self,
            shaped_item: &ShapedItem<'t, 'f>,
            leaves: &[LeafItemRange],
        ) {
            Self::split_on_leaves_for_fragmentation(shaped_item, leaves, |span_id, range| {
                let range_len = range.len();

                SpanState::walk_up(&mut self.span_state, span_id, |state| {
                    state.remaining_content_bytes -= range_len as u32;
                    state.remaining_line_content_bytes += range_len as u32;
                });
            });

            if let ShapedItemKind::Ruby(ruby) = &shaped_item.kind {
                for (base, annotation) in &ruby.base_annotation_pairs {
                    for item in &base.inner.shaped {
                        self.update_line_fragmentation_state_pre(item, &base.inner.text_leaf_items);
                    }

                    for item in &annotation.inner.shaped {
                        self.update_line_fragmentation_state_pre(
                            item,
                            &annotation.inner.text_leaf_items,
                        );
                    }
                }
            }
        }

        unsafe fn push_line(
            &mut self,
            shaped: &mut [ShapedItem<'t, 'f>],
            font_arena: util::rc::Rc<FontArena>,
        ) -> Result<(), InlineLayoutError> {
            let mut line_width = FixedL::ZERO;
            let mut line_metrics = LineHeightMetrics::ZERO;
            for item in &*shaped {
                shaped_item_width(&mut line_width, item);
                line_metrics.process_item(item, LineHeight::Normal);
                self.update_line_fragmentation_state_pre(item, self.text_leaf_items);
            }

            let line_height = line_metrics.height();
            let mut line_box = LineBoxFragment {
                fbox: FragmentBox::new_content_only(Vec2::new(line_width, line_height)),
                baseline_y: line_metrics.max_ascender,
                children: Vec::new(),
            };

            let mut min_ruby_y = FixedL::ZERO;
            {
                InlineItemFragmentBuilder {
                    output: &mut line_box.children,
                    min_ruby_y: &mut min_ruby_y,
                    line_metrics,
                    current_top_y: line_metrics.max_ascender,
                    current_x: FixedL::ZERO,
                    dpi: self.dpi,
                    content: self.content,
                }
                .reorder_and_append(
                    shaped,
                    font_arena,
                    &self.bidi,
                    self.text_leaf_items,
                    &mut self.span_state,
                )?;
            }

            // Make sure that our "fragile" byte coverage calculations were correct.
            // `finish()` also makes sure the total content byte coverage was all
            // accounted for.
            for item in &*shaped {
                Self::split_on_leaves_for_fragmentation(
                    item,
                    self.text_leaf_items,
                    |span_id, _| {
                        SpanState::walk_up(&mut self.span_state, span_id, |state| {
                            // FIXME: This **can** happen because `split_on_leaves` doesn't push sections
                            //        without any glyphs, basically this is an issue only in extreme
                            //        edge cases and falls into the category of "empty span" issues.
                            // debug_assert_eq!(state.remaining_line_content_bytes, 0);
                            state.remaining_line_content_bytes = 0;
                        });
                    },
                );
            }

            let aligning_x_offset = match self.line_align {
                HorizontalAlignment::Left => I26Dot6::ZERO,
                HorizontalAlignment::Center => -line_width / 2,
                HorizontalAlignment::Right => -line_width,
            };

            let ruby_leading = (-min_ruby_y).max(FixedL::ZERO);
            self.result.fbox.content_size.x = self
                .result
                .fbox
                .content_size
                .x
                .max(line_box.fbox.size_for_layout().x);
            self.current_y += ruby_leading;
            self.result.lines.push((
                Vec2L::new(aligning_x_offset, self.current_y),
                line_box.into(),
            ));
            self.current_y += line_height;
            self.result.fbox.content_size.y = self.current_y;

            Ok(())
        }

        fn finish(self) -> InlineContentFragment {
            let mut fragment = self.result;

            // TODO: Investigate whether `self.total_content_bytes_added` hack counts
            //       the same as `QueuedText::flush` in the presence of consecutive newlines.
            #[cfg(debug_assertions)]
            for span_state in self.span_state {
                assert_eq!(
                    span_state.remaining_content_bytes, 0,
                    "a span's content byte counter wasn't exhausted"
                );
            }

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

    let InitialShapingResult {
        mut shaped,
        break_opportunities,
        ref text_leaf_items,
        bidi,
        font_feature_events,
        grapheme_cluster_boundaries,
    } = initial_shaping_result;

    let mut builder = FragmentBuilder {
        current_y: FixedL::ZERO,
        result: InlineContentFragment {
            style: content.root_style.clone(),
            primary_font_metrics: *primary_font_from_style(&content.root_style, &font_arena, lctx)?
                .metrics(),
            ..InlineContentFragment::EMPTY
        },
        line_align: content.root_style.text_align(),
        bidi,
        text_leaf_items,
        dpi: lctx.dpi,
        content,
        span_state,
    };

    if constraints.size.x != FixedL::MAX && !break_opportunities.is_empty() {
        let mut breaking_context = BreakingContext {
            layout: lctx,
            constraints,
            font_arena: &font_arena,
            break_opportunities: &break_opportunities,
            break_buffer: text::ShapingBuffer::new(),
            font_feature_events: &font_feature_events,
            grapheme_cluster_boundaries: &grapheme_cluster_boundaries,
        };

        'break_loop: loop {
            let mut current_width = FixedL::ZERO;
            'item_loop: for mut i in 0..shaped.len() {
                let item = &mut shaped[i];
                let remaining = match item.line_break(&mut current_width, &mut breaking_context)? {
                    BreakOutcome::BreakSplit(item) => Some(item),
                    BreakOutcome::BreakAfter => None,
                    BreakOutcome::BreakBefore => {
                        i = i.saturating_sub(1);
                        None
                    }
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
        'break_loop: loop {
            for i in 0..shaped.len() {
                if shaped[i].forces_line_break_after() {
                    unsafe { builder.push_line(&mut shaped[..=i], font_arena.clone())? };
                    shaped.drain(..=i);
                    continue 'break_loop;
                }
            }

            if !shaped.is_empty() {
                unsafe { builder.push_line(&mut shaped, font_arena.clone())? };
            }
            break;
        }
    }

    Ok(builder.finish())
}

pub struct PartialInline<'a> {
    content: &'a InlineContent,
    font_arena: util::rc::Rc<FontArena>,
    span_state: Vec<SpanState<'a, 'static>>,
    initial_shaping_result: InitialShapingResult<'a, 'static>,
}

pub fn shape<'l, 'a, 'b, 'c>(
    lctx: &'b mut LayoutContext<'l, 'a>,
    content: &'c InlineContent,
) -> Result<PartialInline<'c>, InlineLayoutError> {
    let font_arena = util::rc::Rc::new(FontArena::new());

    if content.text_runs.is_empty() {
        return Ok(PartialInline {
            content,
            span_state: Vec::new(),
            font_arena,
            initial_shaping_result: InitialShapingResult::empty(),
        });
    }

    let mut span_state = Vec::new();
    let initial_shaping_result = shape_run_initial(
        content,
        0,
        0,
        &mut content.items.len(),
        lctx,
        &font_arena,
        true,
        &mut span_state,
        &content.root_style,
    )?;

    Ok(PartialInline {
        content,
        span_state: unsafe { std::mem::transmute(span_state) },
        initial_shaping_result: unsafe { std::mem::transmute(initial_shaping_result) },
        font_arena,
    })
}

impl PartialInline<'_> {
    pub fn intrinsic_width(&self, _lctx: &mut LayoutContext) -> FixedL {
        let mut max = FixedL::ZERO;
        let mut current = FixedL::ZERO;
        for item in &self.initial_shaping_result.shaped {
            shaped_item_width(&mut current, item);
            if item.forces_line_break_after() {
                max = max.max(current);
                current = FixedL::ZERO;
            }
        }
        max.max(current)
    }

    pub fn layout<'b, 'l, 'a>(
        self,
        lctx: &'b mut LayoutContext<'l, 'a>,
        constraints: &LayoutConstraints,
    ) -> Result<InlineContentFragment, InlineLayoutError> {
        unsafe {
            layout_run_full(
                self.content,
                self.initial_shaping_result,
                self.span_state,
                lctx,
                constraints,
                self.font_arena,
            )
        }
    }
}

pub fn layout<'l, 'a, 'b, 'c>(
    lctx: &'b mut LayoutContext<'l, 'a>,
    constraints: &LayoutConstraints,
    content: &'c InlineContent,
) -> Result<InlineContentFragment, InlineLayoutError> {
    shape(lctx, content).and_then(|s| s.layout(lctx, constraints))
}
