use util::math::I26Dot6;

use crate::{style::computed::FontFeatureSettings, text::OpenTypeTag};

use super::common::*;

fn make_feature_settings(features: &[([u8; 4], u32)]) -> FontFeatureSettings {
    let mut result = FontFeatureSettings::empty();
    for &(tag_bytes, value) in features {
        result.set(OpenTypeTag::from_bytes(tag_bytes), value);
    }
    result
}

test_define_style! {
    .normal { font_size: I26Dot6::new(24) }
    .big { font_size: I26Dot6::new(32) }
    .frac { font_feature_settings: make_feature_settings(&[(*b"frac", 1)]) }
    .smcp { font_feature_settings: make_feature_settings(&[(*b"smcp", 1)]) }
    .sinf { font_feature_settings: make_feature_settings(&[(*b"sinf", 1)]) }
    .many {
        font_feature_settings: make_feature_settings(&[
            (*b"smcp", 1),
            (*b"onum", 1),
        ])
    }
}

check_test! {
    name = frac,
    size = (120, 90),
    inline.big.noto_serif {
        text "123/456\n"
        span.frac { text "123/456" }
    }
}

check_test! {
    name = smcp,
    size = (300, 90),
    inline.big.noto_serif {
        text "This is some "
        span.smcp {
            text "small caps"
        }
        text " text!!"
    }
}

check_test! {
    name = mixed,
    size = (365, 90),
    inline.big.noto_serif {
        text "Here is "
        span.many {
            text "some mixed 123 feature text."
        }
    }
}

check_test! {
    name = ruby_annotation_sinf,
    size = (225, 60),
    inline.big.noto_serif {
        ruby {
            base { text "triethyl citrate" }
            annotation.normal {
                text "C"
                span.sinf { text "12" }
                text "H"
                span.sinf { text "20" }
                text "O"
                span.sinf { text "7" }
            }
        }
    }
}
