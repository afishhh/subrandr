use rasterize::color::BGRA8;
use util::math::I26Dot6;

use super::common::*;
use crate::style::computed::InlineSizing;

test_define_style! {
    .green_bg { background_color: BGRA8::GREEN }
    .red_bg { background_color: BGRA8::RED }
    .stretch { inline_sizing: InlineSizing::Stretch }
    .larger { font_size: I26Dot6::new(20) }
    .normal { font_size: I26Dot6::new(16) }
}

check_test! {
    name = normal,
    size = (140, 24),
    inline.noto_serif {
        span.green_bg {
            text "hello"
        }
        text " "
        span.ahem.red_bg {
            text "world縦"
        }
    }
}

check_test! {
    name = stretch,
    size = (140, 24),
    inline.noto_serif.stretch {
        span.green_bg {
            text "hello"
        }
        text " "
        span.ahem.red_bg {
            text "world縦"
        }
    }
}

check_test! {
    name = after_break,
    size = (100, 40),
    inline.noto_serif.stretch {
        span.green_bg {
            text "hello"
        }
        text " "
        span.ahem.red_bg {
            text "world縦"
        }
    }
}

check_test! {
    name = ruby_background,
    size = (70, 40),
    inline.noto_sans_jp.stretch {
        ruby {
            base.green_bg {
                text "広"
            }
            annotation.red_bg {
                text "ひろ"
            }
        }
        span.green_bg {
            text "が"
            span.larger {
                text "る"
            }
        }
    }
}
