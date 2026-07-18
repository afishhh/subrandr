use super::common::*;

test_define_style! {
    .padding_left_16 "padding-left: 16px"
    .padding_right_16 "padding-right: 16px"
    .margin_left_16 "margin-left: 16px"
    .margin_right_16 "margin-right: 16px"
    .margin_right_32 "margin-right: 32px"
}

check_test! {
    name = simple_padding,
    size = (16 * 14, 16),
    inline.ahem {
        span.green_bg.padding_left_16.padding_right_16 {
            text "hello"
        }
        span.red_bg.padding_right_16 {
            text "world"
        }
    }
}

check_test! {
    name = line_broken_padding,
    size = (16 * 6, 16 * 2),
    inline.ahem {
        span.green_bg.padding_left_16.padding_right_16 {
            text "hello world"
        }
    }
}

check_test! {
    name = flush_on_left_padding,
    size = (16 * 8, 16 * 2),
    inline.ahem {
        span.green_bg.padding_left_16 {
            text "hello!"
        }
        span.green_bg.padding_left_16 {
            text "x"
        }
    }
}

check_test! {
    name = padding_sensitive_breaking,
    size = (16 * 8, 16 * 3),
    inline.ahem {
        // This should fit on a single line
        span.green_bg.padding_left_16.padding_right_16 {
            text "hello!"
        }
        // This should just barely get broken (and without padding it wouldn't)
        span.green_bg.padding_right_16 {
            text "hi steve"
        }
    }
}

check_test! {
    name = padding_sensitive_breaking2,
    size = (16 * 8, 16 * 3),
    inline.ahem {
        span.green_bg.padding_left_16.padding_right_16 {
            text "hi mark\n"
        }
        span.green_bg.padding_left_16.padding_right_16 {
            text "hi bob"
        }
    }
}

check_test! {
    name = padding_sensitive_breaking3,
    size = (16 * 9, 16 * 2),
    inline.ahem {
        span.green_bg.padding_left_16.padding_right_16 {
            text "hello"
        }
        span.green_bg.padding_left_16 {
            text "xy"
        }
    }
}

// FIXME: This is currently broken (should have green on the right but doesn't).
//        Empty spans are currently never re-materialized into the fragment tree
//        after they understandably emit no content.
//        Very much an edge case though.
check_test! {
    name = empty_padded_span,
    size = (16 * 7, 16),
    inline.ahem {
        span.green_bg.padding_left_16 {
            text "hello"
        }
        span.padding_right_16 {}
    }
}

check_test! {
    name = margins,
    size = (16 * 14, 32),
    inline.ahem {
        span.yellow_bg {
            span.green_bg.margin_left_16.margin_right_32 {
                text "hello"
            }
            span.red_bg.padding_left_16.margin_right_16 {
                text "world"
            }
        }
    }
}
