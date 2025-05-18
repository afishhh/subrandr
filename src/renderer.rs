use std::{
    collections::{HashMap, VecDeque},
    fmt::Debug,
    ops::Range,
    rc::Rc,
};

use thiserror::Error;

use crate::{
    color::BGRA8,
    layout::{self, BlockContainerFragment, FixedL, LayoutContext, Point2L, Vec2L},
    log::{info, trace},
    math::{I16Dot16, I26Dot6, Point2, Point2f, Rect2, Vec2, Vec2f},
    rasterize::{self, Rasterizer, RenderTarget},
    srv3,
    style::types::{
        Alignment, HorizontalAlignment, TextDecorations, TextShadow, VerticalAlignment,
    },
    text::{self, FontArena, FreeTypeError, GlyphRenderError, GlyphString, TextMetrics},
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

    pub fn pixel_scale(&self) -> I16Dot16 {
        I16Dot16::from_quotient(self.dpi as i32, 72)
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
    fragments: Vec<(Point2L, BlockContainerFragment)>,
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

    pub fn emit_fragment(&mut self, pos: Point2L, block: BlockContainerFragment) {
        self.fragments.push((pos, block));
    }
}

pub struct FrameRenderPass<'s, 'frame> {
    sctx: &'frame SubtitleContext,
    fonts: &'frame mut text::FontDb<'s>,
    rasterizer: &'frame mut dyn Rasterizer,
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
                match matches.primary(&font_arena, self.fonts)? {
                    Some(font) => font,
                    None => return Ok(()),
                },
                &text::compute_extents_ex(true, &glyphs)?,
                alignment,
            );

        let image = text::render(
            self.rasterizer,
            I26Dot6::ZERO,
            I26Dot6::ZERO,
            &GlyphString::from_glyphs(text, glyphs),
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

    fn draw_text_full(
        &mut self,
        target: &mut RenderTarget,
        x: I26Dot6,
        y: I26Dot6,
        glyphs: &GlyphString<'_, Rc<str>>,
        color: BGRA8,
        decoration: &TextDecorations,
        shadows: &[TextShadow],
    ) -> Result<(), RenderError> {
        if glyphs.is_empty() {
            // TODO: Maybe instead ensure empty segments aren't emitted during layout?
            return Ok(());
        }

        let image = text::render(self.rasterizer, x.fract(), y.fract(), glyphs)?;

        let mut blurs = HashMap::new();

        // TODO: This should also draw an offset underline I think and possibly strike through?
        for shadow in shadows.iter().rev() {
            if shadow.color.a > 0 {
                if shadow.blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    let sigma = shadow.blur_radius / 2;

                    let (blurred, offset) = blurs.entry(sigma).or_insert_with(|| {
                        let offset = image.prepare_for_blur(self.rasterizer, sigma.into_f32());
                        let padding = self.rasterizer.blur_padding();
                        (
                            self.rasterizer.blur_to_mono_texture(),
                            -Vec2f::new(offset.x as f32, offset.y as f32) + padding,
                        )
                    });

                    self.rasterizer.blit(
                        target,
                        (x + shadow.offset.x - offset.x).trunc_to_inner(),
                        (y + shadow.offset.y - offset.y).trunc_to_inner(),
                        blurred,
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

            // TODO: Should this somehow ignore trailing advance?
            //       The issue with that is that it causes issues with cross-segment decorations
            //       so it would have to take those into account.
            for glyph in glyphs.iter_glyphs() {
                end_x += glyph.x_advance;
            }

            end_x
        };

        image.blit(
            self.rasterizer,
            target,
            x.trunc_to_inner(),
            y.trunc_to_inner(),
            color,
        );

        // FIXME: This should use the main font for the segment, not the font
        //        of the first glyph..
        let font_metrics = glyphs.iter_glyphs().next().unwrap().font.metrics();

        if decoration.underline {
            let thickness = font_metrics.underline_thickness;
            self.rasterizer.fill_axis_aligned_antialias_rect(
                target,
                Rect2::new(
                    Point2::new(x.into_f32(), y.into_f32()),
                    Point2::new(text_end_x.into_f32(), (y + thickness).into_f32()),
                ),
                decoration.underline_color,
            );
        }

        if decoration.strike_out {
            let strike_y = y + font_metrics.strikeout_top_offset;
            let thickness = font_metrics.strikeout_thickness;
            self.rasterizer.fill_axis_aligned_antialias_rect(
                target,
                Rect2::new(
                    Point2::new(x.into_f32(), strike_y.into_f32()),
                    Point2::new(text_end_x.into_f32(), (strike_y + thickness).into_f32()),
                ),
                decoration.strike_out_color,
            );
        }

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
                "subrandr version {} rev {}{}",
                env!("CARGO_PKG_VERSION"),
                env!("BUILD_REV"),
                env!("BUILD_DIRTY")
            );
        }

        Self {
            sbr,
            fonts: text::FontDb::new(sbr).unwrap(),
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
        self.previous_context = *ctx;
        self.previous_output_size = (width, height);

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
        if target_width == 0 || target_height == 0 {
            return Ok(());
        }

        self.perf.start_frame();
        self.fonts.advance_cache_generation();

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
                        " rev ",
                        env!("BUILD_REV")
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

                let rasterizer_line = format!("rasterizer: {}", pass.rasterizer.name());
                pass.debug_text(
                    target,
                    Point2L::new(FixedL::ZERO, y),
                    &rasterizer_line,
                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
                y += debug_line_height;

                if let Some(adapter_line) = pass.rasterizer.adapter_info_string() {
                    pass.debug_text(
                        target,
                        Point2L::new(FixedL::ZERO, y),
                        &adapter_line,
                        Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                        debug_font_size,
                        BGRA8::WHITE,
                    )?;
                    y += debug_line_height;
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
            }
            self.perf.end_debug_raster();

            for &(pos, ref fragment) in &fragments {
                let final_total_rect = Rect2::from_min_size(pos, fragment.fbox.size);

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
                        fragment.fbox.size.y + 20,
                        Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
                    ),
                    VerticalAlignment::Center => (
                        fragment.fbox.size.y + 20,
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
                            final_total_rect.min.y + total_position_debug_pos.0,
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

                // TODO: A trait for casting these types?
                fn convert_rect(rect: Rect2<I26Dot6>) -> Rect2<f32> {
                    Rect2::new(
                        Point2::new(rect.min.x.into_f32(), rect.min.y.into_f32()),
                        Point2::new(rect.max.x.into_f32(), rect.max.y.into_f32()),
                    )
                }

                for &(offset, ref container) in &fragment.children {
                    let current = pos + offset;

                    for &(offset, ref line) in &container.lines {
                        let current = current + offset;

                        for &(offset, ref text) in &line.children {
                            let current = current + offset;

                            if text.style.background_color.a != 0 {
                                pass.rasterizer.fill_axis_aligned_rect(
                                    target,
                                    convert_rect(Rect2::from_min_size(current, text.fbox.size)),
                                    text.style.background_color,
                                );
                            }
                        }
                    }
                }

                for &(offset, ref container) in &fragment.children {
                    let current = pos + offset;

                    for &(offset, ref line) in &container.lines {
                        let current = current + offset;

                        for &(offset, ref text) in &line.children {
                            let current = current + offset;

                            if self.sbr.debug.draw_layout_info {
                                let final_logical_box =
                                    Rect2::from_min_size(current, text.fbox.size);

                                pass.debug_text(
                                    target,
                                    final_logical_box.min,
                                    &format!("{:.0},{:.0}", current.x, current.y),
                                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Bottom),
                                    debug_font_size,
                                    BGRA8::RED,
                                )?;

                                pass.debug_text(
                                    target,
                                    Point2L::new(final_logical_box.min.x, final_logical_box.max.y),
                                    &format!("{:.1}", offset.x + text.baseline_offset.x),
                                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                                    debug_font_size,
                                    BGRA8::RED,
                                )?;

                                pass.debug_text(
                                    target,
                                    Point2L::new(final_logical_box.max.x, final_logical_box.min.y),
                                    &format!("{:.0}pt", text.style.font_size),
                                    Alignment(
                                        HorizontalAlignment::Right,
                                        VerticalAlignment::Bottom,
                                    ),
                                    debug_font_size,
                                    BGRA8::GOLD,
                                )?;

                                let final_logical_boxf = convert_rect(final_logical_box);

                                pass.rasterizer.stroke_axis_aligned_rect(
                                    target,
                                    final_logical_boxf,
                                    BGRA8::BLUE,
                                );

                                pass.rasterizer.horizontal_line(
                                    target,
                                    (current.y + text.baseline_offset.y).into_f32(),
                                    final_logical_boxf.min.x,
                                    final_logical_boxf.max.x,
                                    BGRA8::GREEN,
                                );
                            }

                            pass.draw_text_full(
                                target,
                                current.x + text.baseline_offset.x,
                                current.y + text.baseline_offset.y,
                                text.glyphs(),
                                text.style.color,
                                &text.style.decorations,
                                &text.style.shadows,
                            )?;
                        }
                    }
                }
            }
        }

        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {time:.2}ms to render");

        Ok(())
    }
}
