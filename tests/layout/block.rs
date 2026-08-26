use super::common::*;

test_define_style! {
    .black_on_white "
        color: black;
        background-color: white;
    "
    .text_centered "text-align: center"
    .text_right "text-align: right"
    .vpadding10 "padding-top: 10px; padding-bottom: 10px"
    .vpadding4 "padding-top: 4px; padding-bottom: 4px"
    .hpadding5 "padding-left: 5px; padding-right: 5px"
    .lpadding6 "padding-left: 6px"

    .lmargin_auto "margin-left: auto"
    .lmargin10 "margin-left: 10px"
    .lmargin48 "margin-left: 48px"
    .rmargin_auto "margin-right: auto"
    .rmargin16 "margin-right: 16px"
    .rmargin22 "margin-right: 22px"
    .hmargin_auto "margin-left: auto; margin-right: auto"
    .hmargin40 "margin-left: 40px; margin-right: 40px"

    .width32 "width: 32px"
    .width64 "width: 64px"

    .rtl "direction: rtl"

    .inline_sizing_stretch "-sbr-inline-sizing: stretch"
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
