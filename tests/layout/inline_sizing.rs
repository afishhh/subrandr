use super::common::*;
use crate::style::computed::InlineSizing;

test_define_style! {
    .stretch { inline_sizing: InlineSizing::Stretch }
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
            span.fs20 {
                text "る"
            }
        }
    }
}
