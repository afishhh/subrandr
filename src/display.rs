use std::rc::Rc;

use rasterize::{
    color::BGRA8,
    scene::{Bitmap, BitmapFilter, DeferredBitmaps, FilledRect, SceneNode},
};
use util::math::{I26Dot6, Point2, Rect2};

use crate::{
    layout::{
        block::{BlockContainerFragment, BlockContainerFragmentContent},
        inline::{InlineContentFragment, InlineItemFragment, RubyFragment, TextFragment},
        FixedL, FragmentBox, Point2L, Rect2L,
    },
    style::ComputedStyle,
    text::{self, FontMetrics, GlyphCache},
};

mod decoration;
use decoration::*;

pub struct DisplayPass<'r> {
    pub output: &'r mut Vec<SceneNode>,
    decoration_tracker: DecorationTracker,
}

struct DisplayContext<'c> {
    output: &'c mut Vec<SceneNode>,
    decoration_ctx: DecorationContext<'c>,
}

fn round_y(mut p: Point2L) -> Point2L {
    p.y = p.y.round();
    p
}

impl<'r> DisplayPass<'r> {
    pub fn new(output: &'r mut Vec<SceneNode>) -> Self {
        Self {
            output,
            decoration_tracker: DecorationTracker::new(),
        }
    }

    fn root_ctx(&mut self) -> DisplayContext<'_> {
        DisplayContext {
            output: &mut *self.output,
            decoration_ctx: self.decoration_tracker.root(),
        }
    }

    pub fn display_inline_content_fragment(
        &mut self,
        pos: Point2L,
        fragment: &InlineContentFragment,
    ) {
        self.root_ctx()
            .display_inline_content_fragment(pos, fragment);
    }

    pub fn display_block_container_fragment(
        &mut self,
        pos: Point2L,
        fragment: &BlockContainerFragment,
    ) {
        self.root_ctx()
            .display_block_container_fragment(pos, fragment);
    }
}

impl DisplayContext<'_> {
    fn push_rect_fill(output: &mut Vec<SceneNode>, rect: Rect2L, color: BGRA8) {
        output.push(SceneNode::FilledRect(FilledRect { rect, color }));
    }

    fn push_text(
        &mut self,
        pos: Point2L,
        fragment: &TextFragment,
        shadow: Option<f32>,
        color: BGRA8,
    ) {
        let fragment = fragment.clone();
        self.output
            .push(SceneNode::DeferredBitmaps(DeferredBitmaps {
                to_bitmaps: Rc::new(move |rasterizer, user_data| {
                    let glyph_cache = user_data
                        .downcast_ref::<GlyphCache>()
                        .expect("to_bitmaps user_data is not a GlyphCache?");

                    let mut bitmaps = Vec::new();
                    let glyphs = text::render(
                        glyph_cache,
                        rasterizer,
                        pos.x.fract(),
                        pos.y.fract(),
                        shadow.unwrap_or(0.0),
                        &mut fragment.glyphs().iter_glyphs_visual(),
                    )?;

                    let base_pos = Point2::new(pos.x.floor_to_inner(), pos.y.floor_to_inner());
                    for glyph in glyphs {
                        bitmaps.push(Bitmap {
                            pos: base_pos + glyph.offset,
                            texture: glyph.texture,
                            filter: if shadow.is_some() {
                                Some(BitmapFilter::ExtractAlpha)
                            } else {
                                None
                            },
                            color,
                        });
                    }

                    Ok(bitmaps)
                }),
            }));
    }

    fn display_line_decoration(
        output: &mut Vec<SceneNode>,
        x0: FixedL,
        x1: FixedL,
        baseline_y: I26Dot6,
        decoration: &ActiveDecoration,
    ) {
        let decoration_y = baseline_y + decoration.baseline_offset;

        Self::push_rect_fill(
            output,
            Rect2::new(
                Point2::new(x0, decoration_y),
                Point2::new(x1, decoration_y + decoration.thickness),
            ),
            decoration.color,
        );
    }

    fn display_text(&mut self, pos: Point2L, baseline_y: FixedL, fragment: &TextFragment) {
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

                self.push_text(
                    round_y(pos + shadow.offset),
                    fragment,
                    Some(stddev.into_f32()),
                    shadow.color,
                );
            }
        }

        let text_end_x = {
            let mut end_x = pos.x;

            for glyph in fragment.glyphs().iter_glyphs_visual() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        // Decorations are drawn in the order specified by https://drafts.csswg.org/css-text-decor/#painting-order
        for decoration in self
            .decoration_ctx
            .active_decorations()
            .iter()
            .filter(|x| matches!(x.kind, DecorationKind::Underline))
        {
            Self::display_line_decoration(self.output, pos.x, text_end_x, baseline_y, decoration);
        }

        let color = fragment.style.color();
        if color.a > 0 {
            self.push_text(pos, fragment, None, color);
        }

        for decoration in self
            .decoration_ctx
            .active_decorations()
            .iter()
            .filter(|x| matches!(x.kind, DecorationKind::LineThrough))
        {
            Self::display_line_decoration(self.output, pos.x, text_end_x, baseline_y, decoration);
        }
    }

    fn enter_box(
        &mut self,
        style: &ComputedStyle,
        font_metrics_if_inline: Option<&FontMetrics>,
    ) -> DisplayContext<'_> {
        DisplayContext {
            output: &mut *self.output,
            decoration_ctx: self
                .decoration_ctx
                .push_decorations(style, font_metrics_if_inline),
        }
    }

    fn suspend_decorations(&mut self) -> DisplayContext<'_> {
        DisplayContext {
            output: &mut *self.output,
            decoration_ctx: self.decoration_ctx.suspend_active(),
        }
    }

    fn display_background(
        &mut self,
        pos: Point2L,
        style: &ComputedStyle,
        fragment_box: &FragmentBox,
    ) {
        let background = style.background_color();
        if background.a > 0 {
            // This seems like reasonable rounding for inline backgrounds because:
            // 1. Adjacent backgrounds will not overlap or have gaps unless they are less than 1 pixel wide.
            // 2. It rounds the background box to whole integers avoiding conflation artifacts.
            // Not sure what browsers do here though maybe that's worthwhile to investigate.
            let mut bg = fragment_box.padding_box().translate(pos.to_vec());
            bg.max.x = bg.max.x.floor();
            bg.max.y = bg.max.y.round();
            bg.min.x = bg.min.x.floor();
            bg.min.y = bg.min.y.round();
            Self::push_rect_fill(self.output, bg, background);
        }
    }

    fn display_ruby_fragment(&mut self, pos: Point2L, baseline_y: FixedL, fragment: &RubyFragment) {
        let content_pos = pos + fragment.fbox.content_offset();
        let mut last_x = pos.x;
        for &(base_offset, ref base, annotation_offset, ref annotation) in &fragment.content {
            {
                let base_pos = content_pos + base_offset;
                self.display_background(base_pos, &base.style, &base.fbox);
                // Careful spec reading suggests ruby containers only *propagate* decorations:
                // https://drafts.csswg.org/css-text-decor/#line-decoration
                let mut ruby_scope = self.enter_box(&fragment.style, None);
                let mut base_scope =
                    ruby_scope.enter_box(&base.style, Some(base.primary_font.metrics()));

                let initial_base_padding_end =
                    base_pos.x + base.children.first().map_or(FixedL::ZERO, |x| x.0.x);
                for decoration in base_scope.decoration_ctx.active_decorations() {
                    Self::display_line_decoration(
                        base_scope.output,
                        last_x,
                        initial_base_padding_end,
                        baseline_y,
                        decoration,
                    );
                }

                for &(base_item_offset, ref base_item) in &base.children {
                    base_scope.display_inline_item_fragment(
                        base_pos + base.fbox.content_offset() + base_item_offset,
                        baseline_y,
                        base_item,
                    );
                }

                let final_base_padding_end =
                    base_pos.x + base.children.last().map_or(FixedL::ZERO, |x| x.0.x);
                let base_end_x = base_pos.x + base.fbox.size_for_layout().x;
                for decoration in base_scope.decoration_ctx.active_decorations() {
                    Self::display_line_decoration(
                        base_scope.output,
                        final_base_padding_end,
                        base_end_x,
                        baseline_y,
                        decoration,
                    );
                }

                last_x = base_end_x;
            }

            {
                let annotation_pos = pos + fragment.fbox.content_offset() + annotation_offset;
                let mut suspend_scope = self.suspend_decorations();
                let mut annotation_scope = suspend_scope
                    .enter_box(&annotation.style, Some(annotation.primary_font.metrics()));
                annotation_scope.display_background(
                    annotation_pos,
                    &annotation.style,
                    &annotation.fbox,
                );
                let annotation_content_offset = annotation_pos + annotation.fbox.content_offset();
                for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                    annotation_scope.display_inline_item_fragment(
                        annotation_content_offset + annotation_item_offset,
                        annotation_content_offset.y + annotation.baseline_y,
                        annotation_item,
                    );
                }
            }
        }
    }

    fn display_inline_item_fragment(
        &mut self,
        pos: Point2L,
        baseline_y: FixedL,
        fragment: &InlineItemFragment,
    ) {
        match fragment {
            InlineItemFragment::Span(span) => {
                self.display_background(pos, &span.style, &span.fbox);

                let mut scope = self.enter_box(&span.style, Some(span.primary_font.metrics()));
                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    scope.display_inline_item_fragment(child_pos, baseline_y, child);
                }
            }
            InlineItemFragment::Text(text) => {
                self.display_text(round_y(pos), baseline_y, text);
            }
            InlineItemFragment::Ruby(ruby) => self.display_ruby_fragment(pos, baseline_y, ruby),
            InlineItemFragment::Block(block) => self.display_block_container_fragment(pos, block),
        }
    }

    fn display_inline_content_fragment(&mut self, pos: Point2L, fragment: &InlineContentFragment) {
        let mut scope = self.enter_box(&fragment.style, Some(&fragment.primary_font_metrics));

        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;
            let baseline_y = (current.y + line.baseline_y).round();

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                scope.display_inline_item_fragment(current, baseline_y, item)
            }
        }
    }

    fn display_block_container_fragment(
        &mut self,
        pos: Point2L,
        fragment: &BlockContainerFragment,
    ) {
        self.display_background(pos, &fragment.style, &fragment.fbox);

        let content_pos = pos + fragment.fbox.content_offset();
        let mut scope = self.enter_box(&fragment.style, None);
        match &fragment.content {
            &BlockContainerFragmentContent::Inline(offset, ref inline) => {
                scope.display_inline_content_fragment(content_pos + offset, inline);
            }
            BlockContainerFragmentContent::Block(children) => {
                for &(child_off, ref child) in children {
                    scope.display_block_container_fragment(content_pos + child_off, child);
                }
            }
        }
    }
}
