use super::common::*;

test_define_style! {
    .black_on_white "
        color: black;
        background-color: white;
    "
    .text_centered "text-align: center"
    .text_right "text-align: right"
    .vpadding10 "padding-top: 10px; padding-bottom: 10px"
    .hpadding5 "padding-left: 5px; padding-right: 5px"
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
