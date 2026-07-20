use rasterize::color::BGRA8;

use crate::{
    layout::FixedL,
    style::computed::{Direction, HorizontalAlignment, InlineSizing, Length},
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
    .vpadding4 {
        padding_top: Length::from_pixels(FixedL::new(4)),
        padding_bottom: Length::from_pixels(FixedL::new(4)),
    }
    .lpadding6 { padding_left: Length::from_pixels(FixedL::new(6)) }

    .lmargin_auto { margin_left: None }
    .lmargin10 { margin_left: Some(Length::from_pixels(FixedL::new(10))) }
    .lmargin48 { margin_left: Some(Length::from_pixels(FixedL::new(48))) }
    .rmargin_auto { margin_right: None }
    .rmargin16 { margin_right: Some(Length::from_pixels(FixedL::new(16))) }
    .rmargin22 { margin_right: Some(Length::from_pixels(FixedL::new(22))) }
    .hmargin_auto { margin_left: None, margin_right: None }
    .hmargin40 {
        margin_left: Some(Length::from_pixels(FixedL::new(40))),
        margin_right: Some(Length::from_pixels(FixedL::new(40))),
    }

    .width32 { width: Some(Length::from_pixels(FixedL::new(32))) }
    .width64 { width: Some(Length::from_pixels(FixedL::new(64))) }

    .rtl { direction: Direction::Rtl }

    .inline_sizing_stretch { inline_sizing: InlineSizing::Stretch }
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

check_test! {
    name = horizontal_margins,
    size = (96, 16 * 8 + 8),
    block.ahem.black_on_white {
        // Auto width (case 4)
        block.hmargin_auto.yellow_bg {
            inline { text "A" }
        }

        // Padding + auto width (case 4)
        block.lmargin10.rmargin16.lpadding6.vpadding4.red_bg {
            inline { text "BBBB" }
        }

        // Auto left margin (case 3)
        block.width64.lmargin_auto.rmargin22.green_bg {
            inline { text "CCCC" }
        }
        // Auto right marign (case 2)
        block.width64.lmargin10.rmargin_auto.green_bg {
            inline { text "CCCC" }
        }

        // Centered (case 5)
        block.hmargin_auto.width32.blue_bg {
            inline { text "D" }
        }

        // Overconstrained values (case 1)
        block.hmargin40.width32.green_bg {
            inline { text "E" }
        }
        block.rtl.text_right {
            block.hmargin40.width32.green_bg {
                inline { text "E" }
            }
        }

        // Overflowing total width
        block.hmargin_auto.width64.red_bg {
            block.lmargin48.width32.blue_bg {
                inline { text "F" }
            }
        }
    }
}

// TODO: inline margins
