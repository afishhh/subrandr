use std::{borrow::Cow, collections::VecDeque, fmt::Debug, ops::Range};

use rasterize::{color::BGRA8, Rasterizer, RenderTarget};
use thiserror::Error;
use util::{
    math::{I26Dot6, Point2, Point2f, Rect2, Vec2},
    rc::Rc,
};

use crate::{
    layout::{
        self,
        inline::{InlineContentFragment, InlineItemFragment, SpanFragment},
        FixedL, FragmentBox, LayoutContext, Point2L, Vec2L,
    },
    log::{info, trace},
    srv3,
    style::{
        computed::{Alignment, HorizontalAlignment, TextShadow, VerticalAlignment},
        ComputedStyle,
    },
    text::{
        self, platform_font_provider, FontArena, FreeTypeError, GlyphRenderError, GlyphString,
        TextMetrics,
    },
    vtt, Subrandr,
};

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
// TODO: Maybe call this a viewport or have a field called "viewport"
pub struct SubtitleContext {
    pub dpi: u32,
    pub video_width: I26Dot6,
    pub video_height: I26Dot6,
    pub padding_left: I26Dot6,
    pub padding_right: I26Dot6,
    pub padding_top: I26Dot6,
    pub padding_bottom: I26Dot6,
}

impl SubtitleContext {
    pub fn ppi(&self) -> u32 {
        self.dpi * 96 / 72
    }

    pub fn pixel_scale(&self) -> f32 {
        self.dpi as f32 / 72.0
    }

    pub fn padding_width(&self) -> I26Dot6 {
        self.padding_left + self.padding_right
    }

    pub fn padding_height(&self) -> I26Dot6 {
        self.padding_top + self.padding_bottom
    }

    pub fn player_width(&self) -> I26Dot6 {
        self.video_width + self.padding_width()
    }

    pub fn player_height(&self) -> I26Dot6 {
        self.video_height + self.padding_height()
    }
}

#[derive(Debug, Clone)]
pub enum Subtitles {
    Srv3(Rc<srv3::Subtitles>),
    Vtt(Rc<vtt::Subtitles>),
}

enum FormatLayouter {
    Srv3(srv3::Layouter),
    Vtt(vtt::Layouter),
}

pub(crate) struct FrameLayoutPass<'s, 'frame> {
    pub sctx: &'frame SubtitleContext,
    pub lctx: &'frame mut LayoutContext<'frame, 's>,
    pub t: u32,
    unchanged_range: Range<u32>,
    fragments: Vec<(Point2L, InlineContentFragment)>,
}

impl FrameLayoutPass<'_, '_> {
    pub fn add_event_range(&mut self, event: Range<u32>) -> bool {
        let r = self.unchanged_range.clone();

        if (event.start..event.end).contains(&self.t) {
            self.unchanged_range = r.start.max(event.start)..r.end.min(event.end);

            true
        } else {
            if event.start > self.t {
                self.unchanged_range = r.start..r.end.min(event.start);
            } else {
                self.unchanged_range = r.start.max(event.end)..r.end;
            }

            false
        }
    }

    pub fn add_animation_point(&mut self, point: u32) {
        if point < self.t {
            self.unchanged_range.start = self.unchanged_range.start.max(point);
        } else {
            self.unchanged_range.end = self.unchanged_range.end.min(point);
        }
    }

    pub fn emit_fragment(&mut self, pos: Point2L, block: InlineContentFragment) {
        self.fragments.push((pos, block));
    }
}

pub struct FrameRenderPass<'s, 'frame> {
    sctx: &'frame SubtitleContext,
    glyph_cache: &'frame text::GlyphCache,
    fonts: &'frame mut text::FontDb<'s>,
    rasterizer: &'frame mut dyn Rasterizer,
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

impl FrameRenderPass<'_, '_> {
    fn debug_text(
        &mut self,
        target: &mut RenderTarget,
        pos: Point2L,
        text: &str,
        alignment: Alignment,
        size: I26Dot6,
        color: BGRA8,
    ) -> Result<(), RenderError> {
        let font_arena = FontArena::new();
        let matches = text::FontMatcher::match_all(
            ["monospace"],
            text::FontStyle::default(),
            size,
            self.sctx.dpi,
            &font_arena,
            self.fonts,
        )?;
        let glyphs = text::simple_shape_text(matches.iterator(), &font_arena, text, self.fonts)?;
        let final_pos = pos
            + Self::translate_for_aligned_text(
                matches.primary(&font_arena, self.fonts)?,
                &text::compute_extents_ex(self.glyph_cache, true, &glyphs)?,
                alignment,
            );

        let image = text::render(
            self.glyph_cache,
            self.rasterizer,
            I26Dot6::ZERO,
            I26Dot6::ZERO,
            0.0,
            &GlyphString::from_glyphs(text, glyphs, text::Direction::Ltr),
        )?;
        image.blit(
            self.rasterizer,
            target,
            final_pos.x.round_to_inner(),
            final_pos.y.round_to_inner(),
            color,
        );

        Ok(())
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        extents: &TextMetrics,
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

    fn draw_line_decoration(
        &mut self,
        target: &mut RenderTarget,
        x0: FixedL,
        x1: FixedL,
        baseline_y: I26Dot6,
        decoration: &LineDecoration,
    ) {
        let decoration_y = baseline_y + decoration.baseline_offset;

        self.rasterizer.fill_axis_aligned_antialias_rect(
            target,
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
        target: &mut RenderTarget,
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
            glyphs,
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

                    text::render(
                        self.glyph_cache,
                        self.rasterizer,
                        x.fract(),
                        y.fract(),
                        sigma.into_f32(),
                        glyphs,
                    )?
                    .blit(
                        self.rasterizer,
                        target,
                        (x + shadow.offset.x).trunc_to_inner(),
                        (y + shadow.offset.y).trunc_to_inner(),
                        shadow.color,
                    );
                } else {
                    let monochrome = image.monochrome(self.rasterizer);
                    monochrome.blit(
                        self.rasterizer,
                        target,
                        (x + monochrome.offset.x + shadow.offset.x).trunc_to_inner(),
                        (y + monochrome.offset.y + shadow.offset.y).trunc_to_inner(),
                        shadow.color,
                    );
                }
            }
        }

        let text_end_x = {
            let mut end_x = x;

            for glyph in glyphs.iter_glyphs() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::Underline))
        {
            self.draw_line_decoration(target, x, text_end_x, y, decoration);
        }

        if color.a > 0 {
            image.blit(
                self.rasterizer,
                target,
                x.trunc_to_inner(),
                y.trunc_to_inner(),
                color,
            );
        }

        for decoration in decorations
            .iter()
            .filter(|x| matches!(x.kind, LineDecorationKind::LineThrough))
        {
            let decoration_y = y + decoration.baseline_offset;

            self.rasterizer.fill_axis_aligned_antialias_rect(
                target,
                Rect2::new(
                    Point2::new(x.into_f32(), decoration_y.into_f32()),
                    Point2::new(
                        text_end_x.into_f32(),
                        (decoration_y + decoration.thickness).into_f32(),
                    ),
                ),
                decoration.color,
            );
        }

        Ok(())
    }

    fn draw_background(
        &mut self,
        target: &mut RenderTarget,
        pos: Point2L,
        style: &ComputedStyle,
        fragment_box: &FragmentBox,
    ) {
        let background = style.background_color();
        if background.a != 0 {
            self.rasterizer.fill_axis_aligned_rect(
                target,
                Rect2::to_float(fragment_box.padding_box().translate(pos.to_vec())),
                background,
            );
        }
    }

    fn draw_inline_item_fragment_background(
        &mut self,
        target: &mut RenderTarget,
        pos: Point2L,
        fragment: &InlineItemFragment,
    ) {
        match fragment {
            InlineItemFragment::Span(span) => {
                self.draw_background(target, pos, &span.style, &span.fbox);

                for &(offset, ref child) in &span.content {
                    let child_pos = pos + span.fbox.content_offset() + offset;
                    self.draw_inline_item_fragment_background(target, child_pos, child);
                }
            }
            InlineItemFragment::Text(_) => {}
            InlineItemFragment::Ruby(ruby) => {
                for &(base_offset, ref base, annotation_offset, ref annotation) in &ruby.content {
                    let base_pos = pos + ruby.fbox.content_offset() + base_offset;
                    self.draw_background(target, base_pos, &base.style, &base.fbox);
                    for &(base_item_offset, ref base_item) in &base.children {
                        self.draw_inline_item_fragment_background(
                            target,
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                        );
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    self.draw_background(
                        target,
                        annotation_pos,
                        &annotation.style,
                        &annotation.fbox,
                    );
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.draw_inline_item_fragment_background(
                            target,
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
        target: &mut RenderTarget,
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
                    self.draw_inline_item_fragment_content(
                        target,
                        child_pos,
                        child,
                        current_decorations,
                    )?;
                }
            }
            InlineItemFragment::Text(text) => {
                self.draw_text_full(
                    target,
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
                            target,
                            base_pos + base.fbox.content_offset() + base_item_offset,
                            base_item,
                            current_decorations,
                        )?;
                    }

                    let annotation_pos = pos + ruby.fbox.content_offset() + annotation_offset;
                    for &(annotation_item_offset, ref annotation_item) in &annotation.children {
                        self.draw_inline_item_fragment_content(
                            target,
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
}

struct PerfTimes {
    frames: VecDeque<f32>,
    sum: f32,
}

impl PerfTimes {
    fn new() -> Self {
        Self {
            frames: VecDeque::new(),
            sum: 0.0,
        }
    }

    fn add(&mut self, duration: std::time::Duration) -> f32 {
        let time = duration.as_secs_f32() * 1000.;
        if self.frames.len() >= 100 {
            self.sum -= self.frames.pop_front().unwrap();
        }
        self.frames.push_back(time);
        self.sum += time;
        time
    }

    fn avg_frame_time(&self) -> f32 {
        self.sum / self.frames.len() as f32
    }

    fn minmax_frame_times(&self) -> (f32, f32) {
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        for time in self.frames.iter() {
            min = min.min(*time);
            max = max.max(*time);
        }

        (min, max)
    }

    fn last(&self) -> Option<f32> {
        self.frames.back().copied()
    }
}

struct PerfStats {
    start: std::time::Instant,

    layout_start: std::time::Instant,
    layout_end: std::time::Instant,
    debug_raster_end: std::time::Instant,

    whole: PerfTimes,
    layout: PerfTimes,
    debug_raster: PerfTimes,
    raster: PerfTimes,
}

impl PerfStats {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            layout_start: now,
            layout_end: now,
            debug_raster_end: now,
            whole: PerfTimes::new(),
            layout: PerfTimes::new(),
            debug_raster: PerfTimes::new(),
            raster: PerfTimes::new(),
        }
    }

    fn start_frame(&mut self) {
        self.start = std::time::Instant::now();
    }

    fn start_layout(&mut self) {
        self.layout_start = std::time::Instant::now();
    }

    fn end_layout(&mut self) {
        self.layout_end = std::time::Instant::now();
    }

    fn end_debug_raster(&mut self) {
        self.debug_raster_end = std::time::Instant::now();
    }

    fn end_frame(&mut self) -> f32 {
        let end = std::time::Instant::now();
        self.layout.add(self.layout_end - self.layout_start);
        self.debug_raster
            .add(self.debug_raster_end - self.layout_end);
        self.raster.add(end - self.debug_raster_end);
        self.whole.add(end - self.start)
    }

    fn is_empty(&self) -> bool {
        self.whole.frames.is_empty()
    }
}

pub struct Renderer<'a> {
    sbr: &'a Subrandr,
    pub(crate) fonts: text::FontDb<'a>,
    pub(crate) glyph_cache: text::GlyphCache,
    perf: PerfStats,

    unchanged_range: Range<u32>,
    previous_context: SubtitleContext,
    previous_output_size: (u32, u32),

    layouter: Option<FormatLayouter>,
}

impl<'a> Renderer<'a> {
    pub fn new(sbr: &'a Subrandr) -> Self {
        if !sbr.did_log_version.get() {
            sbr.did_log_version.set(true);
            info!(
                sbr,
                concat!(
                    "subrandr version ",
                    env!("CARGO_PKG_VERSION"),
                    env!("BUILD_REV_SUFFIX"),
                    env!("BUILD_DIRTY")
                )
            );
        }

        Self {
            sbr,
            fonts: text::FontDb::new(sbr).unwrap(),
            glyph_cache: text::GlyphCache::new(),
            perf: PerfStats::new(),
            unchanged_range: 0..0,
            previous_context: SubtitleContext {
                dpi: 0,
                video_width: I26Dot6::ZERO,
                video_height: I26Dot6::ZERO,
                padding_left: I26Dot6::ZERO,
                padding_right: I26Dot6::ZERO,
                padding_top: I26Dot6::ZERO,
                padding_bottom: I26Dot6::ZERO,
            },
            previous_output_size: (0, 0),
            layouter: None,
        }
    }

    pub fn library(&self) -> &'a Subrandr {
        self.sbr
    }

    pub fn invalidate_subtitles(&mut self) {
        self.unchanged_range = 0..0;
    }

    pub fn unchanged_inside(&self) -> Range<u32> {
        self.unchanged_range.clone()
    }

    pub fn did_change(&self, ctx: &SubtitleContext, t: u32) -> bool {
        self.previous_context != *ctx || !self.unchanged_range.contains(&t)
    }

    pub fn set_subtitles(&mut self, subs: Option<&Subtitles>) {
        self.layouter = match subs {
            Some(Subtitles::Srv3(srv3_subs)) => {
                if let Some(FormatLayouter::Srv3(srv3)) = self.layouter.as_ref() {
                    if Rc::ptr_eq(srv3.subtitles(), srv3_subs) {}
                }

                Some(FormatLayouter::Srv3(srv3::Layouter::new(srv3_subs.clone())))
            }
            Some(Subtitles::Vtt(vtt_subs)) => {
                if let Some(FormatLayouter::Vtt(vtt)) = self.layouter.as_ref() {
                    if Rc::ptr_eq(vtt.subtitles(), vtt_subs) {
                        return;
                    }
                }

                Some(FormatLayouter::Vtt(vtt::Layouter::new(vtt_subs.clone())))
            }
            None => None,
        };
    }
}

#[derive(Debug, Error)]
pub enum RenderError {
    #[error(transparent)]
    FontProviderUpdate(#[from] platform_font_provider::UpdateError),
    #[error(transparent)]
    FreeType(#[from] FreeTypeError),
    #[error(transparent)]
    GlyphRender(#[from] GlyphRenderError),
    #[error(transparent)]
    FontSelect(#[from] text::font_db::SelectError),
    #[error(transparent)]
    SimpleShaping(#[from] text::ShapingError),
    #[error(transparent)]
    Layout(#[from] layout::InlineLayoutError),
}

impl Renderer<'_> {
    pub fn render(
        &mut self,
        ctx: &SubtitleContext,
        t: u32,
        buffer: &mut [BGRA8],
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<(), RenderError> {
        buffer.fill(BGRA8::ZERO);
        self.render_to(
            &mut rasterize::sw::Rasterizer::new(),
            &mut rasterize::sw::create_render_target(buffer, width, height, stride),
            ctx,
            t,
        )
    }

    // FIXME: This is kinda ugly but `render_to` cannot be public without
    //        exposing the Rasterizer trait.
    //        Maybe just do it and mark it #[doc(hidden)]?
    #[cfg(feature = "wgpu")]
    pub fn render_to_wgpu(
        &mut self,
        rasterizer: &mut rasterize::wgpu::Rasterizer,
        mut target: RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
    ) -> Result<(), RenderError> {
        self.render_to(rasterizer, &mut target, ctx, t)?;
        rasterizer.submit_render(target);
        Ok(())
    }

    fn render_to(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
    ) -> Result<(), RenderError> {
        let (target_width, target_height) = (target.width(), target.width());

        self.previous_context = *ctx;
        self.previous_output_size = (target_width, target_height);

        if target_width == 0 || target_height == 0 {
            return Ok(());
        }

        self.perf.start_frame();
        self.fonts.update_platform_font_list()?;
        self.glyph_cache.advance_generation();

        let ctx = SubtitleContext {
            dpi: self.sbr.debug.dpi_override.unwrap_or(ctx.dpi),
            ..*ctx
        };

        let subtitle_class_name = self.layouter.as_ref().map_or("none", |layouter| {
            let this = &layouter;
            match this {
                FormatLayouter::Srv3(_) => "srv3",
                FormatLayouter::Vtt(_) => "vtt",
            }
        });

        trace!(
            self.sbr,
            "rendering frame (class={subtitle_class_name} ctx={ctx:?} t={t}ms)",
        );

        self.perf.start_layout();
        let fragments = {
            let mut pass = FrameLayoutPass {
                sctx: &ctx,
                lctx: &mut LayoutContext {
                    dpi: ctx.dpi,
                    fonts: &mut self.fonts,
                },
                t,
                unchanged_range: 0..u32::MAX,
                fragments: Vec::new(),
            };

            match self.layouter {
                Some(FormatLayouter::Srv3(ref mut layouter)) => layouter.layout(&mut pass)?,
                Some(FormatLayouter::Vtt(ref mut layouter)) => layouter.layout(&mut pass)?,
                None => (),
            }

            self.unchanged_range = pass.unchanged_range;
            pass.fragments
        };
        self.perf.end_layout();

        {
            let mut pass = FrameRenderPass {
                sctx: &ctx,
                glyph_cache: &self.glyph_cache,
                fonts: &mut self.fonts,
                rasterizer,
            };

            // FIXME: Currently mpv does not seem to have a way to pass the correct DPI
            //        to a subtitle renderer so this doesn't work.
            let debug_font_size = I26Dot6::new(16);
            let debug_line_height = FixedL::new(20) * ctx.pixel_scale();

            if self.sbr.debug.draw_version_string {
                let mut y = debug_line_height;
                pass.debug_text(
                    target,
                    Point2L::new(FixedL::ZERO, y),
                    concat!(
                        "subrandr ",
                        env!("CARGO_PKG_VERSION"),
                        env!("BUILD_REV_SUFFIX"),
                    ),
                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                pass.debug_text(
                    target,
                    Point2L::new(FixedL::ZERO, y),
                    &format!("subtitle class: {subtitle_class_name}"),
                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                let rasterizer_line = format!("=== rasterizer: {} ===", pass.rasterizer.name());
                pass.debug_text(
                    target,
                    Point2L::new(FixedL::ZERO, y),
                    &rasterizer_line,
                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                {
                    let mut rasterizer_debug_info = String::new();
                    _ = pass.rasterizer.write_debug_info(&mut rasterizer_debug_info);

                    for line in rasterizer_debug_info.lines() {
                        pass.debug_text(
                            target,
                            Point2L::new(FixedL::ZERO, y),
                            line,
                            Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                            debug_font_size,
                            BGRA8::WHITE,
                        )?;
                        y += debug_line_height;
                    }
                }

                {
                    let stats = pass.glyph_cache.stats();

                    let (footprint_divisor, footprint_suffix) =
                        util::human_size_suffix(stats.total_memory_footprint);
                    for line in [
                        format_args!("=== glyph cache stats ==="),
                        format_args!(
                            "approximate memory footprint: {:.3}{footprint_suffix}B",
                            stats.total_memory_footprint as f32 / footprint_divisor as f32
                        ),
                        format_args!("total entries: {}", stats.total_entries),
                        format_args!("current generation: {}", stats.generation),
                    ] {
                        pass.debug_text(
                            target,
                            Point2L::new(FixedL::ZERO, y),
                            &match line.as_str() {
                                Some(value) => Cow::Borrowed(value),
                                None => Cow::Owned(line.to_string()),
                            },
                            Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                            debug_font_size,
                            BGRA8::WHITE,
                        )?;
                        y += debug_line_height;
                    }
                }
            }

            if self.sbr.debug.draw_perf_info {
                let mut y = debug_line_height;
                pass.debug_text(
                    target,
                    Point2L::new(ctx.padding_left + ctx.video_width, y),
                    &format!(
                        "{:.2}x{:.2} dpi:{}",
                        ctx.video_width, ctx.video_height, ctx.dpi
                    ),
                    Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                pass.debug_text(
                    target,
                    Point2L::new(ctx.padding_left + ctx.video_width, y),
                    &format!(
                        "l:{:.2} r:{:.2} t:{:.2} b:{:.2}",
                        ctx.padding_left, ctx.padding_right, ctx.padding_top, ctx.padding_bottom
                    ),
                    Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                if !self.perf.is_empty() {
                    let mut draw_times =
                        |name: &str, times: &PerfTimes| -> Result<(), RenderError> {
                            let (min, max) = times.minmax_frame_times();
                            let avg = times.avg_frame_time();

                            pass.debug_text(
                                target,
                                Point2L::new(ctx.padding_left + ctx.video_width, y),
                                &format!(
                                "{name} min={:.1}ms avg={:.1}ms ({:.1}/s) max={:.1}ms ({:.1}/s)",
                                min,
                                avg,
                                1000.0 / avg,
                                max,
                                1000.0 / max
                            ),
                                Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                                debug_font_size,
                                BGRA8::WHITE,
                            )?;
                            y += debug_line_height;

                            Ok(())
                        };

                    draw_times("whole", &self.perf.whole)?;
                    draw_times("layout", &self.perf.layout)?;
                    draw_times("raster", &self.perf.raster)?;
                    draw_times("draster", &self.perf.debug_raster)?;

                    if let Some(last) = self.perf.whole.last() {
                        pass.debug_text(
                            target,
                            Point2L::new(ctx.padding_left + ctx.video_width, y),
                            &format!("last={:.1}ms ({:.1}/s)", last, 1000.0 / last),
                            Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                            debug_font_size,
                            BGRA8::WHITE,
                        )?;
                    }
                    y += debug_line_height;

                    let wmax = self.perf.whole.minmax_frame_times().1;
                    let lmax = self.perf.layout.minmax_frame_times().1;
                    let rmax = self.perf.raster.minmax_frame_times().1;
                    let gmax = wmax.max(lmax).max(rmax);

                    let graph_width = I26Dot6::new(400) * ctx.pixel_scale();
                    let graph_height = I26Dot6::new(50) * ctx.pixel_scale();
                    let offx = ctx.padding_left + ctx.video_width - graph_width;

                    let mut draw_polyline = |times: &PerfTimes, color: BGRA8| {
                        let mut polyline = vec![];
                        for (i, time) in times.frames.iter().copied().enumerate() {
                            let x = (graph_width * i as i32 / times.frames.len() as i32).into_f32();
                            let y = -(graph_height * time / gmax).into_f32();
                            polyline.push(Point2f::new(x, y));
                        }

                        pass.rasterizer.stroke_polyline(
                            target,
                            Vec2::new(offx.into_f32(), (y + graph_height).into_f32()),
                            &polyline,
                            color,
                        );
                    };

                    draw_polyline(&self.perf.whole, BGRA8::YELLOW);
                    draw_polyline(&self.perf.layout, BGRA8::CYAN);
                    draw_polyline(&self.perf.raster, BGRA8::ORANGERED);
                    y += graph_height;
                }

                pass.rasterizer.flush(target);
            }
            self.perf.end_debug_raster();

            for &(pos, ref fragment) in &fragments {
                let final_total_rect = fragment.fbox.margin_box().translate(pos.to_vec());

                if self.sbr.debug.draw_layout_info {
                    pass.rasterizer.stroke_axis_aligned_rect(
                        target,
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

                if self.sbr.debug.draw_layout_info {
                    pass.debug_text(
                        target,
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
                        debug_font_size,
                        BGRA8::MAGENTA,
                    )?;
                }

                for &(offset, ref line) in &fragment.lines {
                    let current = pos + offset;

                    for &(offset, ref item) in &line.children {
                        let current = current + offset;

                        pass.draw_inline_item_fragment_background(target, current, item);
                    }
                }

                let mut decoration_stack = Vec::new();
                for &(offset, ref line) in &fragment.lines {
                    let current = pos + offset;

                    for &(offset, ref item) in &line.children {
                        let current = current + offset;

                        pass.draw_inline_item_fragment_content(
                            target,
                            current,
                            item,
                            &mut decoration_stack,
                        )?;
                    }
                }
            }

            // Make sure all batched draws are flushed, although currently this is not
            // necessary because the wgpu rasterizer flushes automatically on `submit_render`.
            pass.rasterizer.flush(target);
        }

        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {time:.2}ms to render");

        Ok(())
    }
}
