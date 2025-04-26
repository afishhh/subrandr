#![allow(clippy::too_many_arguments)]
#![allow(clippy::missing_transmute_annotations)]

use std::{
    cell::Cell,
    collections::{HashMap, VecDeque},
    fmt::Debug,
    ops::Range,
};

use thiserror::Error;

use color::BGRA8;
use log::{info, trace, Logger};
use math::{I16Dot16, Point2, Point2f, Rect2, Vec2, Vec2f};
use rasterize::{Rasterizer, RenderTarget};
use srv3::{Srv3Event, Srv3TextShadow};
use text::{
    layout::{MultilineTextShaper, ShapedLine, TextWrapOptions},
    FontArena, FreeTypeError, GlyphRenderError, GlyphString, TextMetrics,
};
use vtt::VttEvent;

pub mod srv3;
pub mod vtt;

mod capi;
mod color;
mod html;
mod log;
mod math;
mod outline;
pub mod rasterize;
mod text;
mod util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Alignment(pub HorizontalAlignment, pub VerticalAlignment);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VerticalAlignment {
    Top,
    BaselineCentered,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HorizontalAlignment {
    Left,
    Center,
    Right,
}

trait Layouter {
    fn wrap_width(&self, ctx: &SubtitleContext, event: &Event) -> I26Dot6;

    fn layout(
        &mut self,
        ctx: &SubtitleContext,
        lines: &mut Vec<ShapedLine>,
        total_rect: &mut Rect2<I26Dot6>,
        event: &Event,
    ) -> Point2f;
}

#[derive(Debug, Clone)]
enum EventExtra {
    Srv3(Srv3Event),
    Vtt(VttEvent),
}

#[derive(Debug, Clone)]
struct Event {
    start: u32,
    end: u32,
    // TODO: Add `text-align` to TextSegment and move actual alignment into `Layouter`.
    alignment: Alignment,
    text_wrap: TextWrapOptions,
    segments: Vec<TextSegment>,
    extra: EventExtra,
}

#[derive(Debug, Clone)]
struct TextSegment {
    font: Vec<String>,
    font_size: I26Dot6,
    font_weight: I16Dot16,
    italic: bool,
    decorations: TextDecorations,
    color: BGRA8,
    background_color: BGRA8,
    text: String,
    shadows: Vec<TextShadow>,
    ruby: Ruby,
}

#[derive(Debug, Clone, Copy)]
enum Ruby {
    None,
    Base,
    Over,
}

#[derive(Debug, Clone)]
struct TextDecorations {
    // TODO: f32 for size
    underline: bool,
    underline_color: BGRA8,
    strike_out: bool,
    strike_out_color: BGRA8,
}

#[derive(Debug, Clone)]
enum TextShadow {
    #[expect(dead_code, reason = "for WebVTT")]
    Css(CssTextShadow),
    Srv3(Srv3TextShadow),
}

#[derive(Debug, Clone)]
struct CssTextShadow {
    offset: Vec2f,
    blur_radius: I26Dot6,
    color: BGRA8,
}

impl TextDecorations {
    pub const fn none() -> Self {
        Self {
            underline: false,
            underline_color: BGRA8::ZERO,
            strike_out: false,
            strike_out_color: BGRA8::ZERO,
        }
    }
}

impl Default for TextDecorations {
    fn default() -> Self {
        Self::none()
    }
}

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

pub use math::I26Dot6;

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

#[derive(Debug)]
struct SubtitleClass {
    name: &'static str,
    get_font_size: fn(ctx: &SubtitleContext, event: &Event, segment: &TextSegment) -> I26Dot6,
    create_layouter: fn() -> Box<dyn Layouter>,
}

#[derive(Debug)]
pub struct Subtitles {
    class: &'static SubtitleClass,
    events: Vec<Event>,
}

#[derive(Default, Debug, Clone)]
struct DebugFlags {
    draw_version_string: bool,
    draw_perf_info: bool,
    draw_layout_info: bool,
    stroke_shape_outlines: bool,
}

impl DebugFlags {
    fn from_env() -> Self {
        let mut result = Self::default();

        if let Ok(s) = std::env::var("SBR_DEBUG") {
            for token in s.split(",") {
                match token {
                    "draw_version" => result.draw_version_string = true,
                    "draw_perf" => result.draw_perf_info = true,
                    "draw_layout" => result.draw_layout_info = true,
                    "draw_shape_outlines" => result.stroke_shape_outlines = true,
                    _ => (),
                }
            }
        }

        result
    }
}

#[derive(Debug)]
pub struct Subrandr {
    logger: log::Logger,
    did_log_version: Cell<bool>,
    debug: DebugFlags,
}

impl Subrandr {
    pub fn init() -> Self {
        Self {
            logger: log::Logger::Default,
            did_log_version: Cell::new(false),
            debug: DebugFlags::from_env(),
        }
    }
}

// allows for convenient logging with log!(sbr, ...)
impl log::AsLogger for Subrandr {
    fn as_logger(&self) -> &Logger {
        &self.logger
    }
}

struct PerfStats {
    start: std::time::Instant,
    times: VecDeque<f32>,
    times_sum: f32,
}

impl PerfStats {
    fn new() -> Self {
        Self {
            start: std::time::Instant::now(),
            times: VecDeque::new(),
            times_sum: 0.0,
        }
    }

    fn start_frame(&mut self) {
        self.start = std::time::Instant::now();
    }

    fn avg_frame_time(&self) -> f32 {
        self.times_sum / self.times.len() as f32
    }

    fn minmax_frame_times(&self) -> (f32, f32) {
        let mut min = f32::MAX;
        let mut max = f32::MIN;

        for time in self.times.iter() {
            min = min.min(*time);
            max = max.max(*time);
        }

        (min, max)
    }

    fn end_frame(&mut self) -> f32 {
        let end = std::time::Instant::now();
        let time = (end - self.start).as_secs_f32() * 1000.;
        if self.times.len() >= 100 {
            self.times_sum -= self.times.pop_front().unwrap();
        }
        self.times.push_back(time);
        self.times_sum += time;
        time
    }
}

pub struct Renderer<'a> {
    sbr: &'a Subrandr,
    fonts: text::FontDb<'a>,
    dpi: u32,
    perf: PerfStats,

    unchanged_range: Range<u32>,
    previous_context: SubtitleContext,
    previous_output_size: (u32, u32),
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
            dpi: 0,
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
    Layout(#[from] text::layout::LayoutError),
}

impl<'a> Renderer<'a> {
    fn debug_text(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        x: i32,
        y: i32,
        text: &str,
        alignment: Alignment,
        size: I26Dot6,
        color: BGRA8,
    ) -> Result<(), RenderError> {
        let font_arena = FontArena::new();
        let matches = text::FontMatcher::match_all(
            &["monospace"],
            text::FontStyle::default(),
            size,
            self.dpi,
            &font_arena,
            &mut self.fonts,
        )?;
        let glyphs =
            text::simple_shape_text(matches.iterator(), &font_arena, text, &mut self.fonts)?;
        let (ox, oy) = Self::translate_for_aligned_text(
            match matches.primary(&font_arena, &mut self.fonts)? {
                Some(font) => font,
                None => return Ok(()),
            },
            true,
            &text::compute_extents_ex(true, &glyphs)?,
            alignment,
        );

        let image = text::render(
            rasterizer,
            I26Dot6::ZERO,
            I26Dot6::ZERO,
            &GlyphString::from_glyphs(text, glyphs),
        )?;
        image.blit(rasterizer, target, x + ox, y + oy, color);

        Ok(())
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextMetrics,
        alignment: Alignment,
    ) -> (i32, i32) {
        assert!(horizontal);

        let Alignment(horizontal, vertical) = alignment;

        let ox = match horizontal {
            HorizontalAlignment::Left => -font.horizontal_extents().descender / 64 / 2,
            HorizontalAlignment::Center => -extents.paint_size.x.trunc_to_inner() / 2,
            HorizontalAlignment::Right => (-extents.paint_size.x
                + I26Dot6::from_raw(font.horizontal_extents().descender))
            .trunc_to_inner(),
        };

        let oy = match vertical {
            VerticalAlignment::Top => font.horizontal_extents().ascender / 64,
            VerticalAlignment::BaselineCentered => 0,
            VerticalAlignment::Bottom => font.horizontal_extents().descender / 64,
        };

        (ox, oy)
    }

    fn draw_text_full(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        x: I26Dot6,
        y: I26Dot6,
        glyphs: &GlyphString,
        color: BGRA8,
        decoration: &TextDecorations,
        shadows: &[TextShadow],
        ctx: &SubtitleContext,
    ) -> Result<(), RenderError> {
        if glyphs.is_empty() {
            // TODO: Maybe instead ensure empty segments aren't emitted during layout?
            return Ok(());
        }

        let image = text::render(rasterizer, x.fract(), y.fract(), glyphs)?;

        let mut blurs = HashMap::new();

        // TODO: This should also draw an offset underline I think and possibly strike through?
        let mut draw_css_shadow = |shadow: &CssTextShadow| {
            if shadow.color.a > 0 {
                if shadow.blur_radius > I26Dot6::from_quotient(1, 16) {
                    // https://drafts.csswg.org/css-backgrounds-3/#shadow-blur
                    // A non-zero blur radius indicates that the resulting shadow should be blurred,
                    // ... by applying to the shadow a Gaussian blur with a standard deviation
                    // equal to half the blur radius.
                    let sigma = shadow.blur_radius / 2;

                    let (blurred, offset) = blurs.entry(sigma).or_insert_with(|| {
                        let offset = image.prepare_for_blur(rasterizer, sigma.into_f32());
                        let padding = rasterizer.blur_padding();
                        (
                            rasterizer.blur_to_mono_texture(),
                            -Vec2f::new(offset.x as f32, offset.y as f32) + padding,
                        )
                    });

                    rasterizer.blit(
                        target,
                        (x + shadow.offset.x - offset.x).trunc_to_inner(),
                        (y + shadow.offset.y - offset.y).trunc_to_inner(),
                        blurred,
                        shadow.color,
                    );
                } else {
                    let monochrome = image.monochrome(rasterizer);
                    monochrome.blit(
                        rasterizer,
                        target,
                        (x + monochrome.offset.x + shadow.offset.x).trunc_to_inner(),
                        (y + monochrome.offset.y + shadow.offset.y).trunc_to_inner(),
                        shadow.color,
                    );
                }
            }
        };

        let mut out = Vec::new();
        for shadow in shadows.iter().rev() {
            match shadow {
                TextShadow::Css(css_shadow) => draw_css_shadow(css_shadow),
                TextShadow::Srv3(srv3_shadow) => {
                    srv3_shadow.to_css(ctx, &mut out);
                    for css_shadow in out.iter().rev() {
                        draw_css_shadow(css_shadow)
                    }
                    out.clear();
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
            rasterizer,
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
            rasterizer.fill_axis_aligned_antialias_rect(
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
            rasterizer.fill_axis_aligned_antialias_rect(
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

    pub fn render(
        &mut self,
        ctx: &SubtitleContext,
        t: u32,
        subs: &Subtitles,
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
            subs,
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
        subs: &Subtitles,
    ) -> Result<(), RenderError> {
        self.render_to(rasterizer, &mut target, ctx, t, subs)?;
        rasterizer.submit_render(target);
        Ok(())
    }

    fn render_to(
        &mut self,
        rasterizer: &mut dyn Rasterizer,
        target: &mut RenderTarget,
        ctx: &SubtitleContext,
        t: u32,
        subs: &Subtitles,
    ) -> Result<(), RenderError> {
        let (target_width, target_height) = (target.width(), target.width());
        if target_width == 0 || target_height == 0 {
            return Ok(());
        }

        self.perf.start_frame();
        self.fonts.advance_cache_generation();

        self.dpi = ctx.dpi;

        trace!(
            self.sbr,
            "rendering frame (class={} ctx={ctx:?} t={t}ms)",
            subs.class.name
        );

        // FIXME: Currently mpv does not seem to have a way to pass the correct DPI
        //        to a subtitle renderer so this doesn't work.
        let debug_font_size = I26Dot6::new(16);
        let debug_line_height = I26Dot6::new(20) * ctx.pixel_scale();

        if self.sbr.debug.draw_version_string {
            self.debug_text(
                rasterizer,
                target,
                0,
                0,
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

            self.debug_text(
                rasterizer,
                target,
                0,
                debug_line_height.round_to_inner(),
                &format!("subtitle class: {}", subs.class.name),
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                debug_font_size,
                BGRA8::WHITE,
            )?;

            let rasterizer_line = format!("rasterizer: {}", rasterizer.name());
            self.debug_text(
                rasterizer,
                target,
                0,
                (debug_line_height * 2).round_to_inner(),
                &rasterizer_line,
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                debug_font_size,
                BGRA8::WHITE,
            )?;

            if let Some(adapter_line) = rasterizer.adapter_info_string() {
                self.debug_text(
                    rasterizer,
                    target,
                    0,
                    (debug_line_height * 3).round_to_inner(),
                    &adapter_line,
                    Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                    debug_font_size,
                    BGRA8::WHITE,
                )?;
            }
        }

        if self.sbr.debug.draw_perf_info {
            self.debug_text(
                rasterizer,
                target,
                (ctx.padding_left + ctx.video_width).round_to_inner(),
                debug_line_height.round_to_inner(),
                &format!(
                    "{:.2}x{:.2} dpi:{}",
                    ctx.video_width, ctx.video_height, ctx.dpi
                ),
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                debug_font_size,
                BGRA8::WHITE,
            )?;

            self.debug_text(
                rasterizer,
                target,
                (ctx.padding_left + ctx.video_width).round_to_inner(),
                (debug_line_height * 2).round_to_inner(),
                &format!(
                    "l:{:.2} r:{:.2} t:{:.2} b:{:.2}",
                    ctx.padding_left, ctx.padding_right, ctx.padding_top, ctx.padding_bottom
                ),
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                debug_font_size,
                BGRA8::WHITE,
            )?;

            if !self.perf.times.is_empty() {
                let (min, max) = self.perf.minmax_frame_times();
                let avg = self.perf.avg_frame_time();

                self.debug_text(
                    rasterizer,
                    target,
                    (ctx.padding_left + ctx.video_width).round_to_inner(),
                    (debug_line_height * 3).round_to_inner(),
                    &format!(
                        "min={:.1}ms avg={:.1}ms ({:.1}fps) max={:.1}ms ({:.1}fps)",
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

                if let Some(&last) = self.perf.times.iter().last() {
                    self.debug_text(
                        rasterizer,
                        target,
                        (ctx.padding_left + ctx.video_width).round_to_inner(),
                        (debug_line_height * 4).round_to_inner(),
                        &format!("last={:.1}ms ({:.1}fps)", last, 1000.0 / last),
                        Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
                        debug_font_size,
                        BGRA8::WHITE,
                    )?;
                }

                let graph_width = I26Dot6::new(300) * ctx.pixel_scale();
                let graph_height = I26Dot6::new(50) * ctx.pixel_scale();
                let offx = (ctx.padding_left + ctx.video_width - graph_width).round_to_inner();
                let mut polyline = vec![];
                for (i, time) in self.perf.times.iter().copied().enumerate() {
                    let x = (graph_width * i as i32 / self.perf.times.len() as i32).into_f32();
                    let y = -(graph_height * time / max).into_f32();
                    polyline.push(Point2f::new(x, y));
                }

                rasterizer.stroke_polyline(
                    target,
                    Vec2::new(
                        offx as f32,
                        ((debug_line_height * 5.5) + graph_height).into_f32(),
                    ),
                    &polyline,
                    BGRA8::new(255, 255, 0, 255),
                );
            }
        }

        let mut unchanged_range: Range<u32> = 0..u32::MAX;
        {
            let mut layouter = (subs.class.create_layouter)();
            let font_arena = FontArena::new();
            for event in subs.events.iter() {
                let r = unchanged_range.clone();
                if (event.start..event.end).contains(&t) {
                    unchanged_range = r.start.max(event.start)..r.end.min(event.end);
                } else {
                    if event.start > t {
                        unchanged_range = r.start..r.end.min(event.start);
                    } else {
                        unchanged_range = r.start.max(event.end)..r.end;
                    }
                    continue;
                }

                let mut shaper = MultilineTextShaper::new();
                let mut last_ruby_base = None;
                for segment in event.segments.iter() {
                    let matcher = text::FontMatcher::match_all(
                        segment.font.iter().map(String::as_str),
                        text::FontStyle {
                            weight: segment.font_weight,
                            italic: segment.italic,
                        },
                        (subs.class.get_font_size)(ctx, event, segment),
                        ctx.dpi,
                        &font_arena,
                        &mut self.fonts,
                    )?;

                    match segment.ruby {
                        Ruby::None => {
                            shaper.add_text(&segment.text, matcher);
                        }
                        Ruby::Base => {
                            last_ruby_base = Some(shaper.add_ruby_base(&segment.text, matcher));
                        }
                        Ruby::Over => {
                            shaper.add_ruby_annotation(
                                last_ruby_base.expect("Ruby::Over without preceding Ruby::Base"),
                                &segment.text,
                                matcher,
                            );
                            last_ruby_base = None;
                        }
                    }
                }

                let Alignment(horizontal_alignment, vertical_alignment) = event.alignment;
                let (mut lines, mut total_rect) = shaper.shape(
                    horizontal_alignment,
                    event.text_wrap,
                    layouter.wrap_width(ctx, event),
                    &font_arena,
                    &mut self.fonts,
                )?;

                let Point2 { x, y } = layouter.layout(ctx, &mut lines, &mut total_rect, event);

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => {
                            -total_rect.height().trunc_to_inner() / 2
                        }
                        VerticalAlignment::Bottom => -total_rect.height().trunc_to_inner(),
                    };

                let final_total_rect = total_rect.translate(Vec2::new(x, y));

                if self.sbr.debug.draw_layout_info {
                    rasterizer.stroke_axis_aligned_rect(
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

                let total_position_debug_pos = match vertical_alignment {
                    VerticalAlignment::Top => (
                        total_rect.height() + 20,
                        Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
                    ),
                    VerticalAlignment::BaselineCentered => (
                        total_rect.height() + 20,
                        Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
                    ),
                    VerticalAlignment::Bottom => (
                        I26Dot6::new(-24),
                        Alignment(HorizontalAlignment::Center, VerticalAlignment::Bottom),
                    ),
                };

                if self.sbr.debug.draw_layout_info {
                    self.debug_text(
                        rasterizer,
                        target,
                        (final_total_rect.min.x + final_total_rect.width() / 2).trunc_to_inner(),
                        (final_total_rect.min.y + total_position_debug_pos.0).trunc_to_inner(),
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

                for line in &lines {
                    let mut current_background_box = Rect2::<I26Dot6>::NOTHING;
                    let mut last_segment_index = usize::MAX;

                    for shaped_segment in &line.segments {
                        if shaped_segment.corresponding_input_segment != last_segment_index {
                            if last_segment_index != usize::MAX {
                                let segment = &event.segments[last_segment_index];
                                if segment.background_color.a != 0 {
                                    rasterizer.fill_axis_aligned_rect(
                                        target,
                                        convert_rect(current_background_box)
                                            .translate(Vec2::new(x as f32, y as f32)),
                                        segment.background_color,
                                    );
                                }
                            }

                            current_background_box = shaped_segment.logical_rect;
                            last_segment_index = shaped_segment.corresponding_input_segment;
                        } else {
                            current_background_box.expand_to_point(shaped_segment.logical_rect.max);
                        }
                    }

                    // FIXME: Background boxes should have corner radius (with SRV3, not WebVTT)
                    if last_segment_index != usize::MAX {
                        let segment = &event.segments[last_segment_index];
                        if segment.background_color.a != 0 {
                            rasterizer.fill_axis_aligned_rect(
                                target,
                                convert_rect(current_background_box)
                                    .translate(Vec2::new(x as f32, y as f32)),
                                segment.background_color,
                            );
                        }
                    }
                }

                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];

                    let final_logical_box = convert_rect(shaped_segment.logical_rect)
                        .translate(Vec2::new(x as f32, y as f32));

                    if self.sbr.debug.draw_layout_info {
                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.min.x as i32,
                            final_logical_box.min.y as i32,
                            &format!(
                                "{:.0},{:.0}",
                                shaped_segment.logical_rect.min.x + x,
                                shaped_segment.logical_rect.min.y + y
                            ),
                            Alignment(HorizontalAlignment::Left, VerticalAlignment::Bottom),
                            debug_font_size,
                            BGRA8::RED,
                        )?;

                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.min.x as i32,
                            final_logical_box.max.y as i32,
                            &format!("{:.1}", shaped_segment.baseline_offset.x),
                            Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                            debug_font_size,
                            BGRA8::RED,
                        )?;

                        self.debug_text(
                            rasterizer,
                            target,
                            final_logical_box.max.x as i32,
                            final_logical_box.min.y as i32,
                            &format!("{:.0}pt", (subs.class.get_font_size)(ctx, event, segment)),
                            Alignment(HorizontalAlignment::Right, VerticalAlignment::Bottom),
                            debug_font_size,
                            BGRA8::GOLD,
                        )?;

                        rasterizer.stroke_axis_aligned_rect(target, final_logical_box, BGRA8::BLUE);

                        rasterizer.horizontal_line(
                            target,
                            (shaped_segment.baseline_offset.y + y).into_f32(),
                            final_logical_box.min.x,
                            final_logical_box.max.x,
                            BGRA8::GREEN,
                        );
                    }

                    let x = shaped_segment.baseline_offset.x + x;
                    let y = shaped_segment.baseline_offset.y + y;

                    self.draw_text_full(
                        rasterizer,
                        target,
                        x,
                        y,
                        &shaped_segment.glyphs,
                        segment.color,
                        &segment.decorations,
                        &segment.shadows,
                        ctx,
                    )?;
                }
            }
        }

        self.unchanged_range = unchanged_range;

        let time = self.perf.end_frame();
        trace!(self.sbr, "frame took {:.2}ms to render", time);

        Ok(())
    }
}
