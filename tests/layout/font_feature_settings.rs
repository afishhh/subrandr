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
    inline.noto_serif.fs32 {
        text "123/456\n"
        span.frac { text "123/456" }
    }
}

check_test! {
    name = smcp,
    size = (300, 90),
    inline.noto_serif.fs32 {
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
    inline.noto_serif.fs32 {
        text "Here is "
        span.many {
            text "some mixed 123 feature text."
        }
    }
}

check_test! {
    name = ruby_annotation_sinf,
    size = (225, 60),
    inline.noto_serif.fs32 {
        ruby {
            base { text "triethyl citrate" }
            annotation.fs24 {
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
