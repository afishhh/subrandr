use std::ops::Range;

use crate::{
    layout::block::BlockContainer,
    style::{computed::WhiteSpaceCollapse, ComputedStyle},
};

use super::{
    InlineBlock, InlineContent, InlineItem, InlineSpan, InlineSpanKind, InlineText,
    OBJECT_REPLACEMENT_CHARACTER,
};

pub struct InlineContentBuilder {
    text_runs: Vec<String>,
    // NOTE: This item list is a bit special and may contain `Text` items
    //       with an empty `content_range`. Those are treated as "tombstones"
    //       for deleted (collapsed) text items and get removed when finalizing
    //       into an `InlineContent`.
    items: Vec<InlineItem>,
    root_style: ComputedStyle,

    text_item_stack: Vec<TextStackItem>,
    n_dead_items: usize,

    buffered_item_text_length: usize,
}

impl InlineItem {
    fn is_tombstone(&self) -> bool {
        if let Self::Text(InlineText { content_range }) = self {
            content_range.is_empty()
        } else {
            false
        }
    }
}

struct TextStackItem {
    item_index: usize,
    run_index: usize,
    style: ComputedStyle,
}

impl TextStackItem {
    fn get<'a>(&self, items: &'a [InlineItem]) -> &'a InlineText {
        match items.get(self.item_index) {
            Some(InlineItem::Text(text)) => text,
            _ => unreachable!("Invalid item reference in text_item_stack"),
        }
    }
}

impl InlineContentBuilder {
    pub fn new(root_style: ComputedStyle) -> Self {
        Self {
            text_runs: Vec::new(),
            items: Vec::new(),
            root_style,
            text_item_stack: Vec::new(),
            n_dead_items: 0,
            buffered_item_text_length: 0,
        }
    }

    pub fn set_root_style(&mut self, style: ComputedStyle) {
        self.root_style = style;
    }

    pub fn root(&mut self) -> InlineSpanBuilder<'_> {
        if self.text_runs.is_empty() {
            self.text_runs.push(String::new());
        }

        InlineSpanBuilder {
            parent: self,
            run_index: 0,
            span_index: usize::MAX,
            run_text_item_stack_start: 0,
            finish_run_text_on_end: false,
        }
    }

    fn last_text_item_content(&self, text_stack_start: usize) -> Option<(&str, &ComputedStyle)> {
        let stack_item = self.text_item_stack[text_stack_start..].last()?;
        let item = stack_item.get(&self.items);
        Some((
            &self.text_runs[stack_item.run_index][item.content_range.clone()],
            &stack_item.style,
        ))
    }

    fn pop_text_item_with_content_mut(
        &mut self,
        text_stack_start: usize,
    ) -> Option<(&mut String, Range<usize>, ComputedStyle)> {
        if text_stack_start >= self.text_item_stack.len() {
            return None;
        }

        let stack_item = self.text_item_stack.pop().unwrap();
        let item = stack_item.get(&self.items);
        Some((
            &mut self.text_runs[stack_item.run_index],
            item.content_range.clone(),
            stack_item.style,
        ))
    }

    fn pop_bytes_from_last_text_item(&mut self, count: usize) {
        let stack_item = self
            .text_item_stack
            .last()
            .expect("caller should ensure there is an item on the text item stack");
        let item = match self.items.get_mut(stack_item.item_index) {
            Some(InlineItem::Text(text)) => text,
            _ => unreachable!("Invalid item reference in text_item_stack"),
        };
        let run_text = &mut self.text_runs[stack_item.run_index];
        assert_eq!(item.content_range.end, run_text.len());
        run_text.truncate(run_text.len() - count);
        item.content_range.end -= count;
        if item.content_range.is_empty() {
            debug_assert_eq!(item.content_range.start, item.content_range.end);
            self.n_dead_items += 1;
            self.text_item_stack.pop();
        }
    }

    fn finish_run(&mut self, text_stack_start: usize) {
        finish_text_run_collapse(self, text_stack_start);
        assert_eq!(self.text_item_stack.len(), text_stack_start)
    }

    pub fn finish(&mut self) -> InlineContent {
        self.finish_run(0);

        InlineContent {
            text_runs: self.text_runs.drain(..).map(|s| s.into()).collect(),
            items: {
                let mut result = Vec::with_capacity(self.items.len() - self.n_dead_items);
                result.extend(self.items.drain(..).filter(|x| !x.is_tombstone()));
                debug_assert_eq!(result.len(), result.capacity());
                result.into_boxed_slice()
            },
            root_style: std::mem::take(&mut self.root_style),
        }
    }
}

// TODO: Whitespace collapsing should ignore bidi formatting characters (see css-text-4 4.1.1).

struct BuilderTextSink<'a> {
    builder: &'a mut InlineContentBuilder,
    run_index: usize,
    item_stack_start: usize,
    style: ComputedStyle,
}

impl<'a> BuilderTextSink<'a> {
    fn peek_prev(&self) -> Option<(u8, &ComputedStyle)> {
        if self.builder.buffered_item_text_length > 0 {
            Some((
                self.builder.text_runs[self.run_index]
                    .bytes()
                    .last()
                    .unwrap(),
                &self.style,
            ))
        } else {
            let (text, style) = self.builder.last_text_item_content(self.item_stack_start)?;
            Some((
                text.bytes()
                    .next_back()
                    .expect("inline text items on the text stack should be non-empty"),
                style,
            ))
        }
    }

    fn pop_prev(&mut self) {
        if let Some(new) = self.builder.buffered_item_text_length.checked_sub(1) {
            self.builder.text_runs[self.run_index].pop();
            self.builder.buffered_item_text_length = new;
        } else {
            assert!(self.item_stack_start < self.builder.text_item_stack.len());
            self.builder.pop_bytes_from_last_text_item(1);
        }
    }

    fn style(&self) -> &ComputedStyle {
        &self.style
    }

    fn push_str(&mut self, value: &str) {
        self.builder.text_runs[self.run_index].push_str(value);
        self.builder.buffered_item_text_length += value.len();
    }
}

pub struct InlineSpanBuilder<'a> {
    parent: &'a mut InlineContentBuilder,
    run_index: usize,
    span_index: usize,
    run_text_item_stack_start: usize,
    finish_run_text_on_end: bool,
}

impl<'a> InlineSpanBuilder<'a> {
    fn push_child(&mut self, item: InlineItem) {
        self.parent.items.push(item);
    }

    pub fn current_run_text(&self) -> &str {
        &self.parent.text_runs[self.run_index]
    }

    fn style(&self) -> &ComputedStyle {
        match self.parent.items.get(self.span_index) {
            Some(InlineItem::Span(span)) => &span.style,
            Some(InlineItem::Text(_) | InlineItem::Block(_) | InlineItem::SpanEnd) => {
                unreachable!()
            }
            // This should only happen if the span index is `usize::MAX` in which case
            // we're the root span builder.
            None => &self.parent.root_style,
        }
    }

    pub fn push_text(&mut self, content: &str) {
        if content.is_empty() {
            return;
        }

        let sink = BuilderTextSink {
            style: self.style().clone(),
            builder: self.parent,
            run_index: self.run_index,
            item_stack_start: self.run_text_item_stack_start,
        };

        collapse_text_to(sink, content);
    }

    fn flush_text(&mut self) {
        if self.parent.buffered_item_text_length > 0 {
            let run_end = self.parent.text_runs[self.run_index].len();
            self.parent.text_item_stack.push(TextStackItem {
                item_index: self.parent.items.len(),
                run_index: self.run_index,
                style: self.style().clone(),
            });
            self.push_child(InlineItem::Text(InlineText {
                content_range: run_end - self.parent.buffered_item_text_length..run_end,
            }));
            self.parent.buffered_item_text_length = 0;
        }
    }

    fn push_object_replacement(&mut self) -> usize {
        self.flush_text();
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
        collapse_isolate: bool,
    ) -> InlineSpanBuilder<'_> {
        debug_assert_eq!(self.parent.buffered_item_text_length, 0);
        let span_index = self.parent.items.len();
        self.push_child(InlineItem::Span(InlineSpan { style, kind }));

        InlineSpanBuilder {
            run_index,
            span_index,
            run_text_item_stack_start: if !collapse_isolate {
                self.run_text_item_stack_start
            } else {
                self.parent.text_item_stack.len()
            },
            finish_run_text_on_end: collapse_isolate,
            parent: self.parent,
        }
    }

    pub fn push_span(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.flush_text();
        self.push_span_with(style, InlineSpanKind::Span, self.run_index, false)
    }

    pub fn push_ruby(&mut self, style: ComputedStyle) -> InlineRubyBuilder<'_> {
        let content_index = self.push_object_replacement();
        InlineRubyBuilder {
            span: self.push_span_with(
                style,
                InlineSpanKind::Ruby { content_index },
                self.run_index,
                false,
            ),
            last_was_base: false,
        }
    }

    pub fn push_inline_block(&mut self, block: BlockContainer) {
        let content_index = self.push_object_replacement();
        self.push_child(InlineItem::Block(InlineBlock {
            content_index,
            block: Box::new(block),
        }));
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
        self.flush_text();

        if self.finish_run_text_on_end {
            self.parent.finish_run(self.run_text_item_stack_start);
        }

        if self.span_index != usize::MAX {
            self.parent.items.push(InlineItem::SpanEnd);
        }
    }
}

pub struct InlineRubyBuilder<'a> {
    span: InlineSpanBuilder<'a>,
    last_was_base: bool,
}

impl<'a> InlineRubyBuilder<'a> {
    fn push(&mut self, style: ComputedStyle, annotation: bool) -> InlineSpanBuilder<'_> {
        if self.last_was_base != annotation {
            _ = self.push(ComputedStyle::DEFAULT, !annotation);
        }
        self.last_was_base = !annotation;

        self.span.flush_text();
        let run_index = self.span.push_run();
        self.span.push_span_with(
            style.create_derived(),
            InlineSpanKind::RubyInternal {
                run_index,
                outer_style: style,
            },
            run_index,
            annotation,
        )
    }

    pub fn push_base(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.push(style, false)
    }

    pub fn push_annotation(&mut self, style: ComputedStyle) -> InlineSpanBuilder<'_> {
        self.push(style, true)
    }
}

/// Documentation for white space collapsing implementation.
///
/// This doc comment documents white space collapsing because leaving it undocumented seemed
/// unwise (and doc comments are objectively better than normal comments for this).
///
/// # White space collapsing
///
/// White space collapsing is handled by [`InlineContentBuilder`] in an on-line fashion
/// so that the resulting [`InlineContent`] contains the already-collapsed strings.
///
/// This works as follows:
/// 1. Every time some text is added the builder runs [`collapse_text_to`].
///    This function performs the first pass of white space collapsing, it may also
///    end up removing some characters from the end of previous text items due to
///    this process. This is fine since those will always be at the end of their text
///    run (though see remark about fully removing text items below).
/// 2. After a span that has the [`finish_run_text_on_end`] flag set ends (or when
///    finalizing the builder) [`finish_text_run_collapse`] is called which performs
///    the second and final pass of white space collapsing. This also has the ability
///    to remove characters from the end of the run.
///
///    The main differences between the implementation of this pass and the previous one are:
///    1. This pass does not have to care about "buferred text" of the text item currently
///       being constructed and only works on already-emitted text items.
///    2. It pops all remaining text items for this collapsing run from the text item stack
///       (while doing things like replacing all `\n`s with ` ` in ones that are `collapse`).
///
/// Note that the "run" term above is used very loosely because *collapsing runs*
/// do not map 1-1 to [`text_runs`]. This is because ruby bases live in isolated *text runs*
/// but **are not** separately isolated *collapsing runs* (i.e. they collapse with their
/// surrounding text). I don't have a spec reference for this but browsers do it like this
/// (though Chromium refuses to if the base has an annotation... yeah).
///
/// What items are part of the current *collapsing run* is tracked by [`InlineContentBuilder::text_item_stack`],
/// specifically:
/// 1. Each span builder keeps track of where in the stack the actual stack for its
///    particular collapsing run starts (when pushing an isolated builder this is
///    set to the top of the stack).
/// 2. The operations above only operate on items above this start point to prevent
///    interfering with other collapsing runs. The final collapsing pass truncates
///    the stack back to this point so the parent builder may start using it again.
/// 3. This stack contains [`TextStackItem`]s which, along with their associated text item,
///    store some information needed during collapsing so it doesn't have to be stored in
///    [`InlineText`] and can instead be thrown away immediately once unnecessary.
///
/// ### Tombstones
///
/// Since the above process may end up effectively removing some text items, we introduce
/// a "tombstone" mechanism where text items whose [`content_range`] is empty are treated
/// as "tombstones". These are removed from the final item array when finalizing the
/// builder.
///
/// To ensure we know how many live items to allocate space for when finalizing, the number
/// of dead items is tracked in [`n_dead_items`] and `debug_assert!`ed to be correct later.
/// The above collapsing steps handle this by only ever removing bytes via
/// [`InlineContentBuilder::pop_bytes_from_last_text_item`] which takes care of this bookkeeping.
///
/// [`text_runs`]: InlineContentBuilder::text_runs
/// [`n_dead_items`]: InlineContentBuilder::n_dead_items
/// [`content_range`]: InlineText::content_range
/// [`finish_run_text_on_end`]: InlineSpanBuilder::finish_run_text_on_end
#[cfg(doc)]
struct WhiteSpaceCollapseInternalDoc;

impl WhiteSpaceCollapse {
    fn has_collapsible_spaces(&self) -> bool {
        matches!(self, Self::Collapse | Self::PreserveBreaks)
    }
}

fn collapse_text_to(mut sink: BuilderTextSink, text: &str) {
    let wsc = sink.style().white_space_collapse();
    match wsc {
        // > If white-space-collapse is set to collapse or preserve-breaks, white space characters are considered collapsible and are processed by performing the following steps:
        WhiteSpaceCollapse::Collapse | WhiteSpaceCollapse::PreserveBreaks => {
            let mut text = text;
            while let Some(mut next) = text.bytes().position(|b| matches!(b, b'\t' | b' ' | b'\n'))
            {
                sink.push_str(&text[..next]);

                match text.as_bytes()[next] {
                    // > 3. Every collapsible tab is converted to a collapsible space (U+0020).
                    b' ' | b'\t' => {
                        // > 4. Any collapsible space immediately following another collapsible space—​even one outside the boundary of the inline containing that space, provided both spaces are within the same inline formatting context—​is collapsed to have zero advance width. (It is invisible, but retains its soft wrap opportunity, if any.)
                        // NOTE: This has an edge case where a break opportunity must be preserved
                        //       even if we skip this case but we don't currently support suppressing
                        //       a break opportunity on a space so currently this isn't ever necessary
                        //       since the previous space will have a break opportunity.
                        if !sink.peek_prev().is_some_and(|(b, s)| {
                            matches!(b, b' ' | b'\n')
                                && s.white_space_collapse().has_collapsible_spaces()
                        }) {
                            sink.push_str(" ");
                        }
                        next += 1;
                    }
                    b'\n' => {
                        // > 1.1. Any sequence of collapsible spaces and tabs immediately preceding a segment break is removed.
                        // Tabs have already been replaced with spaces by this point so
                        // only spaces have to be removed.
                        while sink.peek_prev().is_some_and(|(b, s)| {
                            b == b' ' && s.white_space_collapse().has_collapsible_spaces()
                        }) {
                            sink.pop_prev();
                        }

                        // > 2. Collapsible segment breaks are transformed for rendering according to the segment break transformation rules.
                        let mut remove_break = false;
                        // > 2.5. When white-space-collapse is collapse, segment breaks are collapsible, and are collapsed as follows:
                        if matches!(wsc, WhiteSpaceCollapse::Collapse) {
                            let peek = sink.peek_prev();
                            // > 2.5.1. First, any collapsible segment break immediately following another collapsible segment break is removed.
                            remove_break = peek.is_some_and(|(b, s)| {
                                let c = s.white_space_collapse();
                                b == b'\n' && matches!(c, WhiteSpaceCollapse::Collapse)
                            });
                            // > 2.5.2. Then any remaining segment break is either transformed into a space (U+0020) or removed depending on the context before and after the break. The rules for this operation are UA-defined in this level.
                            // This is the first part of this UA-defined operation which we handle as follows:
                            // - Remove line breaks at the start of the inline (done here)
                            // - Remove line breaks at the end of the inline (done later)
                            // - Transform remaining line breaks into spaces (done later)
                            remove_break |= peek.is_none();
                        }
                        if !remove_break {
                            sink.push_str("\n");
                        }
                        next += 1;

                        // TODO: Technically we may fail to remove subsequent whitespace if
                        //       it's in another span and the line break was removed due to
                        //       being at the start of the inline. This should be okay once we have
                        //       § 4.1.2. implemented since it removes collapsible spaces at the
                        //       beggining of every line but it still means more potential for
                        //       `InlineContent`s that are not equal but equivalent.

                        // > 1.2. Any sequence of collapsible spaces and tabs immediately following a segment break is removed.
                        next = text[next..]
                            .bytes()
                            .position(|b| !matches!(b, b' ' | b'\t'))
                            .map_or(text.len(), |i| next + i);
                    }
                    _ => unreachable!(),
                }

                text = &text[next..];
            }
            sink.push_str(text);
        }
        WhiteSpaceCollapse::Preserve => sink.push_str(text),
    }
}

fn finish_text_run_collapse(builder: &mut InlineContentBuilder, text_stack_start: usize) {
    // Remove trailing line breaks as part of 2.5.2 from `collapse_text_to`.
    while let Some((text, _)) = builder
        .last_text_item_content(text_stack_start)
        .filter(|(_, style)| matches!(style.white_space_collapse(), WhiteSpaceCollapse::Collapse))
    {
        let to_remove = text.bytes().rev().take_while(|&x| x == b'\n').count();
        let removed_all = to_remove == text.len();
        builder.pop_bytes_from_last_text_item(to_remove);
        if !removed_all {
            break;
        }
    }

    // Transform remaining line breaks into spaces as part of 2.5.2 from `collapse_text_to`.
    while let Some((run, range, style)) = builder.pop_text_item_with_content_mut(text_stack_start) {
        match style.white_space_collapse() {
            WhiteSpaceCollapse::Collapse => {
                let mut current = range.start;
                while let Some(next) = run[current..range.end].find('\n').map(|i| current + i) {
                    current = next + 1;
                    run.replace_range(next..current, " ");
                }
            }
            WhiteSpaceCollapse::PreserveBreaks | WhiteSpaceCollapse::Preserve => (),
        }
    }
}
