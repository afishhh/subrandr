use rasterize::color::BGRA8;
use util::{math::I26Dot6, rc_static};

use crate::style::computed::HorizontalAlignment;

use super::common::*;

test_define_style! {
    .fs32 {
        font_size: I26Dot6::new(32)
    }

    .red { color: BGRA8::RED }
    .green { color: BGRA8::GREEN }

    .align_right {
        text_align: HorizontalAlignment::Right
    }

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
