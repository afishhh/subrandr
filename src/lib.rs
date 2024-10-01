use text::{ShapedText, TextExtents, TextRenderer};
use text_sys::*;

pub mod ass;

impl From<ass::Alignment> for Alignment {
    fn from(value: ass::Alignment) -> Self {
        match value {
            ass::Alignment::BottomLeft => Alignment::BottomLeft,
            ass::Alignment::BottomCenter => Alignment::Bottom,
            ass::Alignment::BottomRight => Alignment::BottomRight,
            ass::Alignment::MiddleLeft => Alignment::Left,
            ass::Alignment::MiddleCenter => Alignment::Center,
            ass::Alignment::MiddleRight => Alignment::Right,
            ass::Alignment::TopLeft => Alignment::TopLeft,
            ass::Alignment::TopCenter => Alignment::Top,
            ass::Alignment::TopRight => Alignment::TopRight,
        }
    }
}

pub fn ass_to_subs(ass: ass::Script) -> Subtitles {
    let mut subs = Subtitles { events: vec![] };

    let layout_resolution = if ass.layout_resolution.0 > 0 && ass.layout_resolution.1 > 0 {
        ass.layout_resolution
    } else {
        ass.play_resolution
    };

    for event in ass.events {
        let style = ass
            .styles
            .binary_search_by(|x| x.name.cmp(&event.style))
            .map(|idx| &ass.styles[idx])
            .unwrap_or(&ass::DEFAULT_STYLE);

        let mut text = String::new();
        // TODO: correct and alignment specific values
        let mut x = 0.5;
        let mut y = 0.8;
        let mut alignment = style.alignment;

        for part in ass::segment_event_text(&event.text) {
            match part {
                ass::TextPart::Commands(r) => {
                    let command_block = &event.text[r];
                    let mut it = command_block.chars();

                    while !it.as_str().is_empty() {
                        while it.next().is_some_and(|c| c != '\\') {}

                        let remainder = it.as_str();
                        if remainder.len() >= 3 && &remainder[..3] == "pos" {
                            assert_eq!(&remainder[3..4], "(");
                            let args_end = remainder.find(')').unwrap();
                            let args = &remainder[4..args_end];
                            let (left, right) = args.split_once(',').unwrap();
                            let tx = left.parse::<u32>().unwrap();
                            let ty = right.parse::<u32>().unwrap();
                            let (max_x, max_y) = layout_resolution;
                            x = tx as f32 / max_x as f32;
                            y = ty as f32 / max_y as f32
                        };
                        if remainder.len() >= 2 && &remainder[..2] == "an" {
                            alignment = ass::Alignment::from_ass(&remainder[2..3]).unwrap();
                        }
                        println!("{x} {y}");
                    }
                }
                ass::TextPart::Content(c) => {
                    text += &event.text[c];
                }
            }
        }

        subs.events.push(Event {
            start: event.start,
            end: event.end,
            x,
            y,
            alignment: alignment.into(),
            segments: vec![Segment {
                font: style.fontname.to_string(),
                font_size: style.fontsize,
                font_weight: style.weight,
                italic: style.italic,
                underline: style.underline,
                strike_out: style.strike_out,
                text,
            }],
        })
    }

    subs
}

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

#[derive(Debug, Clone)]
pub struct Event {
    start: u32,
    end: u32,
    x: f32,
    y: f32,
    alignment: Alignment,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    font: String,
    font_size: f32,
    font_weight: u32,
    italic: bool,
    underline: bool,
    strike_out: bool,
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

    pub fn test_new() -> Subtitles {
        Subtitles {
            events: vec![
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.2,
                    alignment: Alignment::Top,
                    segments: vec![Segment {
                        font: "ignored".to_string(),
                        font_size: 64.0,
                        font_weight: 400,
                        italic: false,
                        underline: false,
                        strike_out: false,
                        text: "this is normal".to_string(),
                    }],
                },
                Event {
                    start: 0,
                    end: 600000,
                    x: 0.5,
                    y: 0.8,
                    alignment: Alignment::Bottom,
                    segments: vec![Segment {
                        font: "ignored".to_string(),
                        font_size: 64.0,
                        font_weight: 700,
                        italic: false,
                        underline: false,
                        strike_out: false,
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

    fn paint_text(&mut self, x: u32, y: u32, font: &text::Font, text: &ShapedText, alpha: f32) {
        self.text.paint(
            &mut self.buffer,
            x as usize,
            y as usize,
            self.width as usize,
            self.height as usize,
            (self.width * 4) as usize,
            font,
            text,
            [255, 255, 255],
            alpha,
        );
    }

    fn translate_for_aligned_text(
        font: &text::Font,
        horizontal: bool,
        extents: &TextExtents,
        alignment: Alignment,
    ) -> (i32, i32) {
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
                let mut x = (self.width as f32 * event.x) as u32;
                let mut y = (self.height as f32 * event.y) as u32;
                for segment in event.segments.iter() {
                    let font = self.face.with_size_and_weight(
                        segment.font_size,
                        self.dpi,
                        segment.font_weight as f32,
                    );
                    let shaped = self.text.shape_text(&font, &segment.text);
                    let (ox, oy) = Self::translate_for_aligned_text(
                        &font,
                        true,
                        &self.text.compute_extents(&font, &shaped),
                        event.alignment,
                    );
                    let x = x.saturating_add_signed(ox);
                    let y = y.saturating_add_signed(oy);
                    self.paint_text(x, y, &font, &shaped, 1.0);
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
