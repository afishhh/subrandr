use rasterize::color::BGRA8;
use util::math::I26Dot6;

use crate::{layout::FixedL, style::computed::Length};

use super::common::*;

test_define_style! {
    .vpadding10 {
        padding_top: Length::from_pixels(FixedL::new(10)),
        padding_bottom: Length::from_pixels(FixedL::new(10)),
    }
    .hpadding20 {
        padding_left: Length::from_pixels(FixedL::new(20)),
        padding_right: Length::from_pixels(FixedL::new(20)),
    }
    .red_bg { background_color: BGRA8::RED }
    .transparent_red_bg { background_color: BGRA8::RED.mul_alpha(255 / 2) }
    .transparent_green_bg { background_color: BGRA8::GREEN.mul_alpha(255 / 2) }
    .blue_bg { background_color: BGRA8::BLUE }
    .large { font_size: I26Dot6::new(32) }
    .larger { font_size: I26Dot6::new(20) }
}

check_test! {
    name = simple_nested_ahem,
    size = (32 + 140 + 16, 32),
    inline.ahem.blue_bg {
        span.larger.transparent_red_bg {
            span.large.transparent_green_bg { text "縦" }
            block {
                inline { text "横block横" }
            }
        }
        text "縦"
    }
}

check_test! {
    // Checks whether a baseline is correctly synthethized from the margin box.
    // The bottom edge of the block should be aligned to the inline's baseline.
    name = padding_only,
    size = (32 + 40 + 16, 32),
    inline.ahem {
        span.larger.red_bg {
            span.large { text "縦" }
            block.blue_bg.hpadding20.vpadding10 {}
        }
        text "縦"
    }
}

check_test! {
    name = in_ruby,
    size = (16 * 3, 32),
    // Rectangle with a hat 😃
    inline.ahem {
        ruby {
            base {
                block.blue_bg.hpadding20.vpadding10 {}
            }
            annotation {
                text "横横横"
            }
        }
    }
}

// Since only `BaselineSource::Last` is currently supported, this isn't *that* interesting.
check_test! {
    name = multiline,
    size = (16 + 6 * 16 + 16, 32),
    inline.ahem.blue_bg {
        text "縦"
        block {
            inline { text "top\nbottom" }
        }
        text "縦"
    }
}
