use util::rc_static;

use super::common::*;

test_define_style! {
    .noto_arabic_ahem_emoji {
        font_family: rc_static!([
            rc_static!(str b"Noto Sans Arabic"),
            rc_static!(str b"Ahem"),
            rc_static!(str b"Noto Color Emoji")
        ])
    }

    .noto_serif_ahem_emoji {
        font_family: rc_static!([
            rc_static!(str b"Noto Serif"),
            rc_static!(str b"Ahem"),
            rc_static!(str b"Noto Color Emoji")
        ])
    }
}

check_test! {
    name = edge_reshaping,
    size = (165, 230),
    block.ahem {
        block {
            inline.noto_arabic_ahem_emoji.align_right.fs32 {
                span.red {
                    text "⭕️لمّا كا ا⭕️\n"
                }
                span.green {
                    text "🧱لمّا كا ا🧱"
                }
            }
        }

        block {
            inline.noto_serif_ahem_emoji.fs32 {
                span.red {
                    text "⭕️EDGE⭕️\n"
                }
                span.green {
                    text "🧱EDGE🧱"
                }
            }
        }
    }
}
