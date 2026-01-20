use rasterize::color::BGRA8;

use crate::{
    layout::FixedL,
    style::computed::{Length, TextDecorations},
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
    .red_bg { background_color: BGRA8::RED }
    .blue_bg { background_color: BGRA8::BLUE }

    .normal { font_size: FixedL::new(16) }
    .large { font_size: FixedL::new(24) }
    .larger { font_size: FixedL::new(32) }

    .underline {
        text_decoration: TextDecorations {
            underline: true,
            underline_color: BGRA8::RED,
            ..TextDecorations::default()
        }
    }
    .green_strikethrough {
        text_decoration: TextDecorations {
            line_through: true,
            line_through_color: BGRA8::GREEN,
            ..TextDecorations::default()
        }
    }
    .red_strikethrough {
        text_decoration: TextDecorations {
            line_through: true,
            line_through_color: BGRA8::RED,
            ..TextDecorations::default()
        }
    }
    .blue_strikethrough {
        text_decoration: TextDecorations {
            line_through: true,
            line_through_color: BGRA8::BLUE,
            ..TextDecorations::default()
        }
    }
    .yellow_strikethrough {
        text_decoration: TextDecorations {
            line_through: true,
            line_through_color: BGRA8::YELLOW,
            ..TextDecorations::default()
        }
    }
}

check_test! {
    name = on_span,
    size = (216, 36),
    inline.ahem.underline {
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
        span.large.green_strikethrough  {
            span.underline {
                text "LARGE"
                span.normal {
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
        span.large.blue_strikethrough {
            text "LARGE"
        }
        ruby.large.green_strikethrough {
            base {
                text "base"
            }
            annotation.normal.yellow_strikethrough {
                text "annotation"
            }
        }
        span.red_strikethrough {
            text "small"
        }
    }
}
