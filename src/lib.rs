use std::ops::Range;

use text::{Glyphs, TextExtents};

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

impl Alignment {
    pub fn into_parts(self) -> (HorizontalAlignment, VerticalAlignment) {
        match self {
            Alignment::TopLeft => (HorizontalAlignment::Left, VerticalAlignment::Top),
            Alignment::Top => (HorizontalAlignment::Center, VerticalAlignment::Top),
            Alignment::TopRight => (HorizontalAlignment::Right, VerticalAlignment::Top),
            Alignment::Left => (
                HorizontalAlignment::Left,
                VerticalAlignment::BaselineCentered,
            ),
            Alignment::Center => (
                HorizontalAlignment::Center,
                VerticalAlignment::BaselineCentered,
            ),
            Alignment::Right => (
                HorizontalAlignment::Right,
                VerticalAlignment::BaselineCentered,
            ),
            Alignment::BottomLeft => (HorizontalAlignment::Left, VerticalAlignment::Bottom),
            Alignment::Bottom => (HorizontalAlignment::Center, VerticalAlignment::Bottom),
            Alignment::BottomRight => (HorizontalAlignment::Right, VerticalAlignment::Bottom),
        }
    }
}

enum VerticalAlignment {
    Top,
    BaselineCentered,
    Bottom,
}

enum HorizontalAlignment {
    Left,
    Center,
    Right,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

    fn shape(
        &self,
        line_alignment: HorizontalAlignment,
        wrapping: TextWrappingMode,
    ) -> (Vec<Glyphs>, Vec<ShapedLine>, Rect) {
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
        let mut total_rect = Rect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
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
            // if let Some(last) = lines.last() {
            //     let x = last
            //         .segments
            //         .iter()
            //         .map(|x| &self.font_boundaries[x.corresponding_font_boundary].0)
            //         .map(|f| f.metrics().height)
            //         .collect::<Vec<_>>();
            //     println!("spacing {x:?}");
            // }
            // if !lines.is_empty() {
            //     total_extents.paint_height += 10 * 64;
            // }

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

            let aligning_x_offset = match line_alignment {
                HorizontalAlignment::Left => 0,
                HorizontalAlignment::Center => -line_extents.paint_width / 128,
                HorizontalAlignment::Right => -line_extents.paint_width / 64,
            };

            for segment in segments.iter_mut() {
                segment.baseline_offset.0 += aligning_x_offset;
                segment.paint_rect.x += aligning_x_offset;
                segment.baseline_offset.1 += (max_bearing_y / 64) as i32;
                segment.paint_rect.y += ((max_bearing_y - segment.max_bearing_y) / 64) as i32;
            }

            if segments.len() > 0 {
                total_rect.x = total_rect
                    .x
                    .min(segments.first().map(|x| x.paint_rect.x).unwrap());
                total_rect.w = total_rect.w.max(
                    (segments
                        .last()
                        .map(|x| (x.paint_rect.x + x.paint_rect.w as i32))
                        .unwrap()
                        - segments.first().map(|x| x.paint_rect.x).unwrap())
                        as u32,
                );
            }

            if line_boundary == self.text.len() {
                total_extents.paint_height += line_extents.paint_height;
            } else {
                total_extents.paint_height += segments
                    .iter()
                    .map(|x| &self.font_boundaries[x.corresponding_font_boundary].0)
                    .map(|f| f.metrics().height)
                    .max()
                    .unwrap_or(0) as i32;
            }

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

        total_rect.h = (total_extents.paint_height / 64) as u32;

        (glyphstrings, lines, total_rect)
    }
}

pub struct Renderer<'a> {
    width: u32,
    height: u32,
    buffer: Vec<u8>,
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

    fn horizontal_line(&mut self, x: i32, y: i32, w: u32, color: u32) {
        let rgba = [
            ((color & 0xFF000000) >> 24) as u8,
            ((color & 0x00FF0000) >> 16) as u8,
            ((color & 0x0000FF00) >> 8) as u8,
            (color & 0x000000FF) as u8,
        ];

        let x = if x < 0 { 0u32 } else { x as u32 };
        let y = if y < 0 { return } else { y as u32 };
        if y >= self.height {
            return;
        }

        for x in x..(x + w).min(self.width) {
            *self.pixel(x, y) = rgba;
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: u32) {
        let rgba = [
            ((color & 0xFF000000) >> 24) as u8,
            ((color & 0x00FF0000) >> 16) as u8,
            ((color & 0x0000FF00) >> 8) as u8,
            (color & 0x000000FF) as u8,
        ];

        if x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let rx = x + w as i32;
        let by = y + h as i32;

        for y in (y.max(0) as u32)..by.clamp(0, self.height as i32) as u32 {
            if x >= 0 {
                *self.pixel(x as u32, y) = rgba;
            }
            if rx >= 0 && rx < self.width as i32 {
                *self.pixel(rx as u32, y) = rgba;
            }
        }
        for x in (x.max(0) as u32)..rx.clamp(0, self.width as i32) as u32 {
            if y >= 0 {
                *self.pixel(x, y as u32) = rgba;
            }
            if by >= 0 && by < self.height as i32 {
                *self.pixel(x, by as u32) = rgba;
            }
        }
    }

    fn paint_text<'g>(
        &mut self,
        x: i32,
        y: i32,
        font: &text::Font,
        text: impl IntoIterator<Item = text::Glyph<'g>>,
        color: u32,
    ) -> (i32, i32) {
        text::paint(
            &mut self.buffer,
            x,
            y,
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
        x: i32,
        y: i32,
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
        self.paint_text(x + ox, y + oy, &font, shape.iter(), color);
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextExtents,
        alignment: Alignment,
    ) -> (i32, i32) {
        assert!(horizontal);

        let (horizontal, vertical) = alignment.into_parts();

        // TODO: Numbers chosen arbitrarily
        let ox = match horizontal {
            HorizontalAlignment::Left => -font.horizontal_extents().descender / 64 / 2,
            HorizontalAlignment::Center => -extents.paint_width / 128,
            HorizontalAlignment::Right => {
                (-extents.paint_width + font.horizontal_extents().descender) / 64
            }
        };

        let oy = match vertical {
            VerticalAlignment::Top => font.horizontal_extents().ascender / 64,
            VerticalAlignment::BaselineCentered => 0,
            VerticalAlignment::Bottom => font.horizontal_extents().descender / 64,
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
            self.width as i32,
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

                let (horizontal_alignment, vertical_alignment) = event.alignment.into_parts();
                let (glyphstrings, lines, total_rect) =
                    shaper.shape(horizontal_alignment, TextWrappingMode::None);
                println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => -(total_rect.h as i32) / 2,
                        VerticalAlignment::Bottom => -(total_rect.h as i32),
                    };

                self.rect(
                    x + total_rect.x - 1,
                    y + total_rect.y - 1,
                    total_rect.w + 1,
                    total_rect.h + 1,
                    0xFF00FFFF,
                );

                let total_position_debug_pos = match vertical_alignment {
                    VerticalAlignment::Top => (total_rect.h as i32 + 20, Alignment::Top),
                    VerticalAlignment::BaselineCentered => {
                        (total_rect.h as i32 + 20, Alignment::Top)
                    }
                    VerticalAlignment::Bottom => (-32, Alignment::Bottom),
                };

                self.debug_text(
                    (x + total_rect.x + total_rect.w as i32 / 2) as i32,
                    (y + total_rect.y + total_position_debug_pos.0) as i32,
                    &format!(
                        "x:{} y:{} w:{} h:{}",
                        x + total_rect.x,
                        y + total_rect.y,
                        total_rect.w,
                        total_rect.h
                    ),
                    total_position_debug_pos.1,
                    16.0,
                    0xFF00FFFF,
                );

                let mut last = 0;
                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];
                    let glyphs_it = glyphstrings[shaped_segment.glyphs.0]
                        .iter_slice(shaped_segment.glyphs.1.start, shaped_segment.glyphs.1.end);

                    let paint_box = (
                        x + shaped_segment.paint_rect.x,
                        y + shaped_segment.paint_rect.y,
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1,
                        &format!(
                            "{},{}",
                            x + shaped_segment.paint_rect.x,
                            y + shaped_segment.paint_rect.y
                        ),
                        Alignment::BottomLeft,
                        16.0,
                        0xFF0000FF,
                    );

                    self.debug_text(
                        paint_box.0,
                        paint_box.1 + shaped_segment.paint_rect.h as i32,
                        &format!(
                            "{},{}",
                            x + shaped_segment.baseline_offset.0,
                            y + shaped_segment.baseline_offset.1
                        ),
                        Alignment::TopLeft,
                        16.0,
                        0xFF0000FF,
                    );

                    self.debug_text(
                        paint_box.0 + shaped_segment.paint_rect.w as i32,
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
                        paint_box.0 as i32,
                        y + shaped_segment.baseline_offset.1,
                        shaped_segment.paint_rect.w as u32,
                        0x00FF00FF,
                    );

                    let x = x + shaped_segment.baseline_offset.0;
                    let y = y + shaped_segment.baseline_offset.1;

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
