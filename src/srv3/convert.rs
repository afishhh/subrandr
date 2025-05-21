use std::{collections::HashMap, ops::Range, rc::Rc};

use crate::math::I26Dot6;
/// Converts parsed SRV3 subtitles into Subtitles.
///
/// Was initially based on YTSubConverter, now also reverse engineered from YouTube's captions.js.
use crate::{
    color::BGRA8,
    math::I16Dot16,
    miniweb::{
        self,
        layout::{
            self, BlockContainer, Container, FixedL, InlineChild, InlineContainer, InlineText,
            LayoutConstraints, Point2L, Vec2L,
        },
        realm::Realm,
        style::{
            self,
            types::{
                Alignment, Display, FontSlant, HorizontalAlignment, Ruby, TextShadow,
                VerticalAlignment,
            },
            ComputedStyle, StyleMap,
        },
    },
    renderer::FrameLayoutPass,
    Subrandr, SubtitleContext,
};

use super::{Document, EdgeType, Pen};

const SRV3_FONTS: &[&[&str]] = &[
    &[
        "Courier New",
        "Courier",
        "Nimbus Mono L",
        "Cutive Mono",
        "monospace",
    ],
    &[
        "Times New Roman",
        "Times",
        "Georgia",
        "Cambria",
        "PT Serif Caption",
        "serif",
    ],
    &[
        "Deja Vu Sans Mono", // not a real font :(
        "Lucida Console",
        "Monaco",
        "Consolas",
        "PT Mono",
        "monospace",
    ],
    &[
        "YouTube Noto",
        "Roboto",
        "Arial",
        "Helvetica",
        "Verdana",
        "PT Sans Caption",
        "sans-serif",
    ],
    &["Comic Sans Ms", "Impact", "Handlee", "fantasy"],
    &[
        "Monotype Corsiva",
        "URW Chancery L",
        "Apple Chancery",
        "Dancing Script",
        "cursive",
    ],
    // YouTube appears to conditionally set this to either:
    // "Carrois Gothic SC", sans-serif-smallcaps
    // or sometimes:
    // Arial, Helvetica, Verdana, "Marcellus SC", sans-serif
    // the first one seems to be used when ran under Cobalt
    // https://developers.google.com/youtube/cobalt
    // i.e. in YouTube TV
    &[
        "Arial",
        "Helvetica",
        "Verdana",
        "Marcellus SC",
        "sans-serif",
    ],
];

fn font_style_to_name(style: u32) -> &'static [&'static str] {
    style
        .checked_sub(1)
        .and_then(|i| SRV3_FONTS.get(i as usize))
        .map_or(SRV3_FONTS[3], |v| v)
}

fn convert_coordinate(coord: f32) -> f32 {
    0.02 + coord * 0.0096
}

fn calculate_font_scale(
    mut video_width: f32,
    video_height: f32,
    player_width: f32,
    player_height: f32,
) -> f32 {
    let mut h = video_height / 360.0 * 16.0;
    if video_height >= video_width {
        video_width = 640.0;
        if player_height > player_width * 1.3 {
            video_width = 480.0;
        }
        h = player_width / video_width * 16.0;
    }
    h
}

fn font_scale_from_ctx(ctx: &SubtitleContext) -> f32 {
    calculate_font_scale(
        ctx.pixels_to_css(ctx.video_width).into_f32(),
        ctx.pixels_to_css(ctx.video_height).into_f32(),
        ctx.pixels_to_css(ctx.player_width()).into_f32(),
        ctx.pixels_to_css(ctx.player_height()).into_f32(),
    )
}

#[allow(clippy::let_and_return)] // shut up
fn font_size_to_pixels(size: u16) -> f32 {
    let c = 1.0 + 0.25 * (size as f32 / 100.0 - 1.0);
    // This appears to be further modified based on an "of" attribute
    // currently we don't even parse it but if start doing so this is the
    // correct transformation:
    // if offset == 0 || offset == 2 {
    //     c *= 0.8;
    // }
    c
}

trait SubtitleContextCssExt {
    // 1px = 1/96in
    fn pixels_to_css(&self, physical_pixels: FixedL) -> FixedL;
    fn pixels_from_css(&self, css_pixels: FixedL) -> FixedL;
}

impl SubtitleContextCssExt for SubtitleContext {
    fn pixels_to_css(&self, physical_pixels: FixedL) -> FixedL {
        physical_pixels / self.pixel_scale()
    }

    fn pixels_from_css(&self, css_pixels: FixedL) -> FixedL {
        css_pixels * self.pixel_scale()
    }
}

fn pixels_to_points(pixels: f32) -> f32 {
    pixels * 96.0 / 72.0
}

#[derive(Debug, Clone)]
pub struct Srv3TextShadow {
    // never None
    kind: EdgeType,
    color: BGRA8,
}

impl Srv3TextShadow {
    pub(crate) fn to_css(&self, ctx: &SubtitleContext, out: &mut Vec<TextShadow>) {
        let a = font_scale_from_ctx(ctx) / 32.0;
        let e = a.max(1.0);
        let l = (2.0 * a).max(1.0);
        let mut t = (3.0 * a).max(1.0);
        let c = (5.0 * a).max(1.0);

        match self.kind {
            EdgeType::None => (),
            EdgeType::HardShadow => {
                // in captions.js it is window.devicePixelRatio >= 2 ? 0.5 : 1
                // BUT that is NOT what we want, I think they do this to increase fidelity on displays
                // with a lower DPI, because browsers scale all their units by window.devicePixelRatio
                // however we're working with direct device pixels here, so we want to do the OPPOSITE
                // of what they do and pick 0.5 when we have less pixels.
                let step = (ctx.dpi >= 144) as i32 as f32 * 0.5 + 0.5;
                let mut x = e;
                while x <= t {
                    out.push(TextShadow {
                        offset: Vec2L::new(
                            ctx.pixels_from_css(x.into()),
                            ctx.pixels_from_css(x.into()),
                        ),
                        blur_radius: I26Dot6::ZERO,
                        color: self.color,
                    });
                    x += step;
                }
            }
            EdgeType::Bevel => {
                let offset =
                    Vec2L::new(ctx.pixels_from_css(e.into()), ctx.pixels_from_css(e.into()));
                out.push(TextShadow {
                    offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
                out.push(TextShadow {
                    offset: -offset,
                    blur_radius: I26Dot6::ZERO,
                    color: self.color,
                });
            }
            EdgeType::Glow => out.extend(std::iter::repeat_n(
                TextShadow {
                    offset: Vec2L::ZERO,
                    blur_radius: ctx.pixels_from_css(l.into()),
                    color: self.color,
                },
                5,
            )),
            EdgeType::SoftShadow => {
                let offset =
                    Vec2L::new(ctx.pixels_from_css(l.into()), ctx.pixels_from_css(l.into()));
                while t <= c {
                    out.push(TextShadow {
                        offset,
                        blur_radius: ctx.pixels_from_css(t.into()),
                        color: self.color,
                    });
                    t += a;
                }
            }
        }
    }
}

impl super::Point {
    pub fn to_alignment(self) -> Alignment {
        match self {
            super::Point::TopLeft => Alignment(HorizontalAlignment::Left, VerticalAlignment::Top),
            super::Point::TopCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Top)
            }
            super::Point::TopRight => Alignment(HorizontalAlignment::Right, VerticalAlignment::Top),
            super::Point::MiddleLeft => {
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Center)
            }
            super::Point::MiddleCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Center)
            }
            super::Point::MiddleRight => {
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Center)
            }
            super::Point::BottomLeft => {
                Alignment(HorizontalAlignment::Left, VerticalAlignment::Bottom)
            }
            super::Point::BottomCenter => {
                Alignment(HorizontalAlignment::Center, VerticalAlignment::Bottom)
            }
            super::Point::BottomRight => {
                Alignment(HorizontalAlignment::Right, VerticalAlignment::Bottom)
            }
        }
    }
}

#[derive(Debug)]
pub struct Subtitles {
    root_style: StyleMap,
    windows: Vec<Window>,
}

#[derive(Debug)]
struct Window {
    x: f32,
    y: f32,
    // TODO: What the heck does this do
    //       How does a timestamp on a window work?
    //       Currently this is just ignored until I figure out what to do with it.
    range: Range<u32>,
    alignment: Alignment,
    events: Vec<WindowEvent>,
}

#[derive(Debug)]
struct WindowEvent {
    range: Range<u32>,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
struct Segment {
    base_style: StyleMap,
    font_size: u16,
    time_offset: u32,
    text: Rc<str>,
    shadow: Srv3TextShadow,
    ruby: Ruby,
}

fn segments_to_inline(
    pass: &mut FrameLayoutPass,
    event_time: u32,
    segments: &[Segment],
) -> Vec<InlineChild> {
    segments
        .iter()
        .filter_map(|segment| {
            pass.add_animation_point(event_time + segment.time_offset);

            if segment.time_offset <= pass.t - event_time {
                Some(InlineChild::Text(InlineText {
                    style: {
                        let mut result = segment.base_style.clone();

                        let mut size =
                            pixels_to_points(font_size_to_pixels(segment.font_size) * 0.75)
                                * font_scale_from_ctx(pass.sctx);
                        if matches!(segment.ruby, Ruby::Over) {
                            size /= 2.0;
                        }

                        result.set::<style::FontSize>(I26Dot6::from(size));

                        let mut shadows = vec![];
                        segment.shadow.to_css(pass.sctx, &mut shadows);

                        if !shadows.is_empty() {
                            result.set::<style::TextShadows>(shadows.into())
                        }

                        let mut r = ComputedStyle::default();
                        r.apply_all(&result);
                        r
                    },
                    text: segment.text.clone(),
                    ruby: segment.ruby,
                }))
            } else {
                None
            }
        })
        .collect()
}

impl Window {
    pub fn layout(
        &self,
        pass: &mut FrameLayoutPass,
        style: &StyleMap,
    ) -> Result<Option<(Point2L, layout::BlockContainerFragment)>, layout::InlineLayoutError> {
        let contents: Vec<Container> = self
            .events
            .iter()
            .filter_map(|line| {
                if pass.add_event_range(line.range.clone()) {
                    Some(Container::Inline(InlineContainer {
                        contents: segments_to_inline(pass, line.range.start, &line.segments),
                        ..InlineContainer::default()
                    }))
                } else {
                    None
                }
            })
            .collect();

        if contents.is_empty() {
            return Ok(None);
        }

        let block = BlockContainer {
            style: {
                let mut result = StyleMap::new();

                if self.alignment.0 != HorizontalAlignment::Left {
                    result.set::<style::TextAlign>(self.alignment.0);
                }

                let mut r = ComputedStyle::default();
                r.apply_all(style);
                let mut r = r.create_child();
                r.apply_all(&result);
                r
            },
            contents,
        };

        let constraints = LayoutConstraints {
            size: Vec2L::new(pass.sctx.player_width() * 96 / 100, FixedL::MAX),
        };

        let fragment = layout::layout(pass.lctx, constraints, &block)?;

        let mut pos = Point2L::new(
            (self.x * pass.sctx.player_width().into_f32()).into(),
            (self.y * pass.sctx.player_height().into_f32()).into(),
        );

        match self.alignment.0 {
            HorizontalAlignment::Left => (),
            HorizontalAlignment::Center => pos.x -= fragment.fbox.size.x / 2,
            HorizontalAlignment::Right => pos.x -= fragment.fbox.size.x,
        }

        match self.alignment.1 {
            VerticalAlignment::Top => (),
            VerticalAlignment::Center => pos.y -= fragment.fbox.size.y / 2,
            VerticalAlignment::Bottom => pos.y -= fragment.fbox.size.y,
        }

        Ok(Some((pos, fragment)))
    }
}

fn pen_to_size_independent_styles(pen: &Pen, set_default: bool) -> StyleMap {
    let mut result = StyleMap::new();

    if set_default || pen.font_style != Pen::DEFAULT.font_style {
        result.set::<style::FontFamily>(
            font_style_to_name(pen.font_style)
                .iter()
                .copied()
                .map(Rc::<str>::from)
                .collect(),
        );
    }

    if pen.bold {
        result.set::<style::FontWeight>(I16Dot16::new(700));
    }

    if pen.italic {
        result.set::<style::FontStyle>(FontSlant::Italic);
    }

    if set_default || pen.foreground_color != Pen::DEFAULT.foreground_color {
        result.set::<style::Color>(BGRA8::from_rgba32(pen.foreground_color));
    }

    if set_default || pen.background_color != Pen::DEFAULT.background_color {
        result.set::<style::BackgroundColor>(BGRA8::from_rgba32(pen.background_color));
    }

    result
}

fn convert_segment(segment: &super::Segment, ruby: Ruby) -> Segment {
    let style = pen_to_size_independent_styles(segment.pen(), false);

    Segment {
        base_style: style,
        font_size: segment.pen().font_size,
        time_offset: segment.time_offset,
        text: segment.text.as_str().into(),
        shadow: Srv3TextShadow {
            kind: segment.pen().edge_type,
            color: BGRA8::from_argb32(segment.pen().edge_color | 0xFF000000),
        },
        ruby,
    }
}

pub fn convert(sbr: &Subrandr, document: Document) -> Subtitles {
    // let mut result = Subtitles {
    //     root_style: pen_to_size_independent_styles(&Pen::DEFAULT, true),
    //     windows: Vec::new(),
    // };

    // log_once_state!(ruby_under_unsupported);

    // let mut wname_to_index = HashMap::new();
    // for (name, window) in document.windows() {
    //     wname_to_index.insert(&**name, result.windows.len());
    //     result.windows.push(Window {
    //         x: convert_coordinate(window.position().x as f32),
    //         y: convert_coordinate(window.position().y as f32),
    //         range: window.time..window.time + window.duration,
    //         alignment: window.position().point.to_alignment(),
    //         events: Vec::new(),
    //     });
    // }

    // for event in document.events() {
    //     let mut segments = vec![];

    //     let mut it = event.segments.iter();
    //     'segment_loop: while let Some(segment) = it.next() {
    //         'ruby_failed: {
    //             if segment.pen().ruby_part == RubyPart::Base && it.as_slice().len() > 3 {
    //                 let ruby_block = <&[_; 3]>::try_from(&it.as_slice()[..3]).unwrap();

    //                 if !matches!(
    //                     ruby_block.each_ref().map(|s| s.pen().ruby_part),
    //                     [
    //                         RubyPart::Parenthesis,
    //                         RubyPart::Over | RubyPart::Under,
    //                         RubyPart::Parenthesis,
    //                     ]
    //                 ) {
    //                     break 'ruby_failed;
    //                 }
    //                 let ruby = match ruby_block[1].pen().ruby_part {
    //                     RubyPart::Over => Ruby::Over,
    //                     RubyPart::Under => {
    //                         warning!(
    //                             sbr,
    //                             once(ruby_under_unsupported),
    //                             "`ruby-position: under`-style ruby text is not supported yet"
    //                         );
    //                         break 'ruby_failed;
    //                     }
    //                     _ => unreachable!(),
    //                 };

    //                 segments.push(convert_segment(segment, Ruby::Base));
    //                 _ = it.next().unwrap();
    //                 segments.push(convert_segment(it.next().unwrap(), ruby));
    //                 _ = it.next().unwrap();

    //                 continue 'segment_loop;
    //             }
    //         }

    //         segments.push(convert_segment(segment, Ruby::None));
    //     }

    //     if let Some(&widx) = event
    //         .window_id
    //         .as_ref()
    //         .and_then(|wname| wname_to_index.get(&**wname))
    //     {
    //         let window = &mut result.windows[widx];
    //         window.events.push(WindowEvent {
    //             range: event.time..event.time + event.duration,
    //             segments,
    //         });
    //     } else {
    //         result.windows.push(Window {
    //             x: convert_coordinate(event.position().x as f32),
    //             y: convert_coordinate(event.position().y as f32),
    //             range: event.time..event.time + event.duration,
    //             alignment: event.position().point.to_alignment(),
    //             events: vec![WindowEvent {
    //                 range: event.time..event.time + event.duration,
    //                 segments,
    //             }],
    //         });
    //     }
    // }

    // result
    todo!()
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

    pub fn create(&mut self, realm: &Rc<Realm>) -> miniweb::dom::Document {
        let mut document = miniweb::dom::Document::new(realm.clone());

        document.style_rules.push(miniweb::dom::Rule {
            selectors: vec![miniweb::dom::Selector {
                name: Some(realm.symbol("window")),
                id: None,
                classes: Vec::new(),
                time_interval: 0..u32::MAX,
                future: false,
                past: false,
            }],
            specificity: 0,
            declarations: {
                let mut result = StyleMap::new();

                result.set::<style::Display>(Display::NONE);

                result
            },
        });

        document
    }

    pub fn update(&mut self, document: &mut miniweb::dom::Document) {
        document.root().children.clear();

        // for window in &self.subtitles.windows {
        //     if !pass.add_event_range(window.range.clone()) {
        //         continue;
        //     }

        //     if let Some((pos, block)) = window.layout(pass, &self.subtitles.root_style)? {
        //         pass.emit_fragment(pos, block);
        //     }
        // }
        // Ok(())
    }
}
