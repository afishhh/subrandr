use super::common::*;

test_define_style! {
    .vpadding10 "padding-top: 10px; padding-bottom: 10px"
    .hpadding20 "padding-left: 20px; padding-right: 20px"

    .transparent_red_bg "background-color: #FF00007F"
    .transparent_green_bg "background-color: #00FF007F"
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
