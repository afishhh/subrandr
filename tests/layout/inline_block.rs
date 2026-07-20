use rasterize::color::BGRA8;

use crate::{
    layout::FixedL,
    style::computed::{HorizontalAlignment, InlineSizing, Length},
};

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

    .transparent_red_bg { background_color: BGRA8::RED.mul_alpha(255 / 2) }
    .transparent_green_bg { background_color: BGRA8::GREEN.mul_alpha(255 / 2) }
}

check_test! {
    name = simple_nested_ahem,
    size = (32 + 140 + 16, 32),
    inline.ahem {
        span.blue_bg {
            span.fs20.transparent_red_bg {
                span.fs32.transparent_green_bg { text "縦" }
                block {
                    inline { text "横block横" }
                }
            }
            text "縦"
        }
    }
}

check_test! {
    // Checks whether a baseline is correctly synthethized from the margin box.
    // The bottom edge of the block should be aligned to the inline's baseline.
    name = padding_only,
    size = (32 + 40 + 16, 32),
    inline.ahem {
        span.fs20.red_bg {
            span.fs32 { text "縦" }
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
                text "ppp"
            }
        }
    }
}

// Since only `BaselineSource::Last` is currently supported, this isn't *that* interesting.
check_test! {
    name = multiline,
    size = (16 + 6 * 16 + 16, 32),
    inline.ahem {
        span.blue_bg {
            text "縦"
            block {
                inline { text "top\nbottom" }
            }
            text "縦"
        }
    }
}

check_test! {
    name = breaking,
    size = (16 * 5, 48),
    inline.ahem {
        block.red {
            inline { text "XXXX" }
        }
        block.green {
            inline { text "YYYYY" }
        }
        block.blue {
            inline { text "ZZZ" }
        }
        block.yellow {
            inline { text "WW" }
        }
    }
}

test_define_style! {
    .black_on_white {
        color: BGRA8::BLACK,
        background_color: BGRA8::WHITE,
    }

    .text_center { text_align: HorizontalAlignment::Center }
    .inline_sizing_stretch { inline_sizing: InlineSizing::Stretch }

    .hmargin5 {
        margin_left: Some(Length::from_pixels(FixedL::new(5))),
        margin_right: Some(Length::from_pixels(FixedL::new(5))),
    }
    .vpadding5 {
        padding_top: Length::from_pixels(FixedL::new(5)),
        padding_bottom: Length::from_pixels(FixedL::new(5)),
    }

    .width32 {
        width: Some(Length::from_pixels(FixedL::new(32)))
    }
}

// NOTE: Once/if vertical margins are support these `vpadding`s can be replaced
//       by the respective `vmargin`s.
check_test! {
    name = margins,
    size = (106, 88),
    block.ahem.black_on_white.text_center {
        inline.ahem {
            span.inline_sizing_stretch.yellow_bg {
                text "A"
                block.hmargin5.vpadding5 {
                    inline { text "XXXX" }
                }
                text "A\n"
            }
            span.inline_sizing_stretch.red_bg {
                text "B"
                block.hmargin5.vpadding10 {}
                text "B\n"
            }
            span.inline_sizing_stretch.green_bg {
                text "C"
                block.vpadding10 {}
                text "C\n"
            }
            span.inline_sizing_stretch.blue_bg {
                text "D"
                block.hmargin5 {}
                text "D"
            }
        }
    }
}

check_test! {
    name = width_calculation,
    size = (64, 48),
    block.ahem.black_on_white.text_center {
        inline.ahem {
            span.inline_sizing_stretch.yellow_bg {
                text "A"
                block {
                    block {
                        block.width32 { inline { text "横" } }
                        block { inline { text "縦" } }
                    }
                }
                text "A\n"
            }
            span.inline_sizing_stretch.green_bg {
                text "B"
                block { inline { text "BB" } }
                text "B\n"
            }
        }
    }
}
