use rasterize::color::BGRA8;
use util::math::I26Dot6;

use super::common::*;

test_define_style! {
    .break_anywhere {
        line_break: icu_segmenter::options::LineBreakStrictness::Anywhere,
    }
    .fs32 {
        font_size: I26Dot6::new(32)
    }

    .red { color: BGRA8::RED }
    .green_bg { background_color: BGRA8::GREEN }
    .blue { color: BGRA8::BLUE }
}

check_test! {
    name = in_text,
    size = (16 * 6, 16 * 2),
    inline.ahem.break_anywhere {
        span.red {
            text ":line_"
        }
        span.blue {
            text "break:"
        }
    }
}

check_test! {
    name = in_space,
    size = (16 * 6, 16 * 2),
    inline.ahem.break_anywhere {
        span.red {
            text ":line"
        }
        span.green_bg {
            text " "
        }
        span.blue {
            text "break:"
        }
    }
}

check_test! {
    name = after_space,
    size = (16 * 6, 16 * 2),
    inline.ahem.break_anywhere {
        span.red {
            text "line"
        }
        span.green_bg {
            text " "
        }
        span.blue {
            text "break:"
        }
    }
}

check_test! {
    name = multibyte,
    size = (16 * 6, 16 * 2),
    inline.ahem.break_anywhere {
        span.red {
            text ":line"
        }
        span.green_bg {
            text "横縦"
        }
        span.blue {
            text "break"
        }
    }
}

// FIXME: Why's there a vertical line to the right of the first tofu?
check_test! {
    name = emoji_sequence,
    size = (32 * 7, 32 * 3),
    inline.ahem.fs32.break_anywhere {
        span.red {
            // U+1F9DF - zombie
            // U+200D  - ZWJ
            // U+2640  - female sign
            // U+FE0F  - variation selector 16 (emoji presentation)
            text "emoji:🧟‍♀️"
        }
        span.blue {
            text "break:"
        }
    }
}

test_define_style! {
    .g1 { color: BGRA8::new(0, 20, 0, 255) }
    .g2 { color: BGRA8::new(0, 40, 0, 255) }
    .g3 { color: BGRA8::new(0, 60, 0, 255) }
    .g4 { color: BGRA8::new(0, 80, 0, 255) }
    .g5 { color: BGRA8::new(0, 110, 0, 255) }
    .g6 { color: BGRA8::new(0, 140, 0, 255) }
    .g7 { color: BGRA8::new(0, 170, 0, 255) }
    .g8 { color: BGRA8::new(0, 220, 0, 255) }
}

check_test! {
    name = serif_ltr,
    size = (240, 135),
    inline.noto_serif.fs32.break_anywhere {
        span.g1 {
            text "The"
        }
        span.g2 {
            text " quick"
        }
        span.g3 {
            text " brown fox "
        }
        span.g4 {
            text "jumps "
        }
        span.g5 {
            text "over "
        }
        span.g6 {
            text "the"
        }
        span.g7 {
            text " lazy"
        }
        span.g8 {
            text " dog."
        }
    }
}

check_test! {
    name = serif_ltr_reshaping,
    size = (76, 90),
    inline.noto_serif.fs32.break_anywhere {
        span.g1 {
            text "conf"
        }
        // The last character from the preceeding string forms a ligature
        // with the first character from this string.
        // Since we're breaking in-between them this'll cause reshaping to happen
        // during the break.
        span.g2 {
            text "lict"
        }
    }
}

check_test! {
    name = ahem_rtl,
    size = (16 * 6, 16 * 4),
    inline.ahem.break_anywhere {
        text "\u{202E}"
        span.g1 {
            text "Th"
        }
        span.g2 {
            text "is"
        }
        span.g3 {
            text " is"
        }
        span.g4 {
            text " right"
        }
        span.g5 {
            text " -to-"
        }
        span.g6 {
            text " left"
        }
        span.g7 {
            text "!"
        }
    }
}

check_test! {
    name = arabic_rtl,
    align = Right,
    size = (16 * 13, 64 * 3),
    inline.noto_sans_arabic.fs32.break_anywhere {
        span.g1 {
            text "لمّا كان الاعترا"
        }
        span.g2 {
            text "ف بالك"
        }
        span.g3 {
            text "رامة المتأصلة"
        }
        span.g4 {
            text "في جميع"
        }
    }
}

check_test! {
    name = arabic_rtl_reshaping,
    align = Right,
    size = (16. * 4., 64 * 2),
    inline.noto_sans_arabic.fs32.break_anywhere {
        span.g1 {
            text "جمي"
        }
        span.g2 {
            text "ع"
        }
    }
}
