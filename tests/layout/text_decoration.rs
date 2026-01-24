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
        span.fs24.green_strikethrough  {
            span.underline {
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
