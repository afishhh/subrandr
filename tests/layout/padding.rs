use rasterize::color::BGRA8;

use super::common::*;
use crate::{layout::FixedL, style::computed::Length};

test_define_style! {
    .green_bg { background_color: BGRA8::GREEN }
    .red_bg { background_color: BGRA8::RED }
    .padding_left_16 { padding_left: Length::from_pixels(FixedL::new(16)) }
    .padding_right_16 { padding_right: Length::from_pixels(FixedL::new(16)) }
}

check_test! {
    name = simple,
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
    name = line_broken,
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
