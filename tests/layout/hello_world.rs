use rasterize::color::BGRA8;

use super::common::*;

test_define_style! {
    .world {
        color: BGRA8::BLUE,
        background_color: BGRA8::RED,
    }
}

check_test! {
    name = basic,
    size = (16 * 12, 16),
    inline.ahem {
        text "Hello, "
        span.world {
            text "横縦pÉ!"
        }
    }
}

test_define_style! {
    .hello { color: BGRA8::LIME }
}

check_test! {
    name = line_broken,
    size = (16 * 6, 16 * 2),
    inline.ahem {
        span.hello {
            text "Hello"
        }
        text ", "
        span.world {
            text "横縦pÉ!"
        }
    }
}

check_test! {
    name = explicitly_rtl,
    direction = Rtl,
    size = (16 * 12, 16),
    inline.ahem {
        span.hello {
            text "縦 "
        }
        text "\u{202B}"
        span.world {
            text "world hi"
        }
        text "\u{202C}"
        span.hello {
            text " 横"
        }
    }
}
