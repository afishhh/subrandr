use super::common::*;
use crate::style::computed::WhiteSpaceCollapse;

test_define_style! {
    .break_anywhere {
        line_break: icu_segmenter::options::LineBreakStrictness::Anywhere,
    }
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

test_define_style! {
    .collapse {
        white_space_collapse: WhiteSpaceCollapse::Collapse,
    }

    .preserve {
        white_space_collapse: WhiteSpaceCollapse::Preserve,
    }
}

// TODO: This result does not actually match browsers but matches my interpretation
//       of the spec. Browsers color the second space green while our current behavior
//       is to color it blue.
//       This matches the spec if the steps in https://www.w3.org/TR/css-text-4/#collapsible-white-space
//       are to be interpreted as sequential operations on the inline's content.
check_test! {
    name = collapse,
    size = (16 * 20, 32),
    inline.ahem.collapse {
        span.green_bg { text "\n   \nfirst    second     " }
        span.blue_bg {
            text "\n\n      third"
            // This span will create a text item that's then killed by stripping
            // of trailing whitespace.
            span { text "\n  " }
        }
    }
}

check_test! {
    name = collapse2,
    size = (16 * 8, 32),
    inline.ahem.collapse {
        span.red_bg { text "asf\n        " }
        span.green_bg.preserve { text "   " }
    }
}

// This doesn't match any browser as of 2026-05-21 but that's because:
// 1. Chromium doesn't collapse the space after the base (but only when an annotation is present).
// 2. WebKit places the annotation intruding in the previous space's space
//    (which is also wrong I think).
// 3. Firefox mostly agrees with us but they extend the background of the base span
//    to cover base padding which we don't do (and I don't recall the spec saying to).
// NOTE: The last space is still present because it's the job of whitespace *hanging* to
//       get rid of it and we don't implement that yet.
check_test! {
    name = collapse_ruby,
    size = (16 * 12, 48),
    inline.ahem.collapse {
        span.green_bg { text "a  " }
        ruby {
            base { span.blue_bg { text " b " } }
            annotation { text "ann" }
        }
        span.yellow_bg { text " c\n" }
        ruby {
            base { span.red_bg { text " d " } }
        }
    }
}
