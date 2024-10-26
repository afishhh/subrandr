use std::{collections::HashMap, ops::Range};

use text::{Font, Glyphs, TextExtents, TextRenderer};

pub mod ass;
pub mod srv3;
#[doc(hidden)] // for testing purposes only
pub mod text;
mod util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextWrappingMode {
    None,
}

#[derive(Debug, Clone)]
struct Event {
    start: u32,
    end: u32,
    x: f32,
    y: f32,
    alignment: Alignment,
    text_wrap: TextWrappingMode,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    font: String,
    font_size: f32,
    font_weight: u32,
    italic: bool,
    underline: bool,
    strike_out: bool,
    color: u32,
    text: String,
}

#[derive(Debug)]
pub struct Subtitles {
    events: Vec<Event>,
}

impl Subtitles {
    pub fn empty() -> Self {
        Self { events: vec![] }
    }

    #[doc(hidden)]
    pub fn test_new() -> Subtitles {
        Subtitles {
            events: vec![
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.2,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![
                        Segment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "this ".to_string(),
                        },
                        Segment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0x0000FFFF,
                            text: "is\n".to_string(),
                        },
                        Segment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "mu".to_string(),
                        },
                        Segment {
                            font: "monospace".to_string(),
                            font_size: 48.0,
                            font_weight: 700,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "ltil".to_string(),
                        },
                        Segment {
                            font: "Arial".to_string(),
                            font_size: 80.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "ine".to_string(),
                        },
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.1,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: 0x00FF00AA,
                        text: "this is for comparison".to_string(),
                    }],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.8,
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: 0xFFFFFFFF,
                        text: "this is bold..".to_string(),
                    }],
                },
                // FIXME: Doesn't work, scaling emoji font fails
                // Event {
                //     start: 0,
                //     end: 600000,
                //     x: 0.5,
                //     y: 0.6,
                //     alignment: Alignment::Bottom,
                //     text_wrap: TextWrappingMode::None,
                //     segments: vec![Segment {
                //         font: "emoji".to_string(),
                //         font_size: 32.,
                //         font_weight: 700,
                //         italic: false,
                //         underline: false,
                //         strike_out: false,
                //         color: 0xFFFFFFFF,
                //         text: "😭".to_string(),
                //     }],
                // },
            ],
        }
    }
}

// impl Subtitles {
//     pub fn test_new() -> Subtitles {
//         Subtitles {
//             events: vec![
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.5,
//                     y: 0.0,
//                     alignment: Alignment::Top,
//                     text: "上".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.0,
//                     y: 0.0,
//                     alignment: Alignment::TopLeft,
//                     text: "左上".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.0,
//                     y: 0.5,
//                     alignment: Alignment::Left,
//                     text: "左".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.0,
//                     y: 1.0,
//                     alignment: Alignment::BottomLeft,
//                     text: "左下".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.5,
//                     y: 0.5,
//                     alignment: Alignment::Center,
//                     text: "中".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 1.0,
//                     y: 1.0,
//                     alignment: Alignment::BottomRight,
//                     text: "右下".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 1.0,
//                     y: 0.0,
//                     alignment: Alignment::TopRight,
//                     text: "右上".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 1.0,
//                     y: 0.5,
//                     alignment: Alignment::Right,
//                     text: "右".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 1.0,
//                     y: 1.0,
//                     alignment: Alignment::BottomRight,
//                     text: "右下".to_string(),
//                 },
//                 Event {
//                     start: 0,
//                     end: 3000,
//                     x: 0.5,
//                     y: 1.0,
//                     alignment: Alignment::Bottom,
//                     text: "下".to_string(),
//                 },
//             ],
//         }
//     }
// }

struct MultilineTextShaper {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    font_boundaries: Vec<(text::Font, /* end of segment i */ usize)>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug)]
struct ShapedLineSegment {
    // TODO: Clean this up
    glyphs: (
        /* glyphstring idx */ usize,
        /* glyph range */ Range<usize>,
    ),
    baseline_offset: (i32, i32),
    paint_rect: Rect,
    // Implementation detail
    max_bearing_y: i64,
    corresponding_font_boundary: usize,
    corresponding_input_segment: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Size2 {
    w: u32,
    h: u32,
}

#[derive(Debug)]
struct ShapedLine {
    segments: Vec<ShapedLineSegment>,
    paint_size: Size2,
}

impl MultilineTextShaper {
    fn new() -> Self {
        Self {
            text: String::new(),
            explicit_line_bounaries: Vec::new(),
            font_boundaries: Vec::new(),
            intra_font_segment_splits: Vec::new(),
        }
    }

    fn add(&mut self, mut text: &str, font: &text::Font) {
        while let Some(nl) = text.find('\n') {
            self.text.push_str(&text[..nl]);
            self.explicit_line_bounaries.push(self.text.len());
            text = &text[nl + 1..];
        }
        self.text.push_str(text);

        if let Some((ref last_font, _)) = self.font_boundaries.last() {
            assert_eq!(last_font.dpi(), font.dpi());
        }

        if let Some((ref last_font, ref mut last_end)) = self.font_boundaries.last_mut() {
            if last_font == font {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.font_boundaries.push((font.clone(), self.text.len()));
    }

    fn shape(&self, wrapping: TextWrappingMode) -> (Vec<Glyphs>, Vec<ShapedLine>, TextExtents) {
        assert_eq!(wrapping, TextWrappingMode::None);

        println!("SHAPING V2 TEXT {:?}", self.text);
        println!(
            "SHAPING V2 LINE BOUNDARIES {:?}",
            self.explicit_line_bounaries
        );

        let mut glyphstrings: Vec<Glyphs> = vec![];
        let mut lines: Vec<ShapedLine> = vec![];
        let mut total_extents = TextExtents {
            paint_width: 0,
            paint_height: 0,
        };

        let mut last = 0;
        for line_boundary in self
            .explicit_line_bounaries
            .iter()
            .copied()
            .chain(std::iter::once(self.text.len()))
        {
            // TODO: Where to get spacing?
            // let spacing = font.horizontal_extents().line_gap as i32 + 10;
            let spacing = 10 * 64;
            if !lines.is_empty() {
                total_extents.paint_height += spacing;
            }

            let mut segments: Vec<ShapedLineSegment> = vec![];
            let mut line_extents = TextExtents {
                paint_height: 0,
                paint_width: 0,
            };

            let starting_font_segment = match self
                .font_boundaries
                .binary_search_by_key(&last, |(_, b)| *b)
            {
                Ok(idx) => idx + 1,
                Err(idx) => idx,
            };
            println!(
                "last: {last}, font boundaries: {:?}, binary search result: {}",
                self.font_boundaries
                    .iter()
                    .map(|(_, s)| s)
                    .collect::<Vec<_>>(),
                starting_font_segment
            );

            let mut max_bearing_y = 0;

            for current_font_segment in starting_font_segment..self.font_boundaries.len() {
                let (font, font_boundary) = &self.font_boundaries[current_font_segment];
                let end = (*font_boundary).min(line_boundary);
                let segment_slice = last..end;

                let glyphs = text::shape_text(font, &self.text[segment_slice]);
                let (extents, (trailing_x_advance, _)) = glyphs.compute_extents_ex(font);

                let segment_max_bearing_y = glyphs
                    .iter()
                    .map(|x| font.glyph_extents(x.codepoint()).horiBearingY)
                    .max()
                    .unwrap_or(0);

                max_bearing_y = std::cmp::max(max_bearing_y, segment_max_bearing_y);

                let first_internal_split_idx =
                    match self.intra_font_segment_splits.binary_search(&last) {
                        Ok(idx) => idx + 1,
                        Err(idx) => idx,
                    };

                println!(
                    "last: {last}, end: {end}, intra font splits: {:?}, binary search result: {}",
                    self.intra_font_segment_splits, first_internal_split_idx
                );
                if self
                    .intra_font_segment_splits
                    .get(first_internal_split_idx)
                    .is_none_or(|split| *split >= end)
                {
                    segments.push(ShapedLineSegment {
                        glyphs: (glyphstrings.len(), 0..glyphs.len()),
                        baseline_offset: (
                            line_extents.paint_width / 64,
                            total_extents.paint_height / 64,
                        ),
                        paint_rect: Rect {
                            x: line_extents.paint_width / 64,
                            y: total_extents.paint_height / 64,
                            w: extents.paint_width as u32 / 64,
                            h: extents.paint_height as u32 / 64,
                        },
                        max_bearing_y: segment_max_bearing_y,
                        corresponding_font_boundary: current_font_segment,
                        corresponding_input_segment: current_font_segment
                            + first_internal_split_idx,
                    });
                } else {
                    let mut last_glyph_idx = 0;
                    let mut x = 0;
                    for (i, split_end) in self.intra_font_segment_splits[first_internal_split_idx..]
                        .iter()
                        .copied()
                        .take_while(|idx| *idx < end)
                        .chain(std::iter::once(end))
                        .enumerate()
                    {
                        let end_glyph_idx = split_end - last;
                        let glyph_range = last_glyph_idx..end_glyph_idx;
                        let (extents, (x_advance, _)) =
                            glyphs.compute_extents_for_slice_ex(font, glyph_range.clone());
                        segments.push(ShapedLineSegment {
                            glyphs: (glyphstrings.len(), glyph_range),
                            baseline_offset: (x / 64, total_extents.paint_height / 64),
                            paint_rect: Rect {
                                x: x / 64,
                                y: total_extents.paint_height / 64,
                                w: extents.paint_width as u32 / 64,
                                h: extents.paint_height as u32 / 64,
                            },
                            max_bearing_y: segment_max_bearing_y,
                            corresponding_font_boundary: current_font_segment,
                            corresponding_input_segment: current_font_segment
                                + first_internal_split_idx
                                + i,
                        });
                        last_glyph_idx = end_glyph_idx;
                        x += extents.paint_width + x_advance;
                    }
                }

                line_extents.paint_width += extents.paint_width;

                // FIXME: THIS IS WRONG!!
                //        It will add trailing advance when the last segments contains zero or only invisible glyphs.
                if end != line_boundary {
                    line_extents.paint_width += trailing_x_advance;
                }

                if line_extents.paint_height < extents.paint_height {
                    line_extents.paint_height = extents.paint_height;
                }

                last = end;

                glyphstrings.push(glyphs);

                if end == line_boundary {
                    break;
                }
            }

            debug_assert_eq!(last, line_boundary);

            for segment in segments.iter_mut() {
                segment.baseline_offset.1 += (max_bearing_y / 64) as i32;
                segment.paint_rect.y += ((max_bearing_y - segment.max_bearing_y) / 64) as i32;
            }

            // println!("line boundary {line_slice:?} {current_extents:?}");

            total_extents.paint_height += line_extents.paint_height;
            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine {
                segments,
                paint_size: Size2 {
                    w: (line_extents.paint_width / 64) as u32,
                    h: (line_extents.paint_height / 64) as u32,
                },
            });
        }

        (glyphstrings, lines, total_extents)
    }
}

pub struct Renderer<'a> {
    width: u32,
    height: u32,
    buffer: Vec<u8>,
    text: TextRenderer,
    fonts: text::FontManager,
    // always should be 72 for ass?
    dpi: u32,
    subs: &'a Subtitles,
}

impl<'a> Renderer<'a> {
    pub fn new(width: u32, height: u32, subs: &'a Subtitles, dpi: u32) -> Self {
        Self {
            width,
            height,
            text: TextRenderer::new(),

            fonts: text::FontManager::new(text::font_backend::platform_default().unwrap()),
            buffer: vec![0; (width * height * 4) as usize],
            dpi,
            subs,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.buffer.resize((width * height * 4) as usize, 0);
    }

    #[inline(always)]
    fn pixel(&mut self, x: u32, y: u32) -> &mut [u8; 4] {
        let start = ((y * self.width + x) * 4) as usize;
        assert!(x < self.width && y < self.height);
        (&mut self.buffer[start..start + 4]).try_into().unwrap()
    }

    fn horizontal_line(&mut self, x: u32, y: u32, w: u32, color: u32) {
        let rgba = [
            ((color & 0xFF000000) >> 24) as u8,
            ((color & 0x00FF0000) >> 16) as u8,
            ((color & 0x0000FF00) >> 8) as u8,
            (color & 0x000000FF) as u8,
        ];

        if y >= self.height {
            return;
        }
        for x in x..(x + w).min(self.width) {
            *self.pixel(x, y) = rgba;
        }
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32) {
        let rgba = [
            ((color & 0xFF000000) >> 24) as u8,
            ((color & 0x00FF0000) >> 16) as u8,
            ((color & 0x0000FF00) >> 8) as u8,
            (color & 0x000000FF) as u8,
        ];

        if x >= self.width || y >= self.height {
            return;
        }

        for y in y..(y + h).min(self.height) {
            *self.pixel(x, y) = rgba;
            if x + w < self.width {
                *self.pixel(x + w, y) = rgba;
            }
        }
        for x in x..(x + w).min(self.width) {
            *self.pixel(x, y) = rgba;
            if y + h < self.height {
                *self.pixel(x, y + h) = rgba;
            }
        }
    }

    fn paint_text<'g>(
        &mut self,
        x: u32,
        y: u32,
        font: &text::Font,
        text: impl IntoIterator<Item = text::Glyph<'g>>,
        color: u32,
    ) -> (u32, u32) {
        self.text.paint(
            &mut self.buffer,
            x as usize,
            y as usize,
            self.width as usize,
            self.height as usize,
            (self.width * 4) as usize,
            font,
            text,
            [
                ((color & 0xFF000000) >> 24) as u8,
                ((color & 0x00FF0000) >> 16) as u8,
                ((color & 0x0000FF00) >> 8) as u8,
            ],
            ((color & 0xFF) as f32) / 255.0,
        )
    }

    fn debug_text(
        &mut self,
        x: u32,
        y: u32,
        text: &str,
        alignment: Alignment,
        size: f32,
        color: u32,
    ) {
        let font = self
            .fonts
            .get_or_load("monospace", 400., false)
            .unwrap()
            .with_size(size, self.dpi);
        let shape = text::shape_text(&font, text);
        let (ox, oy) =
            Self::translate_for_aligned_text(&font, true, &shape.compute_extents(&font), alignment);
        self.paint_text(
            x.saturating_add_signed(ox),
            y.saturating_add_signed(oy),
            &font,
            shape.iter(),
            color,
        );
    }

    /// Shapes and lays out multiple lines of horizontal text.
    fn shape_text_multiline(
        &mut self,
        font: &text::Font,
        wrapping: TextWrappingMode,
        text: &str,
    ) -> (
        Glyphs,
        Vec<usize>,
        Vec<((u32, u32), TextExtents)>,
        TextExtents,
    ) {
        assert_eq!(wrapping, TextWrappingMode::None);

        let mut buffer = text::ShapingBuffer::new();
        let mut line_boundaries = vec![];
        let mut line_extents = vec![];
        let mut extents = TextExtents {
            paint_width: 0,
            paint_height: 0,
        };

        for line in text.lines() {
            buffer.add(line);
            line_boundaries.push(buffer.len());
        }

        let glyphs = buffer.shape(font);

        let mut last = 0;
        for boundary in line_boundaries.iter().copied() {
            let slice = last..boundary;

            let current_extents = glyphs.compute_extents_for_slice_ex(font, slice.clone()).0;
            println!("line boundary {slice:?} {current_extents:?}");
            let spacing = font.horizontal_extents().line_gap as i32 + 10;
            if !line_extents.is_empty() {
                extents.paint_height += spacing;
            }

            line_extents.push(((0, extents.paint_height as u32), current_extents));

            extents.paint_height += current_extents.paint_height;
            extents.paint_width = std::cmp::max(extents.paint_width, current_extents.paint_width);

            last = boundary + 1;
        }

        (glyphs, line_boundaries, line_extents, extents)
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextExtents,
        alignment: Alignment,
    ) -> (i32, i32) {
        assert!(horizontal);

        enum Vertical {
            Top,
            BaselineCentered,
            Bottom,
        }

        enum Horizontal {
            Left,
            Center,
            Right,
        }

        let (vertical, horizontal) = match alignment {
            Alignment::TopLeft => (Vertical::Top, Horizontal::Left),
            Alignment::Top => (Vertical::Top, Horizontal::Center),
            Alignment::TopRight => (Vertical::Top, Horizontal::Right),
            Alignment::Left => (Vertical::BaselineCentered, Horizontal::Left),
            Alignment::Center => (Vertical::BaselineCentered, Horizontal::Center),
            Alignment::Right => (Vertical::BaselineCentered, Horizontal::Right),
            Alignment::BottomLeft => (Vertical::Bottom, Horizontal::Left),
            Alignment::Bottom => (Vertical::Bottom, Horizontal::Center),
            Alignment::BottomRight => (Vertical::Bottom, Horizontal::Right),
        };

        // TODO: Numbers chosen arbitrarily
        let ox = match horizontal {
            Horizontal::Left => -font.horizontal_extents().descender / 64 / 2,
            Horizontal::Center => -extents.paint_width / 128,
            Horizontal::Right => (-extents.paint_width + font.horizontal_extents().descender) / 64,
        };

        let oy = match vertical {
            Vertical::Top => font.horizontal_extents().ascender / 64,
            Vertical::BaselineCentered => 0,
            Vertical::Bottom => font.horizontal_extents().descender / 64,
        };

        (ox, oy)
    }

    #[allow(warnings)]
    pub fn render(&mut self, t: u32) {
        if self.width == 0 || self.height == 0 {
            return;
        }

        self.buffer.fill(0);

        // for y in 0..(self.height / 2) {
        //     for x in 0..(self.width / 2) {
        //         let pixel = self.pixel(x, y);
        //         *pixel = [255, 0, 0, 100];
        //     }
        // }

        // let font = self.face.with_size(256.0);
        // let shaped = self.text.shape_text(&font, "world hello");
        // dbg!(self.text.compute_extents(&font, &shaped));
        //
        // self.paint_text(100, 400, &font, &shaped, 0.85);
        // self.paint_text(100, 600, &font, &shaped, 0.85);
        // self.paint_text(0, 100, &font, &shaped, 0.85);

        self.debug_text(
            self.width,
            0,
            &format!("{}x{} dpi:{}", self.width, self.height, self.dpi),
            Alignment::TopRight,
            16.0,
            0xFFFFFFFF,
        );

        {
            for event in self
                .subs
                .events
                .iter()
                .filter(|ev| ev.start <= t && ev.end > t)
            {
                println!("{event:?}");
                let mut x = (self.width as f32 * event.x) as u32;
                let mut y = (self.height as f32 * event.y) as u32;

                let mut shaper = MultilineTextShaper::new();
                let mut segment_fonts = vec![];
                for segment in event.segments.iter() {
                    let font = self
                        .fonts
                        .get_or_load(&segment.font, segment.font_weight as f32, segment.italic)
                        .unwrap()
                        .with_size(segment.font_size, self.dpi);

                    println!("SHAPING V2 INPUT SEGMENT: {:?} {:?}", segment.text, font);

                    shaper.add(&segment.text, &font);
                    segment_fonts.push(font);
                }

                let (glyphstrings, lines, extents) = shaper.shape(TextWrappingMode::None);
                println!("SHAPING V2 RESULT: {:?} {:#?}", extents, lines);

                self.rect(
                    x - 1,
                    y - 1,
                    (extents.paint_width / 64) as u32 + 1,
                    (extents.paint_height / 64) as u32 + 1,
                    0xFF00FFFF,
                );

                self.debug_text(
                    x.saturating_add_signed(extents.paint_width / 128),
                    y.saturating_add_signed(extents.paint_height / 64) + 20,
                    &format!(
                        "x:{} y:{} w:{} h:{}",
                        x,
                        y,
                        extents.paint_width / 64,
                        extents.paint_height / 64
                    ),
                    Alignment::Top,
                    16.0,
                    0xFF00FFFF,
                );

                let mut last = 0;
                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];
                    let glyphs_it = glyphstrings[shaped_segment.glyphs.0]
                        .iter_slice(shaped_segment.glyphs.1.start, shaped_segment.glyphs.1.end);

                    let paint_box = (
                        x.checked_add_signed(shaped_segment.paint_rect.x).unwrap(),
                        y.checked_add_signed(shaped_segment.paint_rect.y).unwrap(),
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1,
                        &format!(
                            "{},{}",
                            x.saturating_add_signed(shaped_segment.paint_rect.x),
                            y.saturating_add_signed(shaped_segment.paint_rect.y)
                        ),
                        Alignment::BottomLeft,
                        16.0,
                        0xFF0000FF,
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1 + shaped_segment.paint_rect.h,
                        &format!(
                            "{},{}",
                            x.saturating_add_signed(shaped_segment.baseline_offset.0),
                            y.saturating_add_signed(shaped_segment.baseline_offset.1)
                        ),
                        Alignment::TopLeft,
                        16.0,
                        0xFF0000FF,
                    );

                    self.debug_text(
                        paint_box.0 + shaped_segment.paint_rect.w,
                        paint_box.1,
                        &format!("{:.0}pt", segment.font_size),
                        Alignment::BottomRight,
                        16.0,
                        0xFFFFFFFF,
                    );

                    self.rect(
                        paint_box.0,
                        paint_box.1,
                        shaped_segment.paint_rect.w as u32,
                        shaped_segment.paint_rect.h as u32,
                        0x0000FFFF,
                    );

                    self.horizontal_line(
                        paint_box.0,
                        y.checked_add_signed(shaped_segment.baseline_offset.1)
                            .unwrap(),
                        shaped_segment.paint_rect.w as u32,
                        0x00FF00FF,
                    );

                    let x = x
                        .checked_add_signed(shaped_segment.baseline_offset.0)
                        .unwrap();
                    let y = y
                        .checked_add_signed(shaped_segment.baseline_offset.1)
                        .unwrap();

                    let (nx, ny) = self.paint_text(
                        x,
                        y,
                        &segment_fonts[shaped_segment.corresponding_input_segment],
                        glyphs_it,
                        segment.color,
                    );
                }
            }
        }

        // let shaped = self
        //     .text
        //     .shape_text(&font, "あああああLLlloああああああああ");
        // self.paint_text(50, 750, &font, &shaped, 0.85);
        //
        // let font = self.face.with_size(32.0);
        // let shaped = self.text.shape_text(&font, "全角文字");
        // self.paint_text(200, 300, &font, &shaped, 1.0);
    }

    pub fn bitmap(&self) -> &[u8] {
        &self.buffer
    }
}
