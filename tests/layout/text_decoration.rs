use super::common::*;

test_define_style! {
    .vpadding10 "
        padding-top: 10px;
        padding-bottom: 10px;
    "
    .hpadding5 "
        padding-left: 5px;
        padding-right: 5px;
    "
    .rpadding16 "padding-right: 16px;"

    .red_underline "
        text-decoration-line: underline;
        text-decoration-color: red;
    "
    .green_strikethrough "
        text-decoration-line: line-through;
        text-decoration-color: lime;
    "
    .red_strikethrough "
        text-decoration-line: line-through;
        text-decoration-color: red;
    "
    .blue_strikethrough "
        text-decoration-line: line-through;
        text-decoration-color: blue;
    "
    .yellow_strikethrough "
        text-decoration-line: line-through;
        text-decoration-color: yellow;
    "
    .currentcolor_strikethrough "text-decoration-line: line-through"
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
