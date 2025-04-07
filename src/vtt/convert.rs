use crate::{
    color::BGRA8,
    math::{I26Dot6, Point2, Rect2, Vec2},
    vtt, EventExtra, Layouter, Subrandr, SubtitleContext,
};

#[derive(Debug)]
struct Class;

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
    fn snap_to_lines(&self) -> bool {
        match self.line {
            vtt::Line::Auto | vtt::Line::Lines(_) => true,
            vtt::Line::Percentage(_) => false,
        }
    }

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
pub(crate) struct VttEvent {
    writing_direction: vtt::WritingDirection,
    text_alignment: vtt::TextAlignment,
    line: vtt::Line,
    size: f64,
    x: f64,
    y: f64,
}

impl crate::SubtitleClass for Class {
    fn get_name(&self) -> &'static str {
        "vtt"
    }

    fn get_font_size(
        &self,
        ctx: &crate::SubtitleContext,
        _event: &crate::Event,
        _segment: &crate::TextSegment,
    ) -> f32 {
        // Standard says 5vh, but browser engines use 5vmin.
        // See https://github.com/w3c/webvtt/issues/529
        let pixels = ctx.player_height().min(ctx.player_width()) * 0.05 * 96.0 / ctx.ppi() as f32;
        pixels * 96.0 / 72.0
    }

    fn create_layouter(&self) -> Box<dyn crate::Layouter> {
        Box::new(VttLayouter { output: Vec::new() })
    }
}

struct VttLayouter {
    output: Vec<Rect2<I26Dot6>>,
}

impl VttLayouter {
    // https://www.w3.org/TR/webvtt1/#processing-cue-settings
    fn process_cue_settings_adjust_boxes(
        &mut self,
        ctx: &SubtitleContext,
        result: &mut Point2<f32>,
        lines: &mut Vec<crate::text::layout::ShapedLine>,
        total_rect: Rect2<I26Dot6>,
        extra: &VttEvent,
    ) {
        match extra.line.computed(0) {
            ComputedLine::Percentage(_) => todo!(),
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
                    lines[0].bounding_rect.height()
                } else {
                    // 2. Vertical: Let step be the width of the first line box in boxes.
                    lines[0].bounding_rect.width()
                };

                // 3. If step is zero, then jump to the step labeled done positioning below.
                if step == I26Dot6::ZERO {
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
                let mut specified_position = lines.to_vec();

                // Let title area be a box that covers all of the video’s rendering area.
                let title_area = Rect2::new(
                    Point2::ZERO,
                    Point2::new(
                        I26Dot6::from_f32(ctx.video_width),
                        I26Dot6::from_f32(ctx.video_height),
                    ),
                );

                let mut switched = false;
                loop {
                    let mut done = true;
                    'check: for line in &lines[..] {
                        let effective_rect = line.bounding_rect.translate(Vec2::new(
                            I26Dot6::from_f32(result.x),
                            I26Dot6::from_f32(result.y),
                        ));

                        if !title_area.includes(effective_rect) {
                            done = false;
                            break 'check;
                        }

                        for out in &self.output {
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
                    let first_line_box = lines[0].bounding_rect.translate(Vec2::new(
                        I26Dot6::from_f32(result.x),
                        I26Dot6::from_f32(result.y),
                    ));
                    if extra.writing_direction.is_horizontal() {
                        // Horizontal: If step is negative and the top of the first line box in boxes is now above the top of the title area, or if step is positive and the bottom of the first line box in boxes is now below the bottom of the title area, jump to the step labeled switch direction.
                        if (step < I26Dot6::ZERO && first_line_box.min.y < title_area.min.y)
                            || (step > I26Dot6::ZERO && first_line_box.max.y > title_area.max.y)
                        {
                            switch_direction = true;
                        }
                    } else {
                        // Vertical: If step is negative and the left edge of the first line box in boxes is now to the left of the left edge of the title area, or if step is positive and the right edge of the first line box in boxes is now to the right of the right edge of the title area, jump to the step labeled switch direction.
                        if (step < I26Dot6::ZERO && first_line_box.min.x < title_area.min.x)
                            || (step > I26Dot6::ZERO && first_line_box.max.x > title_area.max.x)
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
                            println!("gave up!");
                            lines.clear();
                            return;
                        }

                        // Otherwise, move all the boxes in boxes back to their specified position as determined in the earlier step.
                        *lines = std::mem::take(&mut specified_position);

                        // Negate step.
                        step = -step;

                        // Set switched to true.
                        switched = true;

                        // Jump back to the step labeled step loop.
                    }
                }
            }
        }
    }
}

impl Layouter for VttLayouter {
    fn wrap_width(&self, ctx: &SubtitleContext, event: &crate::Event) -> f32 {
        let EventExtra::Vtt(extra) = &event.extra else {
            panic!("VttLayouter::wrap_width received foreign event {:?}", event);
        };

        ctx.video_width * extra.size as f32 / 100.
    }

    fn layout(
        &mut self,
        ctx: &SubtitleContext,
        lines: &mut Vec<crate::text::layout::ShapedLine>,
        total_rect: &mut crate::math::Rect2<crate::math::I26Dot6>,
        event: &crate::Event,
    ) -> crate::math::Point2f {
        let EventExtra::Vtt(extra) = &event.extra else {
            panic!("VttLayouter::layout received foreign event {:?}", event);
        };

        if lines.is_empty() {
            return Point2::ZERO;
        }

        let mut result = Point2::new(
            extra.x as f32 * ctx.video_width,
            extra.y as f32 * ctx.video_height,
        );

        if extra.writing_direction.is_horizontal() {
            // emulate width = size vw
            result.x += match extra.text_alignment {
                vtt::TextAlignment::Start | vtt::TextAlignment::Left => 0.,
                vtt::TextAlignment::Center => ctx.video_width * extra.size as f32 / 200.,
                vtt::TextAlignment::End | vtt::TextAlignment::Right => {
                    ctx.video_width * extra.size as f32 / 100.
                }
            };
        } else {
            // emulate height = size vh
            result.y += match extra.text_alignment {
                vtt::TextAlignment::Start | vtt::TextAlignment::Left => 0.,
                vtt::TextAlignment::Center => ctx.video_height * extra.size as f32 / 200.,
                vtt::TextAlignment::End | vtt::TextAlignment::Right => {
                    ctx.video_height * extra.size as f32 / 100.
                }
            };
        }

        self.process_cue_settings_adjust_boxes(ctx, &mut result, lines, *total_rect, extra);

        for line in &lines[..] {
            self.output.push(line.bounding_rect.translate(Vec2::new(
                I26Dot6::from_f32(result.x),
                I26Dot6::from_f32(result.y),
            )));
        }

        result.x += ctx.padding_left;
        result.y += ctx.padding_top;

        result
    }
}

pub fn convert(_sbr: &Subrandr, captions: vtt::Captions) -> crate::Subtitles {
    let mut subtitles = crate::Subtitles {
        class: &Class,
        events: Vec::new(),
    };

    for cue in captions.cues {
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

        // TODO: Split crate::Alignment into two.
        let alignment = crate::Alignment::from_parts(
            match cue.text_alignment {
                // TODO: Start and End alignment is not supported yet
                vtt::TextAlignment::Start => crate::HorizontalAlignment::Left,
                vtt::TextAlignment::End => crate::HorizontalAlignment::Right,
                vtt::TextAlignment::Left => crate::HorizontalAlignment::Left,
                vtt::TextAlignment::Right => crate::HorizontalAlignment::Right,
                vtt::TextAlignment::Center => crate::HorizontalAlignment::Center,
            },
            crate::VerticalAlignment::Top,
        );

        subtitles.events.push(crate::Event {
            start: cue.start_time,
            end: cue.end_time,
            // The text-align property on the (root) list of WebVTT Node Objects must be set to the value in the second cell of the row of the table below whose first cell is the value of the corresponding cue’s WebVTT cue text alignment:
            // Table at https://www.w3.org/TR/webvtt1/#applying-css-properties
            alignment,
            text_wrap: crate::TextWrapMode::Normal,
            segments: vec![crate::Segment::Text(crate::TextSegment {
                font: vec!["sans-serif".to_owned()],
                font_size: f32::INFINITY,
                font_weight: 400,
                italic: false,
                decorations: crate::TextDecorations::default(),
                color: BGRA8::WHITE,
                background_color: BGRA8::ZERO,
                text: cue.text.into(),
                shadows: Vec::new(),
                ruby: crate::Ruby::None,
            })],
            extra: crate::EventExtra::Vtt(VttEvent {
                writing_direction: cue.writing_direction,
                text_alignment: cue.text_alignment,
                line: cue.line,
                size,
                x: x_position,
                y: y_position,
            }),
        });
    }

    subtitles
}
