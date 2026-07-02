use super::common::*;

test_define_style! {
    .green_shadow "text-shadow: lime 5px 5px"
    .green_shadow_blurred "text-shadow: lime 5px 5px 3px"
    .blue_shadow_blurred "text-shadow: blue 5px 5px 3px"
    .many_shadows "text-shadow: 3px 3px red, 5px 5px 3px lime, 7px 7px blue"
    .red_shadow_very_blurred "text-shadow: 5px 5px 8px red"
}

check_test! {
    name = simple,
    size = (140, 30),
    inline.noto_serif {
        span.green_shadow {
            text "hello world"
        }
    }
}

check_test! {
    name = blurred_line_broken,
    size = (60, 50),
    inline.noto_serif {
        span.green_shadow_blurred {
            text "hello world"
        }
    }
}

// TODO: Is this correct? Since we use the broken gamma-encoded blending it's hard to tell...
check_test! {
    name = many,
    size = (60, 50),
    inline.noto_serif {
        span.many_shadows {
            text "hello world"
        }
    }
}

check_test! {
    name = emoji,
    size = (90, 32),
    inline.noto_serif.green_shadow {
        span.noto_color_emoji {
            text "😀🧱"
        }
        span.noto_color_emoji.blue_shadow_blurred {
            text "😭⭕️"
        }
    }
}

check_test! {
    name = large,
    size = (155, 105),
    inline.noto_sans_jp.fs64.red_shadow_very_blurred {
        span.noto_color_emoji {
            text "⭕️"
        }
        text "赤"
    }
}
