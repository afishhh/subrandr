use rasterize::color::BGRA8;
use util::math::{I26Dot6, Point2, Rect2};

use crate::{
    layout::{
        inline::{InlineContentFragment, InlineItemFragment, SpanFragment, TextFragment},
        FixedL, FragmentBox, Point2L,
    },
    style::ComputedStyle,
};

mod paint_op;
pub use paint_op::*;

pub struct DisplayPass<'r> {
    pub output: PaintOpBuilder<'r>,
}

struct LineDecoration {
    baseline_offset: FixedL,
    thickness: FixedL,
    color: BGRA8,
    /// Used to determine paint order: https://drafts.csswg.org/css-text-decor/#painting-order.
    kind: LineDecorationKind,
}

enum LineDecorationKind {
    Underline,
    LineThrough,
}

fn round_y(mut p: Point2L) -> Point2L {
    p.y = p.y.round();
    p
}

impl DisplayPass<'_> {
    fn display_line_decoration(
        &mut self,
        x0: FixedL,
        x1: FixedL,
        baseline_y: I26Dot6,
        decoration: &LineDecoration,
    ) {
        let decoration_y = baseline_y + decoration.baseline_offset;

        self.output.push_rect_fill(
            Rect2::new(
                Point2::new(x0, decoration_y),
                Point2::new(x1, decoration_y + decoration.thickness),
            ),
            decoration.color,
        );
    }

    fn display_text(
        &mut self,
        pos: Point2L,
        fragment: &TextFragment,
        decorations: &[LineDecoration],
    ) {
        // TODO: This should also draw an offset underline I think and possibly strike through?
        for shadow in fragment.style.text_shadows().iter().rev() {
            if shadow.color.a > 0 {
                let stddev = if shadow.blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    shadow.blur_radius / 2
                } else {
                    FixedL::ZERO
                };

                self.output.push_text(Text::from_fragment(
                    round_y(pos + shadow.offset),
                    fragment,
                    TextKind::Shadow {
                        blur_stddev: stddev,
                        color: shadow.color,
                    },
                ));
            }
        }

        let text_end_x = {
            let mut end_x = pos.x;

            for glyph in fragment.glyphs().iter_glyphs_visual() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::Underline))
        {
            self.display_line_decoration(pos.x, text_end_x, pos.y, decoration);
        }

        let color = fragment.style.color();
        if color.a > 0 {
            self.output.push_text(Text::from_fragment(
                pos,
                fragment,
                TextKind::Normal { mono_color: color },
            ));
        }

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::LineThrough))
        {
            self.display_line_decoration(pos.x, text_end_x, pos.y, decoration);
        }
    }

    fn display_inline_background(
        &mut self,
        mut pos: Point2L,
        style: &ComputedStyle,
        fragment_box: &FragmentBox,
    ) {
        let background = style.background_color();
        if background.a > 0 {
            // This seems like reasonable rounding for inline backgrounds because:
            // 1. Adjacent backgrounds will not overlap or have gaps unless they are less than 1 pixel wide.
            // 2. It rounds the background box to whole integers avoiding conflation artifacts.
            // Not sure what browsers do here though maybe that's worthwhile to investigate.
            let mut fbox = *fragment_box;
            fbox.content_size.x = (fbox.content_size.x + pos.x.fract()).floor();
            fbox.content_size.y = (fbox.content_size.y + pos.y.fract()).round();
            pos.x = pos.x.floor();
            pos.y = pos.y.round();

            self.output
                .push_rect_fill(fbox.padding_box().translate(pos.to_vec()), background);
        }
    }

    fn display_inline_item_fragment_background(
        &mut self,
        pos: Point2L,
        fragment: &InlineItemFragment,
    ) {
        match fragment {
            InlineItemFragment::Span(span) => {
                self.display_inline_background(pos, &span.style, &span.fbox);

                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    self.display_inline_item_fragment_background(child_pos, child);
                }
            }
            InlineItemFragment::Text(_) => {}
            InlineItemFragment::Ruby(ruby) => {
                for &(base_offset, ref base, annotation_offset, ref annotation) in &ruby.content {
                    let base_pos = pos + ruby.fbox.content_offset() + base_offset;
                    self.display_inline_background(base_pos, &base.style, &base.fbox);
                    for &(base_item_offset, ref base_item) in &base.children {
                        self.display_inline_item_fragment_background(
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                        );
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    self.display_inline_background(
                        annotation_pos,
                        &annotation.style,
                        &annotation.fbox,
                    );
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.display_inline_item_fragment_background(
                            annotation_pos
                                + annotation.fbox.content_offset()
                                + annotation_item_offset,
                            annotation_item,
                        );
                    }
                }
            }
        }
    }

    fn display_inline_item_fragment_content(
        &mut self,
        pos: Point2L,
        fragment: &InlineItemFragment,
        current_decorations: &mut Vec<LineDecoration>,
    ) {
        let previous_decoration_count = current_decorations.len();
        match fragment {
            InlineItemFragment::Span(SpanFragment {
                style,
                primary_font,
                ..
            }) => {
                let font_metrics = primary_font.metrics();
                let decoration = style.text_decoration();

                if decoration.underline {
                    current_decorations.push(LineDecoration {
                        baseline_offset: font_metrics.underline_top_offset,
                        thickness: font_metrics.underline_thickness,
                        color: decoration.underline_color,
                        kind: LineDecorationKind::Underline,
                    });
                }

                if decoration.line_through {
                    current_decorations.push(LineDecoration {
                        baseline_offset: font_metrics.strikeout_top_offset,
                        thickness: font_metrics.strikeout_thickness,
                        color: decoration.line_through_color,
                        kind: LineDecorationKind::LineThrough,
                    });
                }
            }
            // TODO: Technically ruby containers can also have decorations but we don't make
            //       use of that right now, and don't store font metrics in the fragment anyway.
            //       Decorations on ruby bases and annotations probably have the same problem.
            InlineItemFragment::Ruby(_) => (),
            InlineItemFragment::Text(_) => (),
        }

        match fragment {
            InlineItemFragment::Span(span) => {
                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    self.display_inline_item_fragment_content(
                        child_pos,
                        child,
                        current_decorations,
                    );
                }
            }
            InlineItemFragment::Text(text) => {
                self.display_text(
                    round_y(pos + text.baseline_offset),
                    text,
                    current_decorations,
                );
            }
            InlineItemFragment::Ruby(ruby) => {
                for &(base_offset, ref base, annotation_offset, ref annotation) in &ruby.content {
                    let base_pos = pos + ruby.fbox.content_offset() + base_offset;
                    for &(base_item_offset, ref base_item) in &base.children {
                        self.display_inline_item_fragment_content(
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                            current_decorations,
                        );
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.display_inline_item_fragment_content(
                            annotation_pos
                                + annotation.fbox.content_offset()
                                + annotation_item_offset,
                            annotation_item,
                            &mut Vec::new(),
                        );
                    }
                }
            }
        }

        current_decorations.truncate(previous_decoration_count);
    }

    pub fn display_inline_content_fragment(
        &mut self,
        pos: Point2L,
        fragment: &InlineContentFragment,
    ) {
        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                self.display_inline_item_fragment_background(current, item);
            }
        }

        let mut decoration_stack = Vec::new();
        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                self.display_inline_item_fragment_content(current, item, &mut decoration_stack);
            }
        }
    }
}
