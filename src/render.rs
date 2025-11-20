use rasterize::{color::BGRA8, RenderTarget};
use thiserror::Error;
use util::math::{I26Dot6, Point2, Rect2};

use crate::{
    layout::{
        inline::{GlyphString, InlineContentFragment, InlineItemFragment, SpanFragment},
        FixedL, FragmentBox, Point2L, Vec2L,
    },
    style::{
        computed::{Alignment, HorizontalAlignment, TextShadow, VerticalAlignment},
        ComputedStyle,
    },
    text::{self, FontArena, FreeTypeError, GlyphRenderError},
};

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    GlyphRender(#[from] GlyphRenderError),
    #[error("Failed to layout debug text")]
    DebugText(#[from] DebugTextLayoutError),
}

#[derive(Debug, Error)]
pub enum DebugTextLayoutError {
    #[error(transparent)]
    FontSelect(#[from] text::font_db::SelectError),
    #[error(transparent)]
    Shaping(#[from] text::ShapingError),
    #[error("Failed to measure glyphs")]
    Metrics(#[from] FreeTypeError),
}

pub struct RenderPass<'r, 't, 'a> {
    pub dpi: u32,
    pub glyph_cache: &'r text::GlyphCache,
    pub fonts: &'r mut text::FontDb<'a>,
    pub rasterizer: &'r mut dyn rasterize::Rasterizer,
    pub target: &'r mut RenderTarget<'t>,
    pub debug_flags: &'r crate::DebugFlags,
    pub debug_text_font_size: I26Dot6,
}

impl RenderPass<'_, '_, '_> {
    /// Calculates the visual offset required to align text according to `alignment`.
    ///
    /// Only used for drawing debug text which doesn't go through normal inline layout.
    fn translation_for_aligned_debug_text(
        font: &text::Font,
        extents: &text::TextMetrics,
        alignment: Alignment,
    ) -> Vec2L {
        let Alignment(horizontal, vertical) = alignment;

        let width = extents.paint_size.x + extents.trailing_advance;
        let ox = match horizontal {
            HorizontalAlignment::Left => FixedL::ZERO,
            HorizontalAlignment::Center => -width / 2,
            HorizontalAlignment::Right => -width,
        };

        let oy = match vertical {
            VerticalAlignment::Top => font.metrics().ascender / 64,
            VerticalAlignment::Center => FixedL::ZERO,
            VerticalAlignment::Bottom => font.metrics().descender / 64,
        };

        Vec2L::new(ox, oy)
    }

    pub fn draw_debug_text(
        &mut self,
        pos: Point2L,
        text: &str,
        alignment: Alignment,
        color: BGRA8,
    ) -> Result<(), RenderError> {
        let font_arena = FontArena::new();
        let matches = text::FontMatcher::match_all(
            ["monospace"],
            text::FontStyle::default(),
            self.debug_text_font_size,
            self.dpi,
            &font_arena,
            self.fonts,
        )
        .map_err(DebugTextLayoutError::FontSelect)?;
        let glyphs = text::simple_shape_text(matches.iterator(), &font_arena, text, self.fonts)
            .map_err(DebugTextLayoutError::Shaping)?;
        let primary_font = matches
            .primary(&font_arena, self.fonts)
            .map_err(DebugTextLayoutError::FontSelect)?;
        let final_pos = pos
            + Self::translation_for_aligned_debug_text(
                primary_font,
                &text::compute_extents_ex(self.glyph_cache, true, &glyphs)
                    .map_err(DebugTextLayoutError::Metrics)?,
                alignment,
            );

        let image = text::render(
            self.glyph_cache,
            self.rasterizer,
            I26Dot6::ZERO,
            I26Dot6::ZERO,
            0.0,
            &mut glyphs.iter(),
        )?;
        image.blit(
            self.rasterizer,
            self.target,
            final_pos.x.round_to_inner(),
            final_pos.y.round_to_inner(),
            color,
        );

        Ok(())
    }
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

impl RenderPass<'_, '_, '_> {
    fn draw_line_decoration(
        &mut self,
        x0: FixedL,
        x1: FixedL,
        baseline_y: I26Dot6,
        decoration: &LineDecoration,
    ) {
        let decoration_y = baseline_y + decoration.baseline_offset;

        self.rasterizer.fill_axis_aligned_antialias_rect(
            self.target,
            Rect2::new(
                Point2::new(x0.into_f32(), decoration_y.into_f32()),
                Point2::new(
                    x1.into_f32(),
                    (decoration_y + decoration.thickness).into_f32(),
                ),
            ),
            decoration.color,
        );
    }

    fn draw_text_full(
        &mut self,
        x: I26Dot6,
        y: I26Dot6,
        glyphs: &GlyphString<'_, std::rc::Rc<str>>,
        color: BGRA8,
        decorations: &[LineDecoration],
        shadows: &[TextShadow],
    ) -> Result<(), RenderError> {
        if glyphs.is_empty() {
            // TODO: Maybe instead ensure empty segments aren't emitted during layout?
            return Ok(());
        }

        let image = text::render(
            self.glyph_cache,
            self.rasterizer,
            x.fract(),
            y.fract(),
            0.0,
            &mut glyphs.iter_glyphs_visual(),
        )?;

        // TODO: This should also draw an offset underline I think and possibly strike through?
        for shadow in shadows.iter().rev() {
            if shadow.color.a > 0 {
                if shadow.blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    let sigma = shadow.blur_radius / 2;
                    let shadow_x = x + shadow.offset.x;
                    let shadow_y = y + shadow.offset.y;

                    text::render(
                        self.glyph_cache,
                        self.rasterizer,
                        shadow_x.fract(),
                        shadow_y.fract(),
                        sigma.into_f32(),
                        &mut glyphs.iter_glyphs_visual(),
                    )?
                    .blit(
                        self.rasterizer,
                        self.target,
                        shadow_x.trunc_to_inner(),
                        shadow_y.trunc_to_inner(),
                        shadow.color,
                    );
                } else {
                    // TODO: Re-render for correct fractional position
                    let monochrome = image.monochrome(self.rasterizer);
                    monochrome.blit(
                        self.rasterizer,
                        self.target,
                        (x + monochrome.offset.x + shadow.offset.x).trunc_to_inner(),
                        (y + monochrome.offset.y + shadow.offset.y).trunc_to_inner(),
                        shadow.color,
                    );
                }
            }
        }

        let text_end_x = {
            let mut end_x = x;

            for glyph in glyphs.iter_glyphs_visual() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::Underline))
        {
            self.draw_line_decoration(x, text_end_x, y, decoration);
        }

        if color.a > 0 {
            image.blit(
                self.rasterizer,
                self.target,
                x.trunc_to_inner(),
                y.trunc_to_inner(),
                color,
            );
        }

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::LineThrough))
        {
            self.draw_line_decoration(x, text_end_x, y, decoration);
        }

        Ok(())
    }

    fn draw_background(&mut self, pos: Point2L, style: &ComputedStyle, fragment_box: &FragmentBox) {
        let background = style.background_color();
        if background.a != 0 {
            self.rasterizer.fill_axis_aligned_rect(
                self.target,
                Rect2::to_float(fragment_box.padding_box().translate(pos.to_vec())),
                background,
            );
        }
    }

    fn draw_inline_item_fragment_background(
        &mut self,
        pos: Point2L,
        fragment: &InlineItemFragment,
    ) {
        match fragment {
            InlineItemFragment::Span(span) => {
                self.draw_background(pos, &span.style, &span.fbox);

                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    self.draw_inline_item_fragment_background(child_pos, child);
                }
            }
            InlineItemFragment::Text(_) => {}
            InlineItemFragment::Ruby(ruby) => {
                for &(base_offset, ref base, annotation_offset, ref annotation) in &ruby.content {
                    let base_pos = pos + ruby.fbox.content_offset() + base_offset;
                    self.draw_background(base_pos, &base.style, &base.fbox);
                    for &(base_item_offset, ref base_item) in &base.children {
                        self.draw_inline_item_fragment_background(
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                        );
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    self.draw_background(annotation_pos, &annotation.style, &annotation.fbox);
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.draw_inline_item_fragment_background(
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

    fn draw_inline_item_fragment_content(
        &mut self,
        pos: Point2L,
        fragment: &InlineItemFragment,
        current_decorations: &mut Vec<LineDecoration>,
    ) -> Result<(), RenderError> {
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
                    self.draw_inline_item_fragment_content(child_pos, child, current_decorations)?;
                }
            }
            InlineItemFragment::Text(text) => {
                self.draw_text_full(
                    pos.x + text.baseline_offset.x,
                    (pos.y + text.baseline_offset.y).round(),
                    text.glyphs(),
                    text.style.color(),
                    current_decorations,
                    text.style.text_shadows(),
                )?;
            }
            InlineItemFragment::Ruby(ruby) => {
                for &(base_offset, ref base, annotation_offset, ref annotation) in &ruby.content {
                    let base_pos = pos + ruby.fbox.content_offset() + base_offset;
                    for &(base_item_offset, ref base_item) in &base.children {
                        self.draw_inline_item_fragment_content(
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                            current_decorations,
                        )?;
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.draw_inline_item_fragment_content(
                            annotation_pos
                                + annotation.fbox.content_offset()
                                + annotation_item_offset,
                            annotation_item,
                            &mut Vec::new(),
                        )?;
                    }
                }
            }
        }

        current_decorations.truncate(previous_decoration_count);

        Ok(())
    }

    pub fn draw_inline_content_fragment(
        &mut self,
        pos: Point2L,
        fragment: &InlineContentFragment,
    ) -> Result<(), RenderError> {
        let final_total_rect = fragment.fbox.margin_box().translate(pos.to_vec());

        if self.debug_flags.draw_layout_info {
            self.rasterizer.stroke_axis_aligned_rect(
                self.target,
                Rect2::new(
                    Point2::new(
                        final_total_rect.min.x.into_f32() - 1.,
                        final_total_rect.min.y.into_f32() - 1.,
                    ),
                    Point2::new(
                        final_total_rect.max.x.into_f32() + 2.,
                        final_total_rect.max.y.into_f32() + 2.,
                    ),
                ),
                BGRA8::MAGENTA,
            );
        }

        let total_position_debug_pos = match VerticalAlignment::Top {
            VerticalAlignment::Top => (
                final_total_rect.max.y + 20,
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
            ),
            VerticalAlignment::Center => (
                final_total_rect.max.y + 20,
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
            ),
            VerticalAlignment::Bottom => (
                I26Dot6::new(-24),
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Bottom),
            ),
        };

        if self.debug_flags.draw_layout_info {
            self.draw_debug_text(
                Point2L::new(
                    final_total_rect.min.x + final_total_rect.width() / 2,
                    total_position_debug_pos.0,
                ),
                &format!(
                    "x:{:.1} y:{:.1} w:{:.1} h:{:.1}",
                    final_total_rect.x(),
                    final_total_rect.y(),
                    final_total_rect.width(),
                    final_total_rect.height()
                ),
                total_position_debug_pos.1,
                BGRA8::MAGENTA,
            )?;
        }

        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                self.draw_inline_item_fragment_background(current, item);
            }
        }

        let mut decoration_stack = Vec::new();
        for &(offset, ref line) in &fragment.lines {
            let current = pos + offset;

            for &(offset, ref item) in &line.children {
                let current = current + offset;

                self.draw_inline_item_fragment_content(current, item, &mut decoration_stack)?;
            }
        }

        Ok(())
    }
}
