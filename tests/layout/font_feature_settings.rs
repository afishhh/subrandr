use super::common::*;

test_define_style! {
    .frac "font-feature-settings: 'frac'"
    .smcp "font-feature-settings: 'smcp' on"
    .sinf "font-feature-settings: 'sinf' 1"
    .many "font-feature-settings: 'smcp' 'onum'"
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
