use rasterize::color::BGRA8;

use crate::{
    layout::FixedL,
    style::computed::{Color, Length, TextDecorationLines},
};

use super::common::*;

test_define_style! {
    .vpadding10 {
        padding_top: Length::from_pixels(FixedL::new(10)),
        padding_bottom: Length::from_pixels(FixedL::new(10)),
    }
    .hpadding5 {
        padding_left: Length::from_pixels(FixedL::new(5)),
        padding_right: Length::from_pixels(FixedL::new(5)),
    }
    .rpadding16 {
        padding_right: Length::from_pixels(FixedL::new(16)),
    }

    .red_underline {
        text_decoration_line: TextDecorationLines {
            underline: true,
            ..TextDecorationLines::default()
        },
        text_decoration_color: Color::Srgb(BGRA8::RED),
    }
    .green_strikethrough {
        text_decoration_line: TextDecorationLines {
            line_through: true,
            ..TextDecorationLines::default()
        },
        text_decoration_color: Color::Srgb(BGRA8::GREEN),
    }
    .red_strikethrough {
        text_decoration_line: TextDecorationLines {
            line_through: true,
            ..TextDecorationLines::default()
        },
        text_decoration_color: Color::Srgb(BGRA8::RED),
    }
    .blue_strikethrough {
        text_decoration_line: TextDecorationLines {
            line_through: true,
            ..TextDecorationLines::default()
        },
        text_decoration_color: Color::Srgb(BGRA8::BLUE),
    }
    .yellow_strikethrough {
        text_decoration_line: TextDecorationLines {
            line_through: true,
            ..TextDecorationLines::default()
        },
        text_decoration_color: Color::Srgb(BGRA8::YELLOW),
    }
    .currentcolor_strikethrough {
        text_decoration_line: TextDecorationLines {
            line_through: true,
            ..TextDecorationLines::default()
        },
    }
}

check_test! {
    name = on_span,
    size = (216, 36),
    inline.ahem.red_underline {
        span.blue_strikethrough {
            text "hello   world\n"
            // The underline should not go through this padding
            span.rpadding16 {
                text "hello "
            }
            text " world"
        }
    }
}

check_test! {
    name = differently_sized_spans,
    size = (216, 24),
    inline.ahem.blue_strikethrough  {
        // This strike-through should be higher than the one decorating the
        // root inline box
        span.fs24.green_strikethrough  {
            span.red_underline {
                text "LARGE"
                span.fs16 {
                    text " world"
                }
            }
        }
    }
}

check_test! {
    name = ruby_propagation,
    size = (360, 40),
    inline.ahem {
        span.fs24.blue_strikethrough {
            text "LARGE"
        }
        ruby.fs24.green_strikethrough {
            base {
                text "base"
            }
            annotation.fs16.yellow_strikethrough {
                text "annotation"
            }
        }
        span.red_strikethrough {
            text "small"
        }
    }
}

check_test! {
    name = block_propagation,
    size = (216, 48),
    block.ahem.blue_strikethrough {
        // The above strikethrough should propagate to this block's
        // anonymous root inline and decarate it using its metrics.
        block.fs24 {
            inline {
                span.fs24 { text "LARGE" }
                span.fs16.green_strikethrough {
                    text " world\n"
                }
                text "i"
                // Active decorations should be suspended inside an `inline-block`.
                block.fs16 {
                    inline {
                        text "nline横"
                        span.red_strikethrough {
                            text "bloc"
                        }
                    }
                }
                text "k"
            }
        }
    }
}

check_test! {
    name = currentcolor,
    size = (216, 48),
    inline.ahem {
        span.fs24.red_underline {
            text "横横横横横"
            span.fs16.green.currentcolor_strikethrough {
                text " world\n"
            }
        }
    }
}
