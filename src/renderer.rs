use std::{collections::VecDeque, fmt::Debug, fmt::Write as _, ops::Range};

use rasterize::{color::BGRA8, Rasterizer, RenderTarget};
use thiserror::Error;
use util::{
    math::{I16Dot16, I26Dot6, Point2f, Vec2},
    rc::Rc,
    rc_static,
};

use crate::{
    layout::{
        self,
        inline::{InlineContentBuilder, InlineContentFragment},
        FixedL, LayoutConstraints, LayoutContext, Point2L,
    },
    log::{info, trace},
    render::{self, RenderPass},
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
    nondebug_raster_end: std::time::Instant,

    whole: PerfTimes,
    nondebug_layout: PerfTimes,
    debug_layout: PerfTimes,
    nondebug_raster: PerfTimes,
    debug_raster: PerfTimes,
}

impl PerfStats {
    fn new() -> Self {
        let now = std::time::Instant::now();
        Self {
            start: now,
            layout_start: now,
            nondebug_layout_end: now,
            debug_layout_end: now,
            nondebug_raster_end: now,
            whole: PerfTimes::new(),
            nondebug_layout: PerfTimes::new(),
            debug_layout: PerfTimes::new(),
            nondebug_raster: PerfTimes::new(),
            debug_raster: PerfTimes::new(),
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

    fn end_nondebug_raster(&mut self) {
        self.nondebug_raster_end = std::time::Instant::now();
    }

    fn end_frame(&mut self) -> f32 {
        let end = std::time::Instant::now();
        self.nondebug_layout
            .add(self.nondebug_layout_end - self.layout_start);
        self.debug_layout
            .add(self.debug_layout_end - self.nondebug_layout_end);
        self.nondebug_raster
            .add(self.nondebug_raster_end - self.debug_layout_end);
        self.debug_raster.add(end - self.nondebug_raster_end);
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
            previous_output_size: (0, 0),
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
    #[error("Failed to render output")]
    Render(#[from] render::RenderError),
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
    Texture(rasterize::Texture),
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

    const DEBUG_FONT_SIZE: I26Dot6 = I26Dot6::new(16);

    fn layout_debug_overlay(
        sbr: &Subrandr,
        perf: &PerfStats,
        lctx: &mut LayoutContext,
        ctx: &SubtitleContext,
        glyph_cache: &text::GlyphCache,
        rasterizer: &mut dyn Rasterizer,
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

                    let (min, max) = times.minmax_frame_times();
                    let avg = times.avg_frame_time();
                    _ = writeln!(
                        main,
                        " min={:.1}ms avg={:.1}ms ({:.1}/s) max={:.1}ms ({:.1}/s)",
                        min,
                        avg,
                        1000.0 / avg,
                        max,
                        1000.0 / max
                    );
                };

                draw_times("whole", &perf.whole, BGRA8::YELLOW);
                draw_times("layout", &perf.nondebug_layout, BGRA8::CYAN);
                draw_times("dlayout", &perf.debug_layout, BGRA8::BLUE);
                draw_times("raster", &perf.nondebug_raster, BGRA8::ORANGERED);
                draw_times("draster", &perf.debug_raster, BGRA8::RED);

                if let Some(last) = perf.whole.last() {
                    _ = writeln!(main, "last={:.1}ms ({:.1}/s)", last, 1000.0 / last);
                }

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

                let wmax = perf.whole.minmax_frame_times().1;
                let lmax = perf.nondebug_layout.minmax_frame_times().1;
                let rmax = perf.nondebug_raster.minmax_frame_times().1;
                let gmax = wmax.max(lmax).max(rmax);

                let graph_width = I26Dot6::new(400) * ctx.pixel_scale();
                let graph_height = I26Dot6::new(50) * ctx.pixel_scale();
                let texture_size = Vec2::new(
                    graph_width.ceil_to_inner() as u32,
                    graph_height.ceil_to_inner() as u32,
                );

                let graph_texture = unsafe {
                    rasterizer.create_texture_mapped(
                        texture_size.x,
                        texture_size.y,
                        rasterize::PixelFormat::Bgra,
                        Box::new(|buffer, stride| {
                            buffer.fill(std::mem::MaybeUninit::zeroed());

                            let n_pixels = buffer.len() / 4;
                            let pixel_stride = stride / 4;
                            let pixels: &mut [BGRA8] = std::slice::from_raw_parts_mut(
                                buffer.as_mut_ptr().cast(),
                                n_pixels,
                            );

                            let mut glyph_rasterizer = rasterize::sw::GlyphRasterizer::new();
                            let mut draw_polyline = |times: &PerfTimes, color: BGRA8| {
                                let points =
                                    times.frames.iter().copied().enumerate().map(|(i, time)| {
                                        let x = (graph_width * i as i32
                                            / times.frames.len() as i32)
                                            .into_f32();
                                        let y = -(graph_height * time / gmax).into_f32();
                                        Point2f::new(x, y + graph_height.into_f32())
                                    });

                                glyph_rasterizer.reset(texture_size);
                                glyph_rasterizer.stroke_polyline(points, ctx.pixel_scale());
                                glyph_rasterizer.rasterize(|y, xs, v| {
                                    let row = match pixels
                                        .get_mut(y as usize * pixel_stride + xs.start as usize..)
                                    {
                                        Some(row) => row,
                                        None => return,
                                    };
                                    let width = row.len().min((xs.end - xs.start) as usize);
                                    for pixel in &mut row[..width] {
                                        *pixel =
                                            color.mul_alpha((v >> 8) as u8).blend_over(*pixel).0;
                                    }
                                });
                            };

                            draw_polyline(&perf.whole, BGRA8::YELLOW);
                            draw_polyline(&perf.nondebug_layout, BGRA8::CYAN);
                            draw_polyline(&perf.debug_layout, BGRA8::BLUE);
                            draw_polyline(&perf.nondebug_raster, BGRA8::ORANGERED);
                            draw_polyline(&perf.debug_raster, BGRA8::RED);
                        }),
                    )
                };

                fragments.push((
                    Point2L::new(ctx.padding_left + ctx.video_width - graph_width, graph_y),
                    DebugContentFragment::Texture(graph_texture),
                ));
            }
        }

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
            let mut pass = RenderPass {
                dpi: ctx.dpi,
                glyph_cache: &self.glyph_cache,
                fonts: &mut self.fonts,
                rasterizer,
                target,
                debug_flags: &self.sbr.debug,
                debug_text_font_size: Self::DEBUG_FONT_SIZE,
            };

            for &(pos, ref fragment) in &fragments {
                pass.draw_inline_content_fragment(pos, fragment)?;
            }
            self.perf.end_nondebug_raster();

            for &(pos, ref fragment) in &debug_overlay_fragments {
                match fragment {
                    DebugContentFragment::Inline(inline) => {
                        pass.draw_inline_content_fragment(pos, inline)?;
                    }
                    DebugContentFragment::Texture(texture) => {
                        pass.rasterizer.blit(
                            pass.target,
                            pos.x.floor_to_inner(),
                            pos.y.floor_to_inner(),
                            texture,
                            BGRA8::WHITE,
                        );
                    }
                }
            }

            // Make sure all batched draws are flushed, although currently this is not
            // necessary because the wgpu rasterizer flushes automatically on `submit_render`.
            pass.rasterizer.flush(pass.target);
        }

        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {time:.2}ms to render");

        Ok(())
    }
}
