use std::ops::Range;

use text::{Font, Glyphs, TextExtents, TextRenderer};

pub mod ass;
pub mod srv3;

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
                            font: "ignored".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "this ".to_string(),
                        },
                        Segment {
                            font: "ignored".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0x0000FFFF,
                            text: "is\n".to_string(),
                        },
                        Segment {
                            font: "ignored".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "multiline".to_string(),
                        },
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.2,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment {
                        font: "ignored".to_string(),
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
                        font: "ignored".to_string(),
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: 0xFFFFFFFF,
                        text: "this is bold..".to_string(),
                    }],
                },
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
    offset: (i32, i32),
    extents: TextExtents,
    corresponding_input_segment: usize,
}

#[derive(Debug)]
struct ShapedLine {
    segments: Vec<ShapedLineSegment>,
    extents: TextExtents,
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

        let mut glyphstrings = vec![];
        let mut lines = vec![];
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
            let mut segments = vec![];
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

            for current_font_segment in starting_font_segment..self.font_boundaries.len() {
                let (font, font_boundary) = &self.font_boundaries[current_font_segment];
                let end = (*font_boundary).min(line_boundary);
                let segment_slice = last..end;

                let glyphs = text::shape_text(font, &self.text[segment_slice]);
                let extents = glyphs.compute_extents(font);

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
                        offset: (line_extents.paint_width, total_extents.paint_height),
                        extents,
                        corresponding_input_segment: current_font_segment
                            + first_internal_split_idx,
                    });
                } else {
                    let mut last_glyph_idx = 0;
                    for (i, split_end) in self.intra_font_segment_splits[first_internal_split_idx..]
                        .iter()
                        .copied()
                        .take_while(|idx| *idx < end)
                        .chain(std::iter::once(end))
                        .enumerate()
                    {
                        let end_glyph_idx = split_end - last;
                        let glyph_range = last_glyph_idx..end_glyph_idx;
                        segments.push(ShapedLineSegment {
                            glyphs: (glyphstrings.len(), glyph_range.clone()),
                            offset: (line_extents.paint_width, total_extents.paint_height),
                            extents: glyphs.compute_extents_for_slice(font, glyph_range),
                            corresponding_input_segment: current_font_segment
                                + first_internal_split_idx
                                + i,
                        });
                        last_glyph_idx = end_glyph_idx;
                    }
                }

                glyphstrings.push(glyphs);

                line_extents.paint_width += extents.paint_width;
                if line_extents.paint_height < extents.paint_height {
                    line_extents.paint_height = extents.paint_height;
                }

                last = end;

                if end == line_boundary {
                    break;
                } else {
                    debug_assert!(end < line_boundary);
                }
            }

            debug_assert_eq!(last, line_boundary);

            // println!("line boundary {line_slice:?} {current_extents:?}");
            // TODO: Where to get spacing?
            // let spacing = font.horizontal_extents().line_gap as i32 + 10;
            let spacing = 10;
            if !lines.is_empty() {
                total_extents.paint_height += spacing;
            }

            total_extents.paint_height += line_extents.paint_height;
            total_extents.paint_width =
                std::cmp::max(total_extents.paint_width, line_extents.paint_width);

            lines.push(ShapedLine {
                segments,
                extents: line_extents,
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
    face: text::Face,
    // always should be 72 for ass?
    dpi: u32,
    subs: &'a Subtitles,
}

mod text;

impl<'a> Renderer<'a> {
    pub fn new(width: u32, height: u32, subs: &'a Subtitles, dpi: u32) -> Self {
        Self {
            width,
            height,
            text: TextRenderer::new(),
            face: text::Face::load_from_file(
                /* "/nix/store/7y7fyf2jdkl0ny7smybvcwj48nncdws2-home-manager-path/share/fonts/noto/NotoSans[wdth,wght].ttf" */
                "./NotoSansMono[wdth,wght].ttf",
                // "./NotoSansCJK-VF.otf.ttc",
            ),
            // face: text::Face::load_from_file("/nix/store/7y7fyf2jdkl0ny7smybvcwj48nncdws2-home-manager-path/share/fonts/truetype/JetBrainsMono-Regular.ttf"),
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

            let current_extents = glyphs.compute_extents_for_slice(font, slice.clone());
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
            Horizontal::Center => -extents.paint_width / 2,
            Horizontal::Right => -extents.paint_width + font.horizontal_extents().descender / 64,
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
                for segment in event.segments.iter() {
                    println!(
                        "SHAPING V2 INPUT SEGMENT: {:?} font {} {}",
                        segment.text, segment.font_size, segment.font_weight
                    );
                    shaper.add(
                        &segment.text,
                        &self.face.with_size_and_weight(
                            segment.font_size,
                            self.dpi,
                            segment.font_weight as f32,
                        ),
                    )
                }

                let (glyphstrings, lines, extents) = shaper.shape(TextWrappingMode::None);
                println!("SHAPING V2 RESULT: {:?} {:#?}", extents, lines);

                // TODO: Don't do it like this, instead make segments use &str to some other String
                let mut combined = String::new();
                let mut segment_boundaries = vec![];
                for segment in event.segments.iter() {
                    segment_boundaries.push(combined.len());
                    combined.push_str(&segment.text);
                }

                // TODO: Support mixed font_size and font_weight by treating them as separate chunks, this will require more complicated multiline shaping!
                let font = self.face.with_size_and_weight(
                    event.segments.first().unwrap().font_size,
                    self.dpi,
                    event.segments.first().unwrap().font_weight as f32,
                );

                let (glyphs, line_boundaries, line_extents, extents) =
                    self.shape_text_multiline(&font, event.text_wrap, &combined);

                let (ox, oy) =
                    Self::translate_for_aligned_text(&font, true, &extents, event.alignment);

                let x = x.saturating_add_signed(ox);
                let mut y = y.saturating_add_signed(oy);

                let mut last = 0;
                for (end, ((lox, loy), line_extents)) in
                    line_boundaries.into_iter().zip(line_extents.into_iter())
                {
                    let mut x = x + lox;
                    let mut y = y + loy;
                    let mut i = match segment_boundaries.binary_search(&last) {
                        Ok(current) => current,
                        Err(next) => next - 1,
                    };

                    let mut segment_start = segment_boundaries[i];

                    println!("starting at segment {i} char index {last}");

                    while last < end {
                        let segment = &event.segments[i];
                        let next = (segment_start + segment.text.len()
                            - segment.text.chars().filter(|x| *x == '\n').count())
                        .min(end);
                        let glyphs_it = glyphs.iter_slice(last, next);

                        println!(
                            "drawing {:?}[{:?}] line at {},{} with segment {}",
                            combined,
                            (last)..next,
                            x,
                            y,
                            i
                        );

                        let (nx, ny) = self.paint_text(x, y, &font, glyphs_it, segment.color);
                        x = nx;
                        y = ny;

                        last = next;
                        segment_start = next;
                        i += 1;
                    }
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
