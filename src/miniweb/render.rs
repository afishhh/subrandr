use crate::{
    color::BGRA8,
    math::{Point2, Rect2},
    rasterize::RenderTarget,
    renderer::{FrameRenderPass, RenderError},
    I26Dot6,
};

use super::{
    layout::{
        BlockContainerFragment, BlockFragmentChild, FragmentBox, InlineContainerFragment,
        LineBoxFragment, Point2L, TextFragment,
    },
    style::{
        types::{Alignment, HorizontalAlignment, VerticalAlignment},
        ComputedStyle,
    },
};

pub struct RenderContext {}

// TODO: A trait for casting these types?
fn convert_rect(rect: Rect2<I26Dot6>) -> Rect2<f32> {
    Rect2::new(
        Point2::new(rect.min.x.into_f32(), rect.min.y.into_f32()),
        Point2::new(rect.max.x.into_f32(), rect.max.y.into_f32()),
    )
}

impl FragmentBox {
    fn render_background(
        &self,
        pass: &mut FrameRenderPass,
        target: &mut RenderTarget,
        pos: Point2L,
        style: &ComputedStyle,
    ) {
        if style.background_color().a != 0 {
            pass.rasterizer.fill_axis_aligned_rect(
                target,
                convert_rect(Rect2::from_min_size(pos, self.size)),
                style.background_color(),
            );
        }
    }

    fn render_container_debug_info(
        &self,
        pass: &mut FrameRenderPass,
        target: &mut RenderTarget,
        pos: Point2L,
        kind: &'static str,
    ) -> Result<(), RenderError> {
        if pass.sbr.debug.draw_layout_info {
            let final_total_rect = Rect2::from_min_size(pos, self.size);

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

            let total_position_debug_pos = match VerticalAlignment::Top {
                VerticalAlignment::Top => (
                    self.size.y + 20,
                    Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
                ),
                VerticalAlignment::Center => (
                    self.size.y + 20,
                    Alignment(HorizontalAlignment::Center, VerticalAlignment::Top),
                ),
                VerticalAlignment::Bottom => (
                    I26Dot6::new(-24),
                    Alignment(HorizontalAlignment::Center, VerticalAlignment::Bottom),
                ),
            };

            pass.debug_text(
                target,
                Point2L::new(
                    final_total_rect.min.x + final_total_rect.width() / 2,
                    final_total_rect.min.y + total_position_debug_pos.0,
                ),
                &format!(
                    "x:{:.1} y:{:.1} w:{:.1} h:{:.1} {kind}",
                    final_total_rect.x(),
                    final_total_rect.y(),
                    final_total_rect.width(),
                    final_total_rect.height()
                ),
                total_position_debug_pos.1,
                pass.debug_font_size,
                BGRA8::MAGENTA,
            )?;
        }

        Ok(())
    }
}

pub trait Renderable {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError>;
}

impl Renderable for TextFragment {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError> {
        self.fbox.render_background(pass, target, pos, &self.style);

        if pass.sbr.debug.draw_layout_info {
            let final_logical_box = Rect2::from_min_size(pos, self.fbox.size);

            pass.debug_text(
                target,
                final_logical_box.min,
                &format!("{:.0},{:.0}", pos.x, pos.y),
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Bottom),
                pass.debug_font_size,
                BGRA8::RED,
            )?;

            pass.debug_text(
                target,
                Point2L::new(final_logical_box.min.x, final_logical_box.max.y),
                &format!("{:.1}", pos.x + self.baseline_offset.x),
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
                pass.debug_font_size,
                BGRA8::RED,
            )?;

            pass.debug_text(
                target,
                Point2L::new(final_logical_box.max.x, final_logical_box.min.y),
                &format!("{:.0}pt", self.style.font_size()),
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Bottom),
                pass.debug_font_size,
                BGRA8::GOLD,
            )?;

            let final_logical_boxf = convert_rect(final_logical_box);

            pass.rasterizer
                .stroke_axis_aligned_rect(target, final_logical_boxf, BGRA8::BLUE);

            pass.rasterizer.horizontal_line(
                target,
                (pos.y + self.baseline_offset.y).into_f32(),
                final_logical_boxf.min.x,
                final_logical_boxf.max.x,
                BGRA8::GREEN,
            );
        }

        pass.draw_text_full(
            target,
            pos.x + self.baseline_offset.x,
            pos.y + self.baseline_offset.y,
            self.glyphs(),
            self.style.color(),
            &self.style.text_decoration(),
            self.style.text_shadows(),
        )?;

        Ok(())
    }
}

impl Renderable for LineBoxFragment {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError> {
        self.fbox
            .render_container_debug_info(pass, target, pos, "line")?;

        for &(off, ref text) in &self.children {
            text.render(pass, pos + off, target)?;
        }

        Ok(())
    }
}

impl Renderable for InlineContainerFragment {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError> {
        for &(off, ref line) in &self.lines {
            line.render(pass, pos + off, target)?;
        }

        Ok(())
    }
}

impl Renderable for BlockFragmentChild {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError> {
        match self {
            BlockFragmentChild::Inline(inline) => inline.render(pass, pos, target),
            BlockFragmentChild::Block(block) => block.render(pass, pos, target),
        }
    }
}

impl Renderable for BlockContainerFragment {
    fn render(
        &self,
        pass: &mut FrameRenderPass,
        pos: Point2L,
        target: &mut RenderTarget,
    ) -> Result<(), RenderError> {
        for &(off, ref child) in &self.children {
            child.render(pass, pos + off, target)?;
        }

        Ok(())
    }
}
