use std::{ops::Range, rc::Rc};

use rasterize::color::BGRA8;
use util::math::{I16Dot16, Point2, Rect2};

use crate::{
    layout::{
        self, BlockContainer, BlockContainerFragment, FixedL, InlineContainer, InlineLayoutError,
        InlineText, LineBoxFragment, Point2L, Vec2L,
    },
    log::{log_once_state, warning, LogOnceSet},
    renderer::FrameLayoutPass,
    style::{
        self,
        types::{FontSlant, HorizontalAlignment, Ruby, TextDecorations},
        ComputedStyle, StyleMap,
    },
    vtt, Subrandr, SubtitleContext,
};

#[derive(Debug)]
enum ComputedLine {
    Percentage(f64),
    Lines(f64),
}

#[derive(Debug, Clone, Copy)]
enum ComputedPositionAlignment {
    LineLeft,
    Center,
    LineRight,
}

impl vtt::Line {
    // https://www.w3.org/TR/webvtt1/#cue-computed-line
    fn computed(self, showing_track_index: u32) -> ComputedLine {
        match self {
            vtt::Line::Percentage(value) => ComputedLine::Percentage(value),
            vtt::Line::Lines(value) => ComputedLine::Lines(value),
            vtt::Line::Auto => {
                // I believe step 3 can also only happen via DOM manipulation?
                ComputedLine::Lines(-((showing_track_index + 1) as f64))
            }
        }
    }
}

impl vtt::Cue<'_> {
    // https://www.w3.org/TR/webvtt1/#cue-computed-position
    fn computed_position(&self) -> f64 {
        match self.position {
            vtt::Position::Percentage(value) => value,
            vtt::Position::Auto => match self.text_alignment {
                // NOTE: Browsers implement an older version of the spec where
                //       this said that Start and End should act like Left and
                //       Right respectively.
                //       See https://github.com/w3c/webvtt/pull/273
                vtt::TextAlignment::Left => 0.,
                vtt::TextAlignment::Right => 100.,
                _ => 50.,
            },
        }
    }

    // https://www.w3.org/TR/webvtt1/#cue-computed-position-alignment
    fn computed_position_alignment(&self, text_is_ltr: bool) -> ComputedPositionAlignment {
        match self.position_alignment {
            vtt::PositionAlignment::LineLeft => ComputedPositionAlignment::LineLeft,
            vtt::PositionAlignment::Center => ComputedPositionAlignment::Center,
            vtt::PositionAlignment::LineRight => ComputedPositionAlignment::LineRight,
            vtt::PositionAlignment::Auto => match self.text_alignment {
                vtt::TextAlignment::Left => ComputedPositionAlignment::LineLeft,
                vtt::TextAlignment::Right => ComputedPositionAlignment::LineRight,
                vtt::TextAlignment::Start => {
                    if text_is_ltr {
                        ComputedPositionAlignment::LineLeft
                    } else {
                        ComputedPositionAlignment::LineRight
                    }
                }
                vtt::TextAlignment::End => {
                    if text_is_ltr {
                        ComputedPositionAlignment::LineRight
                    } else {
                        ComputedPositionAlignment::LineLeft
                    }
                }
                vtt::TextAlignment::Center => ComputedPositionAlignment::Center,
            },
        }
    }
}

#[derive(Debug, Clone)]
struct Event {
    range: Range<u32>,
    writing_direction: vtt::WritingDirection,
    text_alignment: vtt::TextAlignment,
    horizontal_alignment: HorizontalAlignment,
    line: vtt::Line,
    size: f64,
    segments: Vec<InlineText>,
    x: f64,
    y: f64,
}

impl Event {
    fn layout(
        &self,
        sctx: &SubtitleContext,
        lctx: &mut layout::LayoutContext<'_, '_>,
        style: &style::StyleMap,
        output: &mut Vec<Rect2<FixedL>>,
    ) -> Result<(Point2L, layout::BlockContainerFragment), layout::InlineLayoutError> {
        let mut fragment = layout::layout(
            lctx,
            layout::LayoutConstraints {
                size: Vec2L::new(sctx.video_width * self.size as f32 / 100, FixedL::MAX),
            },
            &BlockContainer {
                style: {
                    let mut result = StyleMap::new();
                    result.set::<style::TextAlign>(self.horizontal_alignment);
                    result;
                    todo!()
                },
                contents: vec![InlineContainer {
                    style: ComputedStyle::default(),
                    contents: self.segments.clone(),
                }],
            },
        )?;

        let container = &mut fragment.children[0].1;

        let lines = &mut container.lines;
        if lines.is_empty() {
            return Ok((Point2L::ZERO, BlockContainerFragment::empty()));
        }

        let mut result = Point2L::new(
            (self.x as f32 * sctx.video_width.into_f32()).into(),
            (self.y as f32 * sctx.video_height.into_f32()).into(),
        );

        if self.writing_direction.is_horizontal() {
            // emulate width = size vw
            result.x += match self.text_alignment {
                vtt::TextAlignment::Start | vtt::TextAlignment::Left => 0.,
                vtt::TextAlignment::Center => sctx.video_width.into_f32() * self.size as f32 / 200.,
                vtt::TextAlignment::End | vtt::TextAlignment::Right => {
                    sctx.video_width.into_f32() * self.size as f32 / 100.
                }
            };
        } else {
            // emulate height = size vh
            result.y += match self.text_alignment {
                vtt::TextAlignment::Start | vtt::TextAlignment::Left => 0.,
                vtt::TextAlignment::Center => {
                    sctx.video_height.into_f32() * self.size as f32 / 200.
                }
                vtt::TextAlignment::End | vtt::TextAlignment::Right => {
                    sctx.video_height.into_f32() * self.size as f32 / 100.
                }
            };
        }

        match self.horizontal_alignment {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => result.x -= fragment.fbox.size.x / 2,
            HorizontalAlignment::Right => result.x -= fragment.fbox.size.x,
        }

        Self::process_cue_settings_adjust_boxes(
            output,
            sctx,
            &mut result,
            lines,
            Rect2::from_min_size(Point2L::ZERO, container.fbox.size),
            self,
        );

        for &(off, ref line) in &lines[..] {
            output.push(Rect2::from_min_size(result + off, line.fbox.size));
        }

        result.x += sctx.padding_left;
        result.y += sctx.padding_top;

        Ok((result, fragment))
    }

    // https://www.w3.org/TR/webvtt1/#processing-cue-settings
    fn process_cue_settings_adjust_boxes(
        output: &[Rect2<FixedL>],
        ctx: &SubtitleContext,
        result: &mut Point2L,
        lines: &mut Vec<(Vec2L, LineBoxFragment)>,
        total_rect: Rect2<FixedL>,
        extra: &Event,
    ) {
        match extra.line.computed(0) {
            ComputedLine::Lines(line) => {
                let full_dimension = if extra.writing_direction.is_horizontal() {
                    // 1. Horizontal: Let full dimension be the height of video’s rendering area.
                    ctx.video_height
                } else {
                    // 1. Vertical: Let full dimension be the width of video’s rendering area.
                    ctx.video_width
                };

                let mut step = if extra.writing_direction.is_horizontal() {
                    // 2. Horizontal: Let step be the height of the first line box in boxes.
                    lines[0].1.fbox.size.y
                } else {
                    // 2. Vertical: Let step be the width of the first line box in boxes.
                    lines[0].1.fbox.size.x
                };

                // 3. If step is zero, then jump to the step labeled done positioning below.
                if step == FixedL::ZERO {
                    return;
                }

                // Let line be cue’s computed line.
                // Round line to an integer by adding 0.5 and then flooring it.
                let mut line = (line + 0.5).floor() as i32;

                // Vertical Growing Left: Add one to line then negate it.
                if extra.writing_direction.is_vertical_growing_left() {
                    line = -(line + 1)
                };

                // Let position be the result of multiplying step and line.
                let mut position = step * line;

                // Vertical Growing Left: Decrease position by the width of the bounding box of the boxes in boxes, then increase position by step.
                if extra.writing_direction.is_vertical_growing_left() {
                    position -= total_rect.width().into_f32();
                    position += step;
                }

                // If line is less than zero then increase position by max dimension, and negate step.
                // NOTE: "max dimension" wasn't defined? Does the standard mean "full dimension"?
                if line < 0 {
                    position += full_dimension;
                    step = -step;
                }

                if extra.writing_direction.is_horizontal() {
                    // Horizontal: Move all the boxes in boxes down by the distance given by position.
                    result.y += position.into_f32();
                } else {
                    // Vertical: Move all the boxes in boxes right by the distance given by position.
                    result.x += position.into_f32();
                }

                // Remember the position of all the boxes in boxes as their specified position.
                // NOTE: We never move actually move the boxes so this is not necessary.

                // Let title area be a box that covers all of the video’s rendering area.
                let title_area =
                    Rect2::new(Point2::ZERO, Point2::new(ctx.video_width, ctx.video_height));

                let mut switched = false;
                loop {
                    let mut done = true;
                    'check: for &(off, ref line) in &lines[..] {
                        let effective_rect = Rect2::from_min_size(*result + off, line.fbox.size);

                        if !title_area.includes(effective_rect) {
                            done = false;
                            break 'check;
                        }

                        for out in output.iter() {
                            if effective_rect.intersects(out) {
                                done = false;
                                break 'check;
                            }
                        }
                    }

                    if done {
                        return;
                    }

                    let mut switch_direction = false;
                    let first_line_box = Rect2::from_min_size(*result, lines[0].1.fbox.size);
                    if extra.writing_direction.is_horizontal() {
                        // Horizontal: If step is negative and the top of the first line box in boxes is now above the top of the title area, or if step is positive and the bottom of the first line box in boxes is now below the bottom of the title area, jump to the step labeled switch direction.
                        if (step < FixedL::ZERO && first_line_box.min.y < title_area.min.y)
                            || (step > FixedL::ZERO && first_line_box.max.y > title_area.max.y)
                        {
                            switch_direction = true;
                        }
                    } else {
                        // Vertical: If step is negative and the left edge of the first line box in boxes is now to the left of the left edge of the title area, or if step is positive and the right edge of the first line box in boxes is now to the right of the right edge of the title area, jump to the step labeled switch direction.
                        if (step < FixedL::ZERO && first_line_box.min.x < title_area.min.x)
                            || (step > FixedL::ZERO && first_line_box.max.x > title_area.max.x)
                        {
                            switch_direction = true;
                        }
                    };

                    if !switch_direction {
                        if extra.writing_direction.is_horizontal() {
                            // Horizontal: Move all the boxes in boxes down by the distance given by step. (If step is negative, then this will actually result in an upwards movement of the boxes in absolute terms.)
                            result.y += step.into_f32();
                        } else {
                            // Vertical: Move all the boxes in boxes right by the distance given by step. (If step is negative, then this will actually result in a leftwards movement of the boxes in absolute terms.)
                            result.x += step.into_f32();
                        }
                        // Jump back to the step labeled step loop.
                    } else {
                        // Switch direction: If switched is true, then remove all the boxes in boxes, and jump to the step labeled done positioning below.
                        if switched {
                            lines.clear();
                            return;
                        }

                        // Otherwise, move all the boxes in boxes back to their specified position as determined in the earlier step.

                        // Negate step.
                        step = -step;

                        // Set switched to true.
                        switched = true;

                        // Jump back to the step labeled step loop.
                    }
                }
            }
            ComputedLine::Percentage(percentage) => {
                // 4. If there is a position to which the boxes in boxes can be moved while maintaining the relative positions of the boxes in boxes to each other such that none of the boxes in boxes would overlap any of the boxes in output, and all the boxes in boxes would be within the video’s rendering area, then move the boxes in boxes to the closest such position to their current position, and then jump to the step labeled done positioning below. If there are multiple such positions that are equidistant from their current position, use the highest one amongst them; if there are several at that height, then use the leftmost one amongst them.
                // TODO: The above instruction is absolutely ridiculous, I have no idea whether this can even be done in in a reasonable time complexity...
                //       Luckily I'm not alone in this and chromium also doesn't implement it:
                //       https://source.chromium.org/chromium/chromium/src/+/main:third_party/blink/renderer/core/html/track/vtt/vtt_cue_layout_algorithm.cc;drc=fdb13881b0ca71cec367a74aa5efdedeaa2160e7;l=326
                //       It seems like this line in the standard is absolutely useless and users will almost definitely start to rely
                //       on the current behaviour in browsers which is to not perform this step.
                //       Actually, WebKit does implement this most likely via "nudging until it works" like
                //       in the snap-to-lines case.
                //       See: https://issues.chromium.org/issues/40339463
                //       12 year old w3c bug: https://www.w3.org/Bugs/Public/show_bug.cgi?id=22029
                //       I couldn't find this reported on the github so it's probably been forgotten about.
                _ = percentage;
            }
        }
    }
}

// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub enum InternalNodeKind<'a> {
//     Class,
//     Italic,
//     Bold,
//     Underline,
//     Ruby,
//     RubyText,
//     Voice { value: Annotation<'a> },
//     Language,
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub struct InternalNode<'a> {
//     pub kind: InternalNodeKind<'a>,
//     pub classes: ClassList<'a>,
//     pub language: Option<Cow<'a, str>>,
//     pub children: Vec<Node<'a>>,
// }

// #[derive(Debug, Clone, PartialEq, Eq)]
// pub enum Node<'a> {
//     Internal(InternalNode<'a>),
//     Text(Text<'a>),
//     Timestamp(u32),
// }

// TODO: Ruby
struct TextConverter {
    segments: Vec<InlineText>,
}

impl TextConverter {
    fn process_node(&mut self, node: &vtt::Node, style: ComputedStyle) {
        match node {
            vtt::Node::Internal(internal) => {
                let mut style = StyleMap::new();
                match internal.kind {
                    vtt::InternalNodeKind::Italic => {
                        style.set::<style::FontStyle>(FontSlant::Italic)
                    }
                    vtt::InternalNodeKind::Bold => {
                        style.set::<style::FontWeight>(I16Dot16::new(700))
                    }
                    vtt::InternalNodeKind::Underline => {
                        style.set::<style::TextDecoration>(TextDecorations {
                            underline: true,
                            underline_color: style.get_copy_or::<style::Color>(BGRA8::WHITE),
                            strike_out: false,
                            strike_out_color: BGRA8::ZERO,
                        })
                    }
                    _ => (),
                }

                for class in internal.classes.iter() {
                    match class {
                        "white" => style.set::<style::Color>(BGRA8::WHITE),
                        "lime" => style.set::<style::Color>(BGRA8::LIME),
                        "cyan" => style.set::<style::Color>(BGRA8::CYAN),
                        "red" => style.set::<style::Color>(BGRA8::RED),
                        "yellow" => style.set::<style::Color>(BGRA8::YELLOW),
                        "magenta" => style.set::<style::Color>(BGRA8::MAGENTA),
                        "blue" => style.set::<style::Color>(BGRA8::BLUE),
                        "black" => style.set::<style::Color>(BGRA8::BLACK),
                        "bg_white" => style.set::<style::BackgroundColor>(BGRA8::WHITE),
                        "bg_lime" => style.set::<style::BackgroundColor>(BGRA8::LIME),
                        "bg_cyan" => style.set::<style::BackgroundColor>(BGRA8::CYAN),
                        "bg_red" => style.set::<style::BackgroundColor>(BGRA8::RED),
                        "bg_yellow" => style.set::<style::BackgroundColor>(BGRA8::YELLOW),
                        "bg_magenta" => style.set::<style::BackgroundColor>(BGRA8::MAGENTA),
                        "bg_blue" => style.set::<style::BackgroundColor>(BGRA8::BLUE),
                        "bg_black" => style.set::<style::BackgroundColor>(BGRA8::BLACK),
                        _ => (),
                    }
                }

                for child in &internal.children {
                    self.process_node(child, todo!());
                }
            }
            vtt::Node::Text(text) => self.segments.push(InlineText {
                style: style.clone(),
                ruby: Ruby::None,
                text: text.content().into(),
            }),
            vtt::Node::Timestamp(_) => (),
        }
    }
}

fn convert_text(text: &str) -> Vec<InlineText> {
    let mut converter = TextConverter {
        segments: Vec::new(),
    };

    for node in vtt::parse_cue_text(text) {
        converter.process_node(&node, ComputedStyle::default());
    }

    converter.segments
}

#[derive(Debug)]
pub struct Subtitles {
    root_style: StyleMap,
    events: Vec<Event>,
}

pub fn convert(sbr: &Subrandr, captions: vtt::Captions) -> Subtitles {
    let mut subtitles = Subtitles {
        root_style: {
            let mut result = StyleMap::new();

            result.set::<style::FontFamily>(Rc::new(["sans-serif".into()]));
            result.set::<style::Color>(BGRA8::WHITE);
            result.set::<style::BackgroundColor>(BGRA8::new(0, 0, 0, /* 255 * 80% */ 204));

            result
        },
        events: Vec::new(),
    };

    let logset = LogOnceSet::new();
    log_once_state!(in logset; region_unsupported);

    if !captions.stylesheets.is_empty() {
        warning!(
            sbr,
            "WebVTT file makes use of stylesheets, which are currently not supported and will be ignored."
        )
    }

    for cue in captions.cues {
        if cue.region.is_some() && !captions.regions.is_empty() {
            warning!(
                sbr,
                once(region_unsupported),
                "WebVTT file makes use of regions, which are currently not supported and will be ignored."
            )
        }

        let computed_position = cue.computed_position();
        let computed_position_alignment = cue.computed_position_alignment(true);
        let maximum_size = match computed_position_alignment {
            ComputedPositionAlignment::LineLeft => 100. - computed_position,
            ComputedPositionAlignment::LineRight => computed_position,
            ComputedPositionAlignment::Center => {
                if computed_position <= 50. {
                    computed_position * 2.
                } else {
                    (100. - computed_position) * 2.
                }
            }
        };

        let size = cue.size.min(maximum_size);

        let mut x_position = 0.0;
        let mut y_position = 0.0;

        match cue.writing_direction {
            vtt::WritingDirection::Horizontal => match computed_position_alignment {
                ComputedPositionAlignment::LineLeft => x_position = computed_position,
                ComputedPositionAlignment::Center => x_position = computed_position - size / 2.,
                ComputedPositionAlignment::LineRight => x_position = computed_position - size,
            },
            vtt::WritingDirection::VerticalGrowingLeft
            | vtt::WritingDirection::VerticalGrowingRight => match computed_position_alignment {
                ComputedPositionAlignment::LineLeft => y_position = computed_position,
                ComputedPositionAlignment::Center => y_position = computed_position - size / 2.,
                ComputedPositionAlignment::LineRight => y_position = computed_position - size,
            },
        }

        match cue.line {
            vtt::Line::Percentage(percentage) => match cue.writing_direction {
                vtt::WritingDirection::Horizontal => y_position = percentage,
                vtt::WritingDirection::VerticalGrowingLeft
                | vtt::WritingDirection::VerticalGrowingRight => x_position = percentage,
            },
            vtt::Line::Lines(_) | vtt::Line::Auto => match cue.writing_direction {
                vtt::WritingDirection::Horizontal => y_position = 0.,
                vtt::WritingDirection::VerticalGrowingLeft
                | vtt::WritingDirection::VerticalGrowingRight => x_position = 0.,
            },
        }

        let horizontal_alignment = match cue.text_alignment {
            // TODO: Start and End alignment is not supported yet
            vtt::TextAlignment::Start => HorizontalAlignment::Left,
            vtt::TextAlignment::End => HorizontalAlignment::Right,
            vtt::TextAlignment::Left => HorizontalAlignment::Left,
            vtt::TextAlignment::Right => HorizontalAlignment::Right,
            vtt::TextAlignment::Center => HorizontalAlignment::Center,
        };

        subtitles.events.push(Event {
            range: cue.start_time..cue.end_time,
            // The text-align property on the (root) list of WebVTT Node Objects must be set to the value in the second cell of the row of the table below whose first cell is the value of the corresponding cue’s WebVTT cue text alignment:
            // Table at https://www.w3.org/TR/webvtt1/#applying-css-properties
            writing_direction: cue.writing_direction,
            text_alignment: cue.text_alignment,
            horizontal_alignment,
            line: cue.line,
            size,
            x: x_position / 100.,
            y: y_position / 100.,
            segments: convert_text(cue.text),
        });
    }

    subtitles
}

pub(crate) struct Layouter {
    subtitles: Rc<Subtitles>,
}

impl Layouter {
    pub fn new(subtitles: Rc<Subtitles>) -> Self {
        Self { subtitles }
    }

    pub fn subtitles(&self) -> &Rc<Subtitles> {
        &self.subtitles
    }

    pub fn layout(&mut self, pass: &mut FrameLayoutPass) -> Result<(), InlineLayoutError> {
        // TODO: This should actually be persisted between frames.
        let mut output = Vec::new();

        let style = {
            let mut result = self.subtitles.root_style.clone();
            result.set::<style::FontSize>(
                // Standard says 5vh, but browser engines use 5vmin.
                // See https://github.com/w3c/webvtt/issues/529
                pass.sctx.video_height.min(pass.sctx.video_width) * 5
                    / 100
                    / pass.sctx.pixel_scale(),
            );
            result
        };

        for event in &self.subtitles.events {
            if !pass.add_event_range(event.range.clone()) {
                continue;
            }

            let (pos, block) = event.layout(pass.sctx, pass.lctx, &style, &mut output)?;
            pass.emit_fragment(pos, block);
        }

        Ok(())
    }
}
