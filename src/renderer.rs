use std::{collections::VecDeque, fmt::Debug, fmt::Write as _, ops::Range};

use rasterize::{color::BGRA8, Rasterizer};
use thiserror::Error;
use util::{
    math::{I16Dot16, I26Dot6, Point2, Vec2},
    rc::Rc,
    rc_static,
};

use crate::{
    display::{DisplayPass, Drawing, DrawingNode, PaintOp, PaintOpBuilder, StrokedPolyline},
    layout::{
        self,
        inline::{InlineContentBuilder, InlineContentFragment},
        FixedL, LayoutConstraints, LayoutContext, Point2L,
    },
    log::{info, trace},
    raster::{rasterize_to_target, RasterContext, RasterError},
    srv3,
    style::{computed::HorizontalAlignment, ComputedStyle},
    text::{self, platform_font_provider},
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
    nondebug_layout_end: std::time::Instant,
    debug_layout_end: std::time::Instant,
    display_end: std::time::Instant,

    whole: PerfTimes,
    nondebug_layout: PerfTimes,
    debug_layout: PerfTimes,
    display: PerfTimes,
    raster: PerfTimes,
}

impl PerfStats {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,

            layout_start: now,
            nondebug_layout_end: now,
            debug_layout_end: now,
            display_end: now,

            whole: PerfTimes::new(),
            nondebug_layout: PerfTimes::new(),
            debug_layout: PerfTimes::new(),
            display: PerfTimes::new(),
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
        self.nondebug_layout_end = std::time::Instant::now();
    }

    fn end_debug_layout(&mut self) {
        self.debug_layout_end = std::time::Instant::now();
    }

    fn end_display(&mut self) {
        self.display_end = std::time::Instant::now();
    }

    fn end_frame(&mut self) -> f32 {
        let end = std::time::Instant::now();

        self.nondebug_layout
            .add(self.nondebug_layout_end - self.layout_start);
        self.debug_layout
            .add(self.debug_layout_end - self.nondebug_layout_end);

        self.display.add(self.display_end - self.debug_layout_end);

        self.raster.add(end - self.display_end);

        self.whole.add(end - self.start)
    }

    fn is_empty(&self) -> bool {
        self.whole.frames.is_empty()
    }

    fn make_frame_time_graph(&self, pixel_scale: I16Dot16) -> Option<(Vec2<I16Dot16>, Drawing)> {
        if self.whole.frames.len() <= 1 {
            return None;
        }

        let wmax = self.whole.minmax_frame_times().1;
        let lmax = self.nondebug_layout.minmax_frame_times().1;
        let dlmax = self.debug_layout.minmax_frame_times().1;
        let dmax = self.display.minmax_frame_times().1;
        let rmax = self.raster.minmax_frame_times().1;
        let gmax = wmax.max(lmax).max(dlmax).max(dmax).max(rmax);

        let graph_width = I16Dot16::new(500) * pixel_scale;
        let graph_height = I16Dot16::new(80) * pixel_scale;
        let mut graph_drawing = Drawing { nodes: Vec::new() };

        let mut draw_polyline = |times: &PerfTimes, color: BGRA8| {
            let polyline = times
                .frames
                .iter()
                .copied()
                .enumerate()
                .map(|(i, time)| {
                    Point2::new(
                        graph_width
                            * I16Dot16::from_quotient(i as i32, times.frames.len() as i32 - 1),
                        graph_height - (graph_height * time / gmax),
                    )
                })
                .collect();

            graph_drawing
                .nodes
                .push(DrawingNode::StrokedPolyline(StrokedPolyline {
                    polyline,
                    width: pixel_scale,
                    color,
                }));
        };

        draw_polyline(&self.whole, BGRA8::YELLOW);
        draw_polyline(&self.nondebug_layout, BGRA8::CYAN);
        draw_polyline(&self.debug_layout, BGRA8::BLUE);
        draw_polyline(&self.display, BGRA8::LIME);
        draw_polyline(&self.raster, BGRA8::ORANGERED);

        Some((Vec2::new(graph_width, graph_height), graph_drawing))
    }
}

// TODO: This type has kind of become a "mainly C API abstraction".
//       Move it into the capi?
pub struct Renderer<'a> {
    sbr: &'a Subrandr,
    pub(crate) fonts: text::FontDb<'a>,
    pub(crate) glyph_cache: text::GlyphCache,
    perf: PerfStats,
    paint_ops: Vec<PaintOp>,

    unchanged_range: Range<u32>,
    previous_context: SubtitleContext,

    layouter: Option<FormatLayouter>,
}

impl<'a> Renderer<'a> {
    pub fn new(sbr: &'a Subrandr) -> Result<Self, platform_font_provider::InitError> {
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

        Ok(Self {
            sbr,
            fonts: text::FontDb::new(sbr)?,
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
            paint_ops: Vec::new(),
            layouter: None,
        })
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
    #[error("Failed to refresh system fonts")]
    FontProviderUpdate(#[from] platform_font_provider::UpdateError),
    #[error("Failed to paint output")]
    Paint(#[from] RasterError),
    #[error("Failed to layout inline content")]
    Layout(#[from] layout::InlineLayoutError),
    #[error("Failed to layout debug overlay")]
    DebugLayout(#[from] DebugLayoutError),
}

#[derive(Debug, Error)]
pub enum DebugLayoutError {
    #[error("Failed to layout inline content")]
    Inline(#[from] layout::InlineLayoutError),
}

enum DebugContentFragment {
    Inline(InlineContentFragment),
    Drawing(Drawing),
}

impl Renderer<'_> {
    // TODO: inline into callers?
    pub fn render(
        &mut self,
        ctx: &SubtitleContext,
        t: u32,
        buffer: &mut [BGRA8],
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<(), RenderError> {
        let mut rasterizer = rasterize::sw::Rasterizer::new();
        self.render_to_ops(ctx, t, &rasterizer)?;

        buffer.fill(BGRA8::ZERO);
        rasterize_to_target(
            &mut rasterizer,
            &mut rasterize::sw::create_render_target(buffer, width, height, stride),
            &mut RasterContext {
                glyph_cache: &self.glyph_cache,
            },
            &self.paint_ops,
        )?;
        self.end_raster();
        Ok(())
    }

    #[cfg(feature = "wgpu")]
    pub fn render_to_wgpu(
        &mut self,
        rasterizer: &mut rasterize::wgpu::Rasterizer,
        mut target: rasterize::RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
    ) -> Result<(), RenderError> {
        self.render_to_ops(ctx, t, rasterizer)?;

        rasterize_to_target(
            rasterizer,
            &mut target,
            &mut RasterContext {
                glyph_cache: &self.glyph_cache,
            },
            &self.paint_ops,
        )?;
        rasterizer.submit_render(target);
        self.end_raster();
        Ok(())
    }

    fn layout_debug_overlay(
        sbr: &Subrandr,
        perf: &PerfStats,
        lctx: &mut LayoutContext,
        ctx: &SubtitleContext,
        glyph_cache: &text::GlyphCache,
        rasterizer: &dyn Rasterizer,
        subtitle_class_name: &str,
        fragments: &mut Vec<(Point2L, DebugContentFragment)>,
    ) -> Result<(), DebugLayoutError> {
        let base_style = {
            let mut result = ComputedStyle::DEFAULT;
            *result.make_font_family_mut() = rc_static!([rc_static!(str b"monospace")]);
            result
        };

        if sbr.debug.draw_version_string {
            let mut builder = InlineContentBuilder::new();
            let mut root = builder.root();
            let mut main = root.push_span(base_style.clone());

            main.push_text(concat!(
                "subrandr ",
                env!("CARGO_PKG_VERSION"),
                env!("BUILD_REV_SUFFIX"),
                "\n"
            ));

            _ = writeln!(main, "subtitle class: {subtitle_class_name}");

            _ = writeln!(main, "=== rasterizer: {} ===", rasterizer.name());
            _ = rasterizer.write_debug_info(&mut main);
            if !main.current_run_text().ends_with('\n') {
                main.push_text("\n");
            }

            {
                let stats = glyph_cache.stats();
                let (footprint_divisor, footprint_suffix) =
                    util::human_size_suffix(stats.total_memory_footprint);

                _ = writeln!(main, "=== glyph cache stats ===");
                _ = writeln!(
                    main,
                    "approximate memory footprint: {:.3}{footprint_suffix}B",
                    stats.total_memory_footprint as f32 / footprint_divisor as f32
                );
                _ = writeln!(main, "total entries: {}", stats.total_entries);
                _ = writeln!(main, "current generation: {}", stats.generation);
            }

            drop(main);
            drop(root);
            fragments.push((
                Point2L::ZERO,
                DebugContentFragment::Inline(layout::inline::layout(
                    lctx,
                    &LayoutConstraints::NONE,
                    &builder.finish(),
                    HorizontalAlignment::Left,
                )?),
            ));
        }

        if sbr.debug.draw_perf_info {
            let mut builder = InlineContentBuilder::new();
            let mut root = builder.root();
            let mut main = root.push_span(base_style.clone());

            _ = writeln!(
                main,
                "{:.2}x{:.2} dpi:{}",
                ctx.video_width, ctx.video_height, ctx.dpi
            );

            _ = writeln!(
                main,
                "l:{:.2} r:{:.2} t:{:.2} b:{:.2}",
                ctx.padding_left, ctx.padding_right, ctx.padding_top, ctx.padding_bottom
            );

            if !perf.is_empty() {
                let mut draw_times = |name: &str, times: &PerfTimes, color: BGRA8| {
                    main.push_span({
                        let mut result = base_style.create_derived();
                        *result.make_color_mut() = color;
                        *result.make_font_weight_mut() = I16Dot16::new(700);
                        result
                    })
                    .push_text(name);

                    let (_, max) = times.minmax_frame_times();
                    let last = times.last().unwrap();
                    let avg = times.avg_frame_time();
                    _ = writeln!(
                        main,
                        " last={:.1}ms avg={:.1}ms ({:.1}/s) max={:.1}ms ({:.1}/s)",
                        last,
                        avg,
                        1000.0 / avg,
                        max,
                        1000.0 / max
                    );
                };

                draw_times("whole", &perf.whole, BGRA8::YELLOW);
                draw_times("layout", &perf.nondebug_layout, BGRA8::CYAN);
                draw_times("debug_layout", &perf.debug_layout, BGRA8::BLUE);
                draw_times("display", &perf.display, BGRA8::LIME);
                draw_times("raster", &perf.raster, BGRA8::ORANGERED);

                drop(main);
                drop(root);
                let perf_text_fragment = layout::inline::layout(
                    lctx,
                    &LayoutConstraints::NONE,
                    &builder.finish(),
                    HorizontalAlignment::Right,
                )?;
                let graph_y = perf_text_fragment.fbox.size_for_layout().y;
                fragments.push((
                    Point2L::new(
                        ctx.video_width + ctx.padding_width()
                            - perf_text_fragment.fbox.size_for_layout().x,
                        FixedL::ZERO,
                    ),
                    DebugContentFragment::Inline(perf_text_fragment),
                ));

                if let Some((graph_size, graph_drawing)) =
                    perf.make_frame_time_graph(I16Dot16::from_quotient(ctx.dpi as i32, 72))
                {
                    fragments.push((
                        Point2L::new(
                            ctx.padding_left + ctx.video_width
                                - FixedL::from_raw(graph_size.x.into_raw() >> 10),
                            graph_y,
                        ),
                        DebugContentFragment::Drawing(graph_drawing),
                    ));
                }
            }
        }

        Ok(())
    }

    pub(crate) fn render_to_ops(
        &mut self,
        ctx: &SubtitleContext,
        t: u32,
        rasterizer: &dyn Rasterizer,
    ) -> Result<(), RenderError> {
        self.previous_context = *ctx;

        self.perf.start_frame();
        self.fonts.update_platform_font_list()?;
        self.glyph_cache.advance_generation();
        self.paint_ops.clear();

        let ctx = SubtitleContext {
            dpi: self.sbr.debug.dpi_override.unwrap_or(ctx.dpi),
            ..*ctx
        };

        let subtitle_class_name =
            self.layouter
                .as_ref()
                .map_or("none", |layouter| match &layouter {
                    FormatLayouter::Srv3(_) => "srv3",
                    FormatLayouter::Vtt(_) => "vtt",
                });

        trace!(
            self.sbr,
            "rendering frame (class={subtitle_class_name} ctx={ctx:?} t={t}ms)",
        );

        self.perf.start_layout();
        let mut debug_overlay_fragments = Vec::new();
        let fragments;
        {
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

            fragments = {
                match self.layouter {
                    Some(FormatLayouter::Srv3(ref mut layouter)) => layouter.layout(&mut pass)?,
                    Some(FormatLayouter::Vtt(ref mut layouter)) => layouter.layout(&mut pass)?,
                    None => (),
                }

                self.unchanged_range = pass.unchanged_range;
                pass.fragments
            };
            self.perf.end_layout();

            Self::layout_debug_overlay(
                self.sbr,
                &self.perf,
                pass.lctx,
                &ctx,
                &self.glyph_cache,
                rasterizer,
                subtitle_class_name,
                &mut debug_overlay_fragments,
            )?;
            self.perf.end_debug_layout();
        }

        {
            let mut pass = DisplayPass {
                output: PaintOpBuilder(&mut self.paint_ops),
            };

            for &(pos, ref fragment) in &fragments {
                pass.display_inline_content_fragment(pos, fragment);
            }

            for &(pos, ref fragment) in &debug_overlay_fragments {
                match fragment {
                    DebugContentFragment::Inline(inline) => {
                        pass.display_inline_content_fragment(pos, inline);
                    }
                    DebugContentFragment::Drawing(drawing) => {
                        pass.output
                            .push_drawing(Point2::new(pos.x, pos.y), drawing.clone());
                    }
                }
            }
        }
        self.perf.end_display();

        Ok(())
    }

    pub(crate) fn paint_ops(&self) -> &[PaintOp] {
        &self.paint_ops
    }

    pub fn end_raster(&mut self) {
        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {time:.2}ms to render");
    }
}
