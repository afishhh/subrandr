use crate::math::Point2;

mod text;
pub(super) use text::*;

#[derive(Debug, Clone, PartialEq)]
pub struct Captions<'a> {
    pub(super) cues: Vec<Cue<'a>>,
    pub(super) regions: Vec<Region<'a>>,
    pub(super) stylesheets: Vec<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WritingDirection {
    Horizontal,
    VerticalGrowingLeft,
    VerticalGrowingRight,
}

impl WritingDirection {
    #[must_use]
    pub(super) fn is_horizontal(&self) -> bool {
        matches!(self, Self::Horizontal)
    }

    #[must_use]
    pub(super) fn is_vertical_growing_left(&self) -> bool {
        matches!(self, Self::VerticalGrowingLeft)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Line {
    Auto,
    Lines(f64),
    Percentage(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LineAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Position {
    Auto,
    Percentage(f64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PositionAlignment {
    LineLeft,
    Center,
    LineRight,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TextAlignment {
    Start,
    Center,
    End,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Cue<'a> {
    pub track_identifier: &'a str,
    pub pause_on_exit: bool,
    pub region: Option<&'a str>,
    pub writing_direction: WritingDirection,
    pub line: Line,
    pub line_alignment: LineAlignment,
    pub position: Position,
    pub position_alignment: PositionAlignment,
    pub size: f64,
    pub text_alignment: TextAlignment,
    pub start_time: u32,
    pub end_time: u32,
    pub text: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Region<'a> {
    pub identifier: &'a str,
    pub width: f64,
    pub lines: u32,
    pub anchor_point: Point2<f64>,
    pub viewport_anchor_point: Point2<f64>,
    pub scroll_value: ScrollValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollValue {
    None,
    Up,
}

struct ParsingBuffer<'a> {
    text: &'a str,
}

impl<'a> ParsingBuffer<'a> {
    fn new(text: &'a str) -> Self {
        Self { text }
    }

    fn take_str(&mut self, value: &str) -> bool {
        if let Some(new_text) = self.text.strip_prefix(value) {
            self.text = new_text;
            true
        } else {
            false
        }
    }

    fn take_any<const N: usize>(&mut self, chars: [char; N]) -> bool {
        if let Some(new_text) = self.text.strip_prefix(chars) {
            self.text = new_text;
            true
        } else {
            false
        }
    }

    fn take(&mut self, chr: char) -> bool {
        self.take_any([chr])
    }

    fn peek(&mut self, chr: char) -> bool {
        self.text.starts_with(chr)
    }

    fn take_linefeed(&mut self) -> bool {
        if let Some(new_text) = self.text.strip_prefix('\r') {
            let new_text = new_text.strip_prefix('\n').unwrap_or(new_text);
            self.text = new_text;
            true
        } else if let Some(new_text) = self.text.strip_prefix('\n') {
            self.text = new_text;
            true
        } else {
            false
        }
    }

    fn collect_whitespace(&mut self) {
        let end = self
            .text
            .bytes()
            .position(|b| !b.is_ascii_whitespace())
            .unwrap_or(self.text.len());
        self.text = &self.text[end..];
    }

    fn collect_digits(&mut self) -> &str {
        let end = self
            .text
            .bytes()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(self.text.len());
        let result = &self.text[..end];
        self.text = &self.text[end..];
        result
    }

    fn collect_line_and_linefeed(&mut self) -> &'a str {
        let i = self
            .text
            .bytes()
            .position(|c| matches!(c, b'\r' | b'\n'))
            .unwrap_or(self.text.len());

        let result = &self.text[..i];
        if self.text.as_bytes().get(i..i + 2) == Some(b"\r\n") {
            self.text = &self.text[i + 2..];
        } else {
            self.text = &self.text[i + 1..];
        }

        result
    }

    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

// https://www.w3.org/TR/webvtt1/#collect-a-webvtt-timestamp
fn collect_timestamp(input: &mut ParsingBuffer) -> Option<u32> {
    let mut is_most_significant_hours = false;

    let string = input.collect_digits();
    let value_1 = string.parse::<u32>().ok()?;
    if string.len() != 2 || value_1 > 59 {
        is_most_significant_hours = true;
    }

    if !input.take(':') {
        return None;
    }

    let string = input.collect_digits();
    if string.len() != 2 {
        return None;
    }

    let value_2 = string.parse::<u32>().unwrap();
    let (value_1, value_2, value_3) = if is_most_significant_hours || input.peek(':') {
        if !input.take(':') {
            return None;
        }

        let string = input.collect_digits();
        if string.len() != 2 {
            return None;
        }

        (value_1, value_2, string.parse::<u32>().unwrap())
    } else {
        (0, value_1, value_2)
    };

    if !input.take('.') {
        return None;
    }

    let string = input.collect_digits();
    if string.len() != 3 {
        return None;
    }

    let value_4 = string.parse::<u32>().unwrap();
    if value_2 > 59 || value_3 > 59 {
        return None;
    }

    let result = ((value_1 * 60 + value_2) * 60 + value_3) * 1000 + value_4;

    Some(result)
}

// https://www.w3.org/TR/webvtt1/#parse-a-percentage-string
fn parse_percentage(input: &str) -> Option<f64> {
    // https://www.w3.org/TR/webvtt1/#webvtt-percentage
    if let Some(non_digit) = input.bytes().position(|b| !b.is_ascii_digit()) {
        if input.as_bytes()[non_digit] != b'.' {
            return None;
        }
        if input[non_digit + 1..].bytes().any(|c| !c.is_ascii_digit()) {
            return None;
        }
    }

    if let Some(value) = input.strip_suffix('%') {
        value.parse::<f64>().ok()
    } else {
        None
    }
}

// https://www.w3.org/TR/webvtt1/#parse-the-webvtt-cue-settings
fn parse_cue_settings<'a>(remainder: &'a str, cue: &mut Cue<'a>) {
    let settings = remainder.split_ascii_whitespace();

    for setting in settings {
        let Some((name, value)) = setting.split_once(':') else {
            continue;
        };

        if name.is_empty() || value.is_empty() {
            continue;
        }

        match name {
            "region" => {
                cue.region = Some(value);
            }
            "vertical" => {
                match value {
                    "rl" => cue.writing_direction = WritingDirection::VerticalGrowingLeft,
                    "lr" => cue.writing_direction = WritingDirection::VerticalGrowingRight,
                    _ => (),
                }
                // TODO: If cue’s WebVTT cue writing direction is not horizontal, let cue’s WebVTT cue region be null (there are no vertical regions).
                //      If "there are no vertical regions" does this mean that
                //      setting region to a value after setting a vertical writing dir
                //      is illegal and should be ignored?
                if cue.writing_direction != WritingDirection::Horizontal {
                    cue.region = None;
                }
            }
            "line" => {
                let (linepos, linealign) = match value.split_once(',') {
                    Some((before, after)) => (before, Some(after)),
                    None => (value, None),
                };

                if !linepos.contains(|c: char| c.is_ascii_digit()) {
                    continue;
                }

                let number = if let Some(number) = parse_percentage(linepos) {
                    Line::Percentage(number)
                } else if let Ok(number) = linepos.parse::<f64>() {
                    Line::Lines(number)
                } else {
                    continue;
                };

                match linealign {
                    Some("start") => cue.line_alignment = LineAlignment::Start,
                    Some("center") => cue.line_alignment = LineAlignment::Center,
                    Some("end") => cue.line_alignment = LineAlignment::End,
                    Some(_) => continue,
                    None => (),
                }

                cue.line = number;
                cue.region = None;
            }
            "position" => {
                let (colpos, colalign) = match value.split_once(',') {
                    Some((before, after)) => (before, Some(after)),
                    None => (value, None),
                };

                let Some(number) = parse_percentage(colpos) else {
                    continue;
                };

                match colalign {
                    Some("line-left") => cue.position_alignment = PositionAlignment::LineLeft,
                    Some("center") => cue.position_alignment = PositionAlignment::Center,
                    Some("line-right") => cue.position_alignment = PositionAlignment::LineRight,
                    Some(_) => continue,
                    None => (),
                }

                cue.position = Position::Percentage(number);
            }
            "size" => {
                if let Some(number) = parse_percentage(value) {
                    cue.size = number;
                }

                if cue.size != 100. {
                    cue.region = None;
                }
            }
            "align" => match value {
                "start" => cue.text_alignment = TextAlignment::Start,
                "center" => cue.text_alignment = TextAlignment::Center,
                "end" => cue.text_alignment = TextAlignment::End,
                "left" => cue.text_alignment = TextAlignment::Left,
                "right" => cue.text_alignment = TextAlignment::Right,
                _ => (),
            },
            _ => (),
        }
    }
}

// https://www.w3.org/TR/webvtt1/#collect-webvtt-cue-timings-and-settings
fn collect_cue_timings_and_settings<'a>(
    input: &mut ParsingBuffer<'a>,
    cue: &mut Cue<'a>,
) -> Option<()> {
    input.collect_whitespace();

    let start_time = collect_timestamp(input)?;
    cue.start_time = start_time;

    input.collect_whitespace();
    if !input.take_str("-->") {
        return None;
    }
    input.collect_whitespace();

    let end_time = collect_timestamp(input)?;
    cue.end_time = end_time;

    parse_cue_settings(input.text, cue);

    Some(())
}

// https://www.w3.org/TR/webvtt1/#region-settings-parsing
fn collect_webvtt_region_settings<'a>(input: &'a str, region: &mut Region<'a>) {
    let settings = input.split_ascii_whitespace();

    for setting in settings {
        let Some((name, value)) = setting.split_once(':') else {
            continue;
        };

        if name.is_empty() || value.is_empty() {
            continue;
        }

        match name {
            "id" => {
                region.identifier = value;
            }
            "width" => {
                if let Some(percentage) = parse_percentage(value) {
                    region.width = percentage;
                }
            }
            "lines" => {
                if value.contains(|c: char| !c.is_ascii_digit()) {
                    continue;
                }

                // TODO: What do when out of range?
                if let Ok(value) = value.parse::<u32>() {
                    region.lines = value;
                }
            }
            "regionanchor" => {
                let Some((anchor_x, anchor_y)) = value.split_once(',') else {
                    continue;
                };

                let (Some(x), Some(y)) = (parse_percentage(anchor_x), parse_percentage(anchor_y))
                else {
                    continue;
                };

                region.anchor_point = Point2::new(x, y);
            }
            "viewportanchor" => {
                let Some((viewport_anchor_x, viewport_anchor_y)) = value.split_once(',') else {
                    continue;
                };

                let (Some(x), Some(y)) = (
                    parse_percentage(viewport_anchor_x),
                    parse_percentage(viewport_anchor_y),
                ) else {
                    continue;
                };

                region.viewport_anchor_point = Point2::new(x, y);
            }
            "scroll" => {
                if value == "up" {
                    region.scroll_value = ScrollValue::Up;
                }
            }
            _ => (),
        }
    }
}

enum Block<'a> {
    Cue(Cue<'a>),
    Region(Region<'a>),
    Stylesheet(&'a str),
}

// https://www.w3.org/TR/webvtt1/#collect-a-webvtt-block
fn collect_block<'a>(input: &mut ParsingBuffer<'a>, in_header: bool) -> Option<Block<'a>> {
    let mut buffer = "";
    let mut line_count = 0;
    let mut seen_eof = false;
    let mut seen_cue = false;
    let mut seen_arrow = false;
    let mut previous_position = input.text;
    let mut cue = None;
    let mut region = None;
    let mut stylesheet = None;

    loop {
        // 1. collect a sequence of code points that are not U+000A LINE FEED (LF) characters. Let line be those characters, if any.
        let line = input.collect_line_and_linefeed();

        // 2. Increment line count by 1.
        line_count += 1;

        // 3. If position is past the end of input, let seen EOF be true. Otherwise, the character indicated by position is a U+000A LINE FEED (LF) character; advance position to the next character in input.
        if input.is_empty() {
            seen_eof = true;
        }

        // 4. If line contains the three-character substring "-->", then run these substeps:
        if line.contains("-->") {
            // 5. If in header is not set and at least one of the following conditions are true:
            if !in_header && /*rustfmt suppressor*/ (
                // line count is 1
                (line_count == 1) ||
                // line count is 2 and seen arrow is false
                (line_count == 2 && !seen_arrow)
            ) {
                // Let seen arrow be true.
                seen_arrow = true;

                // Let previous position be position.
                previous_position = input.text;

                // Cue creation: Let cue be a new WebVTT cue and initialize it as follows:
                let mut cue_ = Cue {
                    // Let cue’s text track cue identifier be buffer.
                    track_identifier: buffer,
                    // Let cue’s text track cue pause-on-exit flag be false.
                    pause_on_exit: false,
                    // Let cue’s WebVTT cue region be null.
                    region: None,
                    // Let cue’s WebVTT cue writing direction be horizontal.
                    writing_direction: WritingDirection::Horizontal,
                    // Let cue’s WebVTT cue snap-to-lines flag be true.
                    // Let cue’s WebVTT cue line be auto.
                    line: Line::Auto,
                    // Let cue’s WebVTT cue line alignment be start alignment.
                    line_alignment: LineAlignment::Start,
                    // Let cue’s WebVTT cue position be auto.
                    position: Position::Auto,
                    // Let cue’s WebVTT cue position alignment be auto.
                    position_alignment: PositionAlignment::Auto,
                    // Let cue’s WebVTT cue size be 100.
                    size: 100.,
                    // Let cue’s WebVTT cue text alignment be center alignment.
                    text_alignment: TextAlignment::Center,
                    start_time: 0,
                    end_time: 0,
                    // Let cue’s cue text be the empty string.
                    text: "",
                };

                // Collect WebVTT cue timings and settings from line using regions for cue. If that fails, let cue be null. Otherwise, let buffer be the empty string and let seen cue be true.
                if collect_cue_timings_and_settings(&mut ParsingBuffer::new(line), &mut cue_)
                    .is_some()
                {
                    cue = Some(cue_);
                    seen_cue = true;
                    buffer = "";
                } else {
                    cue = None;
                }
            } else {
                // Otherwise, let position be previous position and break out of loop.
                input.text = previous_position;
                break;
            }
        } else if line.is_empty() {
            // Otherwise, if line is the empty string, break out of loop.
            break;
        } else {
            // Otherwise, run these substeps:
            // If in header is not set and line count is 2, run these substeps:
            if !in_header && line_count == 2 {
                // If seen cue is false and buffer starts with the substring "STYLE", and the remaining characters in buffer (if any) are all ASCII whitespace, then run these substeps:
                if !seen_cue
                    && buffer
                        .strip_prefix("STYLE")
                        .is_some_and(|remaining| remaining.bytes().all(|b| b.is_ascii_whitespace()))
                {
                    // Let stylesheet be the result of creating a CSS style sheet, with the following properties:
                    stylesheet = Some(());
                    // Let buffer be the empty string.
                    buffer = "";
                } else if !seen_cue
                    && buffer
                        .strip_prefix("REGION")
                        .is_some_and(|remaining| remaining.bytes().all(|b| b.is_ascii_whitespace()))
                {
                    // Otherwise, if seen cue is false and buffer starts with the substring "REGION", and the remaining characters in buffer (if any) are all ASCII whitespace, then run these substeps:
                    // Region creation: Let region be a new WebVTT region.
                    region = Some(Region {
                        // Let region’s identifier be the empty string.
                        identifier: "",
                        // Let region’s width be 100.
                        width: 100.,
                        // Let region’s lines be 3.
                        lines: 3,
                        // Let region’s anchor point be (0,100).
                        anchor_point: Point2::new(0., 100.),
                        // Let region’s viewport anchor point be (0,100).
                        viewport_anchor_point: Point2::new(0., 100.),
                        // Let region’s scroll value be none.
                        scroll_value: ScrollValue::None,
                    });
                    // Let buffer be the empty string.
                    buffer = "";
                }
            }

            // If buffer is not the empty string, append a U+000A LINE FEED (LF) character to buffer.
            // Append line to buffer.
            // NOTE: This is handled somewhat differently from the standard to ensure content can still be
            //       borrowed as a contigous slice of the input text. It should be fully equivalent though
            //       because I can't see how control would have to flow through this function to make buffer
            //       ever be a non-contigous snippet of the input text.
            if buffer.is_empty() {
                buffer = line;
            } else {
                // FIXME: Actually use indicies instead so this can be safe?
                buffer = unsafe {
                    std::str::from_utf8_unchecked(std::slice::from_raw_parts(
                        buffer.as_ptr(),
                        line.as_bytes().as_ptr_range().end.addr()
                            - buffer.as_bytes().as_ptr().addr(),
                    ))
                };
            }

            // Let previous position be position.
            previous_position = input.text;
        }

        // If seen EOF is true, break out of loop.
        if seen_eof {
            break;
        }
    }

    // If cue is not null, let the cue text of cue be buffer, and return cue.
    if let Some(mut cue) = cue {
        cue.text = buffer;
        Some(Block::Cue(cue))
    } else if let Some(()) = stylesheet {
        // Otherwise, if stylesheet is not null, then Parse a stylesheet from buffer. If it returned a list of rules, assign the list as stylesheet’s CSS rules; otherwise, set stylesheet’s CSS rules to an empty list. Finally, return stylesheet.
        // NOTE: Currently stylesheets are not supported so they're just passed through as is.
        Some(Block::Stylesheet(buffer))
    } else if let Some(mut region) = region {
        // Otherwise, if region is not null, then collect WebVTT region settings from buffer using region for the results. Construct a WebVTT Region Object from region, and return it.
        collect_webvtt_region_settings(buffer, &mut region);
        Some(Block::Region(region))
    } else {
        // Otherwise, return null.
        None
    }
}

fn consume_magic(input: &mut ParsingBuffer) -> bool {
    // Optional UTF-8 BOM
    _ = input.take('\u{feff}');

    if !input.take_str("WEBVTT") {
        return false;
    }

    if input.take_any([' ', '\t']) {
        input.collect_line_and_linefeed();
    } else if input.take_linefeed() {
    } else {
        return false;
    }

    true
}

pub fn probe(content: &str) -> bool {
    consume_magic(&mut ParsingBuffer::new(content))
}

// https://www.w3.org/TR/webvtt1/#webvtt-parser-algorithm
pub fn parse<'a>(input: &'a str) -> Option<Captions<'a>> {
    let mut input = ParsingBuffer::new(input);

    if !consume_magic(&mut input) {
        return None;
    }

    let mut output = Captions {
        cues: Vec::new(),
        regions: Vec::new(),
        stylesheets: Vec::new(),
    };

    if !input.take_linefeed() {
        _ = collect_block(&mut input, true);
    }

    while input.take_linefeed() {}

    while !input.is_empty() {
        match collect_block(&mut input, false) {
            Some(Block::Cue(cue)) => output.cues.push(cue),
            Some(Block::Region(region)) => output.regions.push(region),
            Some(Block::Stylesheet(stylesheet)) => output.stylesheets.push(stylesheet),
            None => (),
        }

        while input.take_linefeed() {}
    }

    Some(output)
}
