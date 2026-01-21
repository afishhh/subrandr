use super::InlineContentBuilder;
use crate::style::computed::WhiteSpaceCollapse;

impl WhiteSpaceCollapse {
    fn has_collapsible_spaces(&self) -> bool {
        matches!(self, Self::Collapse)
    }
}

pub fn collapse_text_to(mut sink: super::BuilderTextSink, text: &str) {
    match sink.style().white_space_collapse() {
        // > If white-space-collapse is set to collapse or preserve-breaks, white space characters are considered collapsible and are processed by performing the following steps:
        WhiteSpaceCollapse::Collapse => {
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

                        // >  2. Collapsible segment breaks are transformed for rendering according to the segment break transformation rules.
                        // >   2.5. When white-space-collapse is collapse, segment breaks are collapsible, and are collapsed as follows:
                        // >   2.5.1. First, any collapsible segment break immediately following another collapsible segment break is removed.
                        let mut remove_break = sink.peek_prev().is_some_and(|(b, s)| {
                            b == b'\n'
                                && matches!(s.white_space_collapse(), WhiteSpaceCollapse::Collapse)
                        });
                        // >   2.5.2. Then any remaining segment break is either transformed into a space (U+0020) or removed depending on the context before and after the break. The rules for this operation are UA-defined in this level.
                        // This is the first part of this UA-defined operation which we handle as follows:
                        // - Remove line breaks at the start of the inline (done here)
                        // - Remove line breaks at the end of the inline (done after main loop)
                        // - Transform remaining line breaks into spaces (done after main loop)
                        remove_break |= sink.peek_prev().is_none();
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

pub fn finish_text_run_collapse(builder: &mut InlineContentBuilder, text_stack_start: usize) {
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
            WhiteSpaceCollapse::Preserve => (),
        }
    }
}
