// The library is still under active development
#![allow(dead_code)]

use outline::{Outline, OutlineBuilder, SplineDegree};
use painter::Painter;
use text::TextExtents;
use util::{BoundingBox, Point2, RcArray};

pub mod ass;
mod outline;
mod painter;
mod rasterize;
pub mod srv3;
mod text;
mod util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alignment {
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
enum Segment {
    Text(TextSegment),
    Shape(ShapeSegment),
}

#[derive(Debug, Clone)]
struct TextSegment {
    font: String,
    font_size: f32,
    font_weight: u32,
    italic: bool,
    underline: bool,
    strike_out: bool,
    color: u32,
    text: String,
}

// Shape segment behaviour:
// Treated as constant-sized block during text layout
// Size does not take into account negative coordinates
#[derive(Debug, Clone)]
pub struct ShapeSegment {
    outlines: Vec<outline::Outline>,
    size: Rect,
    color: u32,
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
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "this ".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0x0000FFFF,
                            text: "is\n".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 64.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "mu".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "monospace".to_string(),
                            font_size: 48.0,
                            font_weight: 700,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "ltil".to_string(),
                        }),
                        Segment::Text(TextSegment {
                            font: "Arial".to_string(),
                            font_size: 80.0,
                            font_weight: 400,
                            italic: false,
                            underline: false,
                            strike_out: false,
                            color: 0xFF0000FF,
                            text: "ine".to_string(),
                        }),
                        Segment::Shape(ShapeSegment {
                            outlines: vec![{
                                // let mut outline = Outline::new(Point2::new(0.0, 0.0));
                                // outline.push_line(Point2::new(50.0 / 4.0, 200.0 / 4.0));
                                // outline.push_line(Point2::new(300.0 / 4.0, 150.0 / 4.0));
                                // outline.push_line(Point2::new(400.0 / 4.0, 400.0 / 4.0));
                                // outline
                                Outline::new()
                            }],
                            size: {
                                let mut bbox = BoundingBox::new();
                                bbox.add(&Point2::new(0.0, 0.0));
                                bbox.add(&Point2::new(400.0 / 4.0, 400.0 / 4.0));
                                Rect::from_bounding_box(&bbox)
                            },
                            color: 0x00FF00FF,
                        }),
                    ],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.1,
                    alignment: Alignment::Top,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: 0x00FF00AA,
                        text: "this is for comparison".to_string(),
                    })],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.8,
                    alignment: Alignment::Bottom,
                    text_wrap: TextWrappingMode::None,
                    segments: vec![Segment::Text(TextSegment {
                        font: "monospace".to_string(),
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        color: 0xFFFFFFFF,
                        text: "this is bold..".to_string(),
                    })],
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

enum ShaperSegment {
    Text(text::Font),
    Shape(Rect),
}

struct MultilineTextShaper {
    text: String,
    explicit_line_bounaries: Vec</* end of line i */ usize>,
    segment_boundaries: Vec<(ShaperSegment, /* end of segment i */ usize)>,
    intra_font_segment_splits: Vec<usize>,
}

#[derive(Debug)]
struct ShapedLineSegment {
    glyphs: Option<RcArray<text::Glyph>>,
    baseline_offset: (i32, i32),
    paint_rect: Rect,
    corresponding_input_segment: usize,
    // Implementation details
    max_bearing_y: i64,
    corresponding_font_boundary: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
}

impl Rect {
    fn from_bounding_box(bb: &BoundingBox) -> Rect {
        bb.minmax()
            .map(|(min, max)| Rect {
                x: min.x.floor() as i32,
                y: min.y.floor() as i32,
                w: (max.x - min.x).ceil() as u32,
                h: (max.y - min.y).ceil() as u32,
            })
            .unwrap_or_default()
    }
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
            segment_boundaries: Vec::new(),
            intra_font_segment_splits: Vec::new(),
        }
    }

    fn add_text(&mut self, mut text: &str, font: &text::Font) {
        while let Some(nl) = text.find('\n') {
            self.text.push_str(&text[..nl]);
            self.explicit_line_bounaries.push(self.text.len());
            text = &text[nl + 1..];
        }
        self.text.push_str(text);

        if let Some((ShaperSegment::Text(ref last_font), _)) = self.segment_boundaries.last() {
            assert_eq!(last_font.dpi(), font.dpi());
        }

        if let Some((ShaperSegment::Text(ref last_font), ref mut last_end)) =
            self.segment_boundaries.last_mut()
        {
            if last_font == font {
                self.intra_font_segment_splits.push(*last_end);
                *last_end = self.text.len();
                return;
            }
        }

        self.segment_boundaries
            .push((ShaperSegment::Text(font.clone()), self.text.len()));
    }

    fn add_shape(&mut self, dim: Rect) {
        self.text.push('\0');
        self.segment_boundaries
            .push((ShaperSegment::Shape(dim), self.text.len()))
    }

    fn shape(
        &self,
        line_alignment: HorizontalAlignment,
        wrapping: TextWrappingMode,
    ) -> (Vec<ShapedLine>, Rect) {
        assert_eq!(wrapping, TextWrappingMode::None);

        println!("SHAPING V2 TEXT {:?}", self.text);
        println!(
            "SHAPING V2 LINE BOUNDARIES {:?}",
            self.explicit_line_bounaries
        );

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
                .segment_boundaries
                .binary_search_by_key(&last, |(_, b)| *b)
            {
                Ok(idx) => idx + 1,
                Err(idx) => idx,
            };

            println!(
                "last: {last}, font boundaries: {:?}, binary search result: {}",
                self.segment_boundaries
                    .iter()
                    .map(|(_, s)| s)
                    .collect::<Vec<_>>(),
                starting_font_segment
            );

            let mut max_bearing_y = 0;

            for current_segment in starting_font_segment..self.segment_boundaries.len() {
                let (segment, font_boundary) = &self.segment_boundaries[current_segment];

                let end = (*font_boundary).min(line_boundary);
                let segment_slice = last..end;

                let first_internal_split_idx =
                    match self.intra_font_segment_splits.binary_search(&last) {
                        Ok(idx) => idx + 1,
                        Err(idx) => idx,
                    };

                match segment {
                    ShaperSegment::Text(font) => {
                        let glyphs = {
                            let mut buffer = text::ShapingBuffer::new();
                            let direction = buffer.guess_properties();
                            if !direction.is_horizontal() {
                                buffer.set_direction(direction.make_horizontal());
                            }
                            buffer.add(&self.text[segment_slice]);
                            buffer.shape(font)
                        };
                        let (extents, (trailing_x_advance, _)) =
                            text::compute_extents_ex(true, font, &glyphs);

                        let segment_max_bearing_y = glyphs
                            .iter()
                            .map(|x| font.glyph_extents(x.codepoint).horiBearingY)
                            .max()
                            .unwrap_or(0);

                        max_bearing_y = std::cmp::max(max_bearing_y, segment_max_bearing_y);

                        let rc_glyphs = RcArray::from_boxed(glyphs);

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
                                glyphs: Some(rc_glyphs),
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
                                corresponding_font_boundary: current_segment,
                                corresponding_input_segment: current_segment
                                    + first_internal_split_idx,
                            });
                        } else {
                            let mut last_glyph_idx = 0;
                            let mut x = 0;
                            for (i, split_end) in self.intra_font_segment_splits
                                [first_internal_split_idx..]
                                .iter()
                                .copied()
                                .take_while(|idx| *idx < end)
                                .chain(std::iter::once(end))
                                .enumerate()
                            {
                                let end_glyph_idx = split_end - last;
                                let glyph_range = last_glyph_idx..end_glyph_idx;
                                let glyph_slice = RcArray::slice(rc_glyphs.clone(), glyph_range);
                                let (extents, (x_advance, _)) =
                                    text::compute_extents_ex(true, font, &glyph_slice);
                                segments.push(ShapedLineSegment {
                                    glyphs: Some(glyph_slice),
                                    baseline_offset: (x / 64, total_extents.paint_height / 64),
                                    paint_rect: Rect {
                                        x: x / 64,
                                        y: total_extents.paint_height / 64,
                                        w: extents.paint_width as u32 / 64,
                                        h: extents.paint_height as u32 / 64,
                                    },
                                    max_bearing_y: segment_max_bearing_y,
                                    corresponding_font_boundary: current_segment,
                                    corresponding_input_segment: current_segment
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
                    }
                    ShaperSegment::Shape(dim) => {
                        let logical_w = dim.w - (-dim.x).max(0) as u32;
                        let logical_h = dim.h - (-dim.y).max(0) as u32;
                        let segment_max_bearing_y = (logical_h * 64) as i64;
                        segments.push(ShapedLineSegment {
                            glyphs: None,
                            baseline_offset: (
                                line_extents.paint_width / 64,
                                total_extents.paint_height / 64,
                            ),
                            paint_rect: Rect {
                                x: line_extents.paint_width / 64,
                                y: total_extents.paint_height / 64,
                                w: logical_w,
                                h: logical_h,
                            },
                            corresponding_input_segment: current_segment + first_internal_split_idx,
                            corresponding_font_boundary: current_segment + first_internal_split_idx,
                            max_bearing_y: segment_max_bearing_y,
                        });
                        line_extents.paint_width += (logical_w * 64) as i32;
                        max_bearing_y = max_bearing_y.max(segment_max_bearing_y);
                        line_extents.paint_height =
                            line_extents.paint_height.max((logical_h * 64) as i32);
                    }
                }

                last = end;

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
                if segment.glyphs.is_none() {
                    segment.baseline_offset.1 +=
                        ((max_bearing_y - segment.max_bearing_y) / 64) as i32;
                } else {
                    segment.baseline_offset.1 += (max_bearing_y / 64) as i32;
                }
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
                total_extents.paint_height += dbg!(segments
                    .iter()
                    .map(
                        |x| match &self.segment_boundaries[x.corresponding_font_boundary].0 {
                            ShaperSegment::Text(f) =>
                                x.paint_rect.y as i64 * 64 + f.metrics().height,
                            ShaperSegment::Shape(_) => (x.paint_rect.h * 64) as i64,
                        }
                    )
                    .max()
                    .unwrap_or(0) as i32);
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

        (lines, total_rect)
    }
}

pub struct Renderer<'a> {
    painter: Painter<Vec<u8>>,
    fonts: text::FontManager,
    // always should be 72 for ass?
    dpi: u32,
    subs: &'a Subtitles,
}

impl<'a> Renderer<'a> {
    pub fn new(width: u32, height: u32, subs: &'a Subtitles, dpi: u32) -> Self {
        Self {
            painter: Painter::new_vec(width, height),

            fonts: text::FontManager::new(text::font_backend::platform_default().unwrap()),
            dpi,
            subs,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.painter.resize(width, height);
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
        let shaped = text::shape_text(&font, text);
        let (ox, oy) = Self::translate_for_aligned_text(
            &font,
            true,
            &text::compute_extents(true, &font, &shaped.glyphs),
            alignment,
        );
        self.painter
            .paint_text(x + ox, y + oy, &font, &shaped.glyphs, color);
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

    fn draw_outline(&mut self, x: i32, y: i32, outline: &outline::Outline, color: u32) {
        for point in outline.points() {
            let (x, y) = (point.x as i32 + x, point.y as i32 + y);
            if !self.painter.in_bounds(x, y) {
                continue;
            }
            self.debug_text(
                x,
                y,
                &format!("{x},{y}"),
                Alignment::TopLeft,
                16.0,
                0xFFFFFFFF,
            );
            self.painter.dot(x, y, 0xFFFFFFFF);
        }

        self.painter.stroke_outline(x, y, outline, color);
    }

    pub fn render(&mut self, t: u32) {
        if self.painter.height() == 0 || self.painter.height() == 0 {
            return;
        }

        self.painter.clear(0x00000000);

        self.debug_text(
            self.painter.width() as i32,
            0,
            &format!(
                "{}x{} dpi:{}",
                self.painter.width(),
                self.painter.height(),
                self.dpi
            ),
            Alignment::TopRight,
            16.0,
            0xFFFFFFFF,
        );

        let shape_scale = self.dpi as f32 / 72.0;

        {
            for event in self
                .subs
                .events
                .iter()
                .filter(|ev| ev.start <= t && ev.end > t)
            {
                println!("{event:?}");
                let x = (self.painter.width() as f32 * event.x) as u32;
                let y = (self.painter.height() as f32 * event.y) as u32;

                let mut shaper = MultilineTextShaper::new();
                let mut segment_fonts = vec![];
                for segment in event.segments.iter() {
                    match segment {
                        Segment::Text(segment) => {
                            let font = self
                                .fonts
                                .get_or_load(
                                    &segment.font,
                                    segment.font_weight as f32,
                                    segment.italic,
                                )
                                .unwrap()
                                .with_size(segment.font_size, self.dpi);

                            println!("SHAPING V2 INPUT TEXT: {:?} {:?}", segment.text, font);

                            shaper.add_text(&segment.text, &font);
                            segment_fonts.push(Some(font));
                        }
                        Segment::Shape(shape) => {
                            println!(
                                "SHAPING V2 INPUT SHAPE: {:?} {:?}",
                                shape.outlines, shape.size
                            );

                            shaper.add_shape(Rect {
                                x: (shape.size.x as f32 * shape_scale).floor() as i32,
                                y: (shape.size.y as f32 * shape_scale).floor() as i32,
                                w: (shape.size.w as f32 * shape_scale).ceil() as u32,
                                h: (shape.size.h as f32 * shape_scale).ceil() as u32,
                            });
                            segment_fonts.push(None);
                        }
                    }
                }

                let (horizontal_alignment, vertical_alignment) = event.alignment.into_parts();
                let (lines, total_rect) =
                    shaper.shape(horizontal_alignment, TextWrappingMode::None);
                println!("SHAPING V2 RESULT: {:?} {:#?}", total_rect, lines);

                let x = x as i32;
                let y = y as i32
                    + match vertical_alignment {
                        VerticalAlignment::Top => 0,
                        VerticalAlignment::BaselineCentered => -(total_rect.h as i32) / 2,
                        VerticalAlignment::Bottom => -(total_rect.h as i32),
                    };

                self.painter.stroke_whrect(
                    x + total_rect.x - 1,
                    y + total_rect.y - 1,
                    total_rect.w + 2,
                    total_rect.h + 2,
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

                for shaped_segment in lines.iter().flat_map(|line| &line.segments) {
                    let segment = &event.segments[shaped_segment.corresponding_input_segment];

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
                        &if let Segment::Text(segment) = segment {
                            format!("{:.0}pt", segment.font_size)
                        } else {
                            format!("shape")
                        },
                        Alignment::BottomRight,
                        16.0,
                        0xFFFFFFFF,
                    );

                    self.painter.stroke_whrect(
                        paint_box.0,
                        paint_box.1,
                        shaped_segment.paint_rect.w as u32,
                        shaped_segment.paint_rect.h as u32,
                        0x0000FFFF,
                    );

                    self.painter.horizontal_line(
                        paint_box.0 as i32,
                        y + shaped_segment.baseline_offset.1,
                        shaped_segment.paint_rect.w as u32,
                        0x00FF00FF,
                    );

                    let x = x + shaped_segment.baseline_offset.0;
                    let y = y + shaped_segment.baseline_offset.1;

                    match segment {
                        Segment::Text(t) => {
                            self.painter.paint_text(
                                x,
                                y,
                                segment_fonts[shaped_segment.corresponding_input_segment]
                                    .as_ref()
                                    .unwrap(),
                                shaped_segment.glyphs.as_deref().unwrap(),
                                t.color,
                            );
                        }
                        Segment::Shape(s) => {
                            for c in s.outlines.iter() {
                                let mut c = c.clone();
                                let shape_scale = shape_scale * 5.;
                                c = {
                                    let mut c = OutlineBuilder::new();
                                    c.add_point(Point2::ZERO);
                                    c.add_segment(SplineDegree::Linear);
                                    c.add_point(Point2::new(0.0, 100.0));
                                    c.add_segment(SplineDegree::Linear);
                                    c.add_point(Point2::new(100.0, 100.0));
                                    // c.add_segment(SplineDegree::Linear);
                                    // c.add_point(Point2::new(100.0, 0.0));
                                    c.add_segment(SplineDegree::Linear);
                                    c.close_contour();
                                    c.build()
                                };
                                let x = 150;
                                let y = 150;
                                println!("{c:?}");
                                c.scale(shape_scale);
                                let outer = outline::stroke(
                                    &c,
                                    10.0 * shape_scale,
                                    10.0 * shape_scale,
                                    0.01,
                                );
                                self.painter.stroke_outline(x, y, &c, 0xFFFFFFFF);

                                dbg!(&c);
                                // dbg!(&outer);

                                self.painter.stroke_outline(x, y, &outer.0, 0xFF0000FF);
                                self.painter.stroke_outline(x, y, &outer.1, 0x0000FFFF);
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn bitmap(&self) -> &[u8] {
        &self.painter.buffer()
    }
}
