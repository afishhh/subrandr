use rasterize::color::BGRA8;

use super::common::*;

test_define_style! {
    .break_anywhere {
        line_break: icu_segmenter::options::LineBreakStrictness::Anywhere,
    }

    .red { color: BGRA8::RED }
    .green_bg { background_color: BGRA8::GREEN }
    .blue { color: BGRA8::BLUE }
}

// [UAX#14 LB7] prohibits breaking before a ZWSP character in the below scenario.
// *However* CSS-specific breaking rules require a soft wrap opportunity to be
// inserted at the end of a sequence of space|tab anyway. Since CSS rules have
// to be followed regardless of UAX#14, a break opportunity must be inserted
// after the space in-between the ZWSPs.
//
// [UAX#14 LB7]: https://www.unicode.org/reports/tr14/#LB7
check_test! {
    name = space_between_zwsp,
    size = (16 * 3, 16 * 3),
    inline.ahem {
        span.green_bg {
            text "A B\u{200B} \u{200B}\n"
        }
        text "hello"
    }
}
