use rasterize::{
    color::BGRA8,
    scene::{SceneContentBuilder, SceneFilter},
    Rasterizer,
};
use util::math::{I26Dot6, Rect2};

use crate::{
    layout::{
        block::{BlockContainerFragment, BlockContainerFragmentContent},
        inline::{Baseline, InlineContentFragment, InlineItemFragment, RubyFragment, TextFragment},
        FixedL, FragmentBox, Point2L, Vec2L, Vec2LW, Vec2WritingModeExt as _,
    },
    style::{
        computed::{ToPhysicalPixels, WritingMode},
        ComputedStyle,
    },
    text::{self, FontMetrics, GlyphCache},
};

mod decoration;
use decoration::*;

pub struct DisplayPass<'r> {
    pub output: SceneContentBuilder<'r>,
    dpi: u32,
    glyph_cache: &'r GlyphCache,
    rasterizer: &'r mut dyn Rasterizer,
    decoration_tracker: DecorationTracker,
}

pub type DisplayError = text::GlyphDisplayError;

struct DisplayContext<'c> {
    output: SceneContentBuilder<'c>,
    dpi: u32,
    glyph_cache: &'c GlyphCache,
    rasterizer: &'c mut dyn Rasterizer,
    decoration_ctx: DecorationContext<'c>,
}

fn round_block(mut p: Point2L, writing_mode: WritingMode) -> Point2L {
    *p.block_mut(writing_mode) = p.block_mut(writing_mode).round();
    p
}

impl<'r> DisplayPass<'r> {
    pub fn new(
        output: SceneContentBuilder<'r>,
        dpi: u32,
        glyph_cache: &'r GlyphCache,
        rasterizer: &'r mut dyn Rasterizer,
    ) -> Self {
        Self {
            output,
            dpi,
            glyph_cache,
            rasterizer,
            decoration_tracker: DecorationTracker::new(),
        }
    }

    fn root_ctx(&mut self) -> DisplayContext<'_> {
        DisplayContext {
            output: self.output.child(),
            dpi: self.dpi,
            glyph_cache: self.glyph_cache,
            rasterizer: self.rasterizer,
            decoration_ctx: self.decoration_tracker.root(),
        }
    }

    pub fn display_inline_content_fragment(
        &mut self,
        pos: Point2L,
        fragment: &InlineContentFragment,
    ) -> Result<(), DisplayError> {
        self.root_ctx()
            .display_inline_content_fragment(pos, fragment)
    }

    pub fn display_block_container_fragment(
        &mut self,
        pos: Point2L,
        fragment: &BlockContainerFragment,
    ) -> Result<(), DisplayError> {
        self.root_ctx()
            .display_block_container_fragment(pos, fragment)
    }
}

impl DisplayContext<'_> {
    fn push_text(
        &mut self,
        pos: Point2L,
        fragment: &TextFragment,
        shadow: Option<I26Dot6>,
        color: BGRA8,
    ) -> Result<(), DisplayError> {
        let mut output = self.output.with_translation(pos.to_vec());
        let scene_filter = shadow.map(|blur_radius| SceneFilter::ExtractAlpha {
            blur_stddev: blur_radius,
        });

        for (font, glyph) in fragment.glyphs.iter_glyphs_visual() {
            let font_metrics = font.metrics();
            let glyph_transform = fragment.glyph_transform();
            let mut offset = glyph_transform * Vec2L::new(glyph.x_offset, -glyph.y_offset);
            let advance = glyph_transform * Vec2L::new(glyph.x_advance, -glyph.y_advance);

            // HarfBuzz gives us glyphs aligned on the appropriate baseline for the passed
            // direction. The baseline we want to align on does not always match the above
            // so we need to re-align the glyphs in those cases.
            // TODO: This may give suboptimal results when the baselines are synthetic if
            //       our calculations don't match HarfBuzz.
            let glyph_baseline = if !fragment.is_typographic_mode_vertical() {
                Baseline::Alphabetic
            } else {
                Baseline::Central
            };
            let dominant_baseline = fragment.align_to;
            if glyph_baseline != dominant_baseline {
                let vertical = fragment.is_typographic_mode_vertical();
                let correction = dominant_baseline.metrics(font_metrics, vertical).ascender
                    - glyph_baseline.metrics(font_metrics, vertical).ascender;
                if fragment.is_vertical() {
                    offset.x += correction;
                } else {
                    offset.y += correction;
                }
            }

            output.with_translation(offset).try_subscene(
                scene_filter,
                color,
                |subpixel_pos, _| {
                    let (offset_value, offset_axis_is_y) = if fragment.is_vertical() {
                        (subpixel_pos.y, true)
                    } else {
                        (subpixel_pos.x, false)
                    };

                    font.glyph_subscene(
                        self.glyph_cache,
                        glyph.index,
                        offset_value,
                        offset_axis_is_y,
                        glyph_transform,
                        self.rasterizer,
                    )
                    .map(|x| x.0.clone())
                },
            )?;

            output.apply_translation(advance);
        }

        Ok(())
    }

    fn display_line_decoration(
        output: &mut SceneContentBuilder,
        inline_start: FixedL,
        inline_end: FixedL,
        baseline_pos: FixedL,
        decoration: &ActiveDecoration,
        writing_mode: WritingMode,
    ) {
        let decoration_block_offset = baseline_pos + decoration.baseline_offset;
        let min = Vec2LW::new(decoration_block_offset, inline_start)
            .to_physical(writing_mode)
            .to_point();
        let max = Vec2LW::new(decoration_block_offset + decoration.thickness, inline_end)
            .to_physical(writing_mode)
            .to_point();

        output.filled_rect(Rect2::new(min, max), decoration.color);
    }

    fn display_text(
        &mut self,
        pos: Point2L,
        baseline_off: FixedL,
        fragment: &TextFragment,
        writing_mode: WritingMode,
    ) -> Result<(), DisplayError> {
        // TODO: This should also draw an offset underline I think and possibly strike through?
        for shadow in fragment.style.text_shadows().iter().rev() {
            let color = shadow.color.to_used(&fragment.style);
            if color.a > 0 {
                let blur_radius = shadow.blur_radius.to_physical_pixels(self.dpi);
                let stddev = if blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    blur_radius / 2
                } else {
                    FixedL::ZERO
                };

                self.push_text(
                    round_block(
                        pos + shadow.offset.to_physical_pixels(self.dpi),
                        writing_mode,
                    ),
                    fragment,
                    Some(stddev),
                    color,
                )?;
            }
        }

        let (text_inline_start, text_inline_end) = if writing_mode.is_horizontal() {
            (pos.x, pos.x + fragment.inline_size)
        } else {
            (pos.y, pos.y + fragment.inline_size)
        };

        // Decorations are drawn in the order specified by https://drafts.csswg.org/css-text-decor/#painting-order
        for decoration in self
            .decoration_ctx
            .active_decorations()
            .iter()
            .filter(|x| matches!(x.kind, DecorationKind::Underline))
        {
            Self::display_line_decoration(
                &mut self.output,
                text_inline_start,
                text_inline_end,
                baseline_off,
                decoration,
                writing_mode,
            );
        }

        let color = fragment.style.color();
        if color.a > 0 {
            self.push_text(pos, fragment, None, color)?;
        }

        for decoration in self
            .decoration_ctx
            .active_decorations()
            .iter()
            .filter(|x| matches!(x.kind, DecorationKind::LineThrough))
        {
            Self::display_line_decoration(
                &mut self.output,
                text_inline_start,
                text_inline_end,
                baseline_off,
                decoration,
                writing_mode,
            );
        }

        Ok(())
    }

    fn enter_box(
        &mut self,
        style: &ComputedStyle,
        font_metrics_if_inline: Option<(&FontMetrics, WritingMode)>,
    ) -> DisplayContext<'_> {
        DisplayContext {
            output: self.output.child(),
            dpi: self.dpi,
            glyph_cache: self.glyph_cache,
            rasterizer: self.rasterizer,
            decoration_ctx: self
                .decoration_ctx
                .push_decorations(style, font_metrics_if_inline),
        }
    }

    fn suspend_decorations(&mut self) -> DisplayContext<'_> {
        DisplayContext {
            output: self.output.child(),
            dpi: self.dpi,
            glyph_cache: self.glyph_cache,
            rasterizer: self.rasterizer,
            decoration_ctx: self.decoration_ctx.suspend_active(),
        }
    }

    fn display_background(
        &mut self,
        pos: Point2L,
        style: &ComputedStyle,
        fragment_box: &FragmentBox,
    ) {
        let background = style.background_color().to_used(style);
        if style.visibility().is_visible() && background.a > 0 {
            // This seems like reasonable rounding for inline backgrounds because:
            // 1. Adjacent backgrounds will not overlap or have gaps unless they are less than 1 pixel wide.
            // 2. It rounds the background box to whole integers avoiding conflation artifacts.
            // Not sure what browsers do here though maybe that's worthwhile to investigate.
            let mut bg = fragment_box.padding_box().translate(pos.to_vec());
            bg.max.x = bg.max.x.floor();
            bg.max.y = bg.max.y.round();
            bg.min.x = bg.min.x.floor();
            bg.min.y = bg.min.y.round();
            self.output.filled_rect(bg, background);
        }
    }

    fn display_ruby_fragment(
        &mut self,
        pos: Point2L,
        baseline_pos: FixedL,
        fragment: &RubyFragment,
        writing_mode: WritingMode,
    ) -> Result<(), DisplayError> {
        let content_pos = pos + fragment.fbox.content_offset();
        let mut last_inline = pos.inline(writing_mode);
        for &(base_offset, ref base, annotation_offset, ref annotation) in &fragment.content {
            {
                let base_pos = content_pos + base_offset;
                self.display_background(base_pos, &base.style, &base.fbox);
                // Careful spec reading suggests ruby containers only *propagate* decorations:
                // https://drafts.csswg.org/css-text-decor/#line-decoration
                let mut ruby_scope = self.enter_box(&fragment.style, None);
                let mut base_scope = ruby_scope.enter_box(
                    &base.style,
                    Some((base.primary_font.metrics(), writing_mode)),
                );

                if base.style.visibility().is_visible() {
                    let initial_base_padding_end = base_pos.inline(writing_mode)
                        + base
                            .children
                            .first()
                            .map_or(FixedL::ZERO, |x| x.0.inline(writing_mode));
                    for decoration in base_scope.decoration_ctx.active_decorations() {
                        Self::display_line_decoration(
                            &mut base_scope.output,
                            last_inline,
                            initial_base_padding_end,
                            baseline_pos,
                            decoration,
                            writing_mode,
                        );
                    }
                }

                for &(base_item_offset, ref base_item) in &base.children {
                    base_scope.display_inline_item_fragment(
                        base_pos + base.fbox.content_offset() + base_item_offset,
                        baseline_pos,
                        base_item,
                        writing_mode,
                    )?;
                }

                let base_inline_end =
                    base_pos.inline(writing_mode) + base.fbox.inline_size(writing_mode);
                if base.style.visibility().is_visible() {
                    let final_base_padding_end = base_pos.inline(writing_mode)
                        + base
                            .children
                            .last()
                            .map_or(FixedL::ZERO, |x| x.0.inline(writing_mode));
                    for decoration in base_scope.decoration_ctx.active_decorations() {
                        Self::display_line_decoration(
                            &mut base_scope.output,
                            final_base_padding_end,
                            base_inline_end,
                            baseline_pos,
                            decoration,
                            writing_mode,
                        );
                    }
                }

                last_inline = base_inline_end;
            }

            {
                let annotation_pos = pos + fragment.fbox.content_offset() + annotation_offset;
                let mut suspend_scope = self.suspend_decorations();
                let mut annotation_scope = suspend_scope.enter_box(
                    &annotation.style,
                    Some((annotation.primary_font.metrics(), writing_mode)),
                );
                annotation_scope.display_background(
                    annotation_pos,
                    &annotation.style,
                    &annotation.fbox,
                );
                let annotation_content_offset = annotation_pos + annotation.fbox.content_offset();
                for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                    annotation_scope.display_inline_item_fragment(
                        annotation_content_offset + annotation_item_offset,
                        annotation_content_offset.block(writing_mode)
                            + annotation.baseline_block_offset,
                        annotation_item,
                        writing_mode,
                    )?;
                }
            }
        }

        Ok(())
    }

    fn display_inline_item_fragment(
        &mut self,
        pos: Point2L,
        baseline_pos: FixedL,
        fragment: &InlineItemFragment,
        writing_mode: WritingMode,
    ) -> Result<(), DisplayError> {
        match fragment {
            InlineItemFragment::Span(span) => {
                self.display_background(pos, &span.style, &span.fbox);

                let mut scope = self.enter_box(
                    &span.style,
                    Some((span.primary_font.metrics(), writing_mode)),
                );
                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    scope.display_inline_item_fragment(
                        child_pos,
                        baseline_pos,
                        child,
                        writing_mode,
                    )?;
                }
            }
            InlineItemFragment::Text(text) => {
                if text.style.visibility().is_visible() {
                    self.display_text(
                        round_block(pos, writing_mode),
                        baseline_pos,
                        text,
                        writing_mode,
                    )?;
                }
            }
            InlineItemFragment::Ruby(ruby) => {
                self.display_ruby_fragment(pos, baseline_pos, ruby, writing_mode)?
            }
            InlineItemFragment::Block(block) => {
                self.display_block_container_fragment(pos, block)?
            }
        }

        Ok(())
    }

    fn display_inline_content_fragment(
        &mut self,
        pos: Point2L,
        fragment: &InlineContentFragment,
    ) -> Result<(), DisplayError> {
        let writing_mode = fragment.style.writing_mode();
        let mut scope = self.enter_box(
            &fragment.style,
            Some((&fragment.primary_font_metrics, writing_mode)),
        );

        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;
            let baseline_pos =
                (current.block(writing_mode) + line.dominant_baseline_offset).round();

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                scope.display_inline_item_fragment(current, baseline_pos, item, writing_mode)?
            }
        }

        Ok(())
    }

    fn display_block_container_fragment(
        &mut self,
        pos: Point2L,
        fragment: &BlockContainerFragment,
    ) -> Result<(), DisplayError> {
        self.display_background(pos, &fragment.style, &fragment.fbox);

        let content_pos = pos + fragment.fbox.content_offset();
        let mut scope = self.enter_box(&fragment.style, None);
        match &fragment.content {
            &BlockContainerFragmentContent::Inline(offset, ref inline) => {
                scope.display_inline_content_fragment(content_pos + offset, inline)?;
            }
            BlockContainerFragmentContent::Block(children) => {
                for &(child_off, ref child) in children {
                    scope.display_block_container_fragment(content_pos + child_off, child)?;
                }
            }
        }

        Ok(())
    }
}
