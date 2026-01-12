use rasterize::color::BGRA8;

use crate::{
    layout::FixedL,
    style::computed::{HorizontalAlignment, Length},
};

use super::common::*;

test_define_style! {
    .black_on_white {
        color: BGRA8::BLACK,
        background_color: BGRA8::WHITE,
    }
    .text_centered { text_align: HorizontalAlignment::Center }
    .text_right { text_align: HorizontalAlignment::Right }
    .vpadding10 {
        padding_top: Length::from_pixels(FixedL::new(10)),
        padding_bottom: Length::from_pixels(FixedL::new(10)),
    }
    .hpadding5 {
        padding_left: Length::from_pixels(FixedL::new(5)),
        padding_right: Length::from_pixels(FixedL::new(5)),
    }
    .red_bg { background_color: BGRA8::RED }
    .blue_bg { background_color: BGRA8::BLUE }
}

check_test! {
    name = single_centered_inline,
    size = (200, 36),
    block.ahem.black_on_white.text_centered.vpadding10 {
        inline {
            text "縦ab横cd縦"
        }
    }
}

check_test! {
    name = many_lines,
    size = (200, 84),
    block.hpadding5.ahem.black_on_white {
        block.red_bg {
            inline { text "left" }
        }
        block.text_centered.vpadding10 {
            inline {
                ruby {
                    base { text "center" }
                    annotation {
                        text "annotated"
                    }
                }
            }
        }
        block.blue_bg.text_right {
            inline { text "right" }
        }
    }
}
