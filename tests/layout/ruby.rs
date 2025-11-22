use rasterize::color::BGRA8;

use super::common::*;

test_define_style! {
    .blue { color: BGRA8::BLUE }
    .green_bg { background_color: BGRA8::GREEN }
    .red_bg { background_color: BGRA8::RED }
}

check_test! {
    name = narrow,
    size = (16 * 22, 16 * 2),
    inline.ahem {
        ruby {
            base {
                text "a small amount"
            }
            annotation.blue {
                text "some"
            }
        }
        text " of text"
    }
}

check_test! {
    name = wide,
    size = (16 * 15, 16 * 2),
    inline.ahem {
        text "som"
        ruby {
            base {
                text "e t"
            }
            annotation.blue {
                text "annotated"
            }
        }
        text "ext"
    }
}

check_test! {
    name = simple,
    size = (16 * 10, 32),
    inline.noto_sans_jp {
        ruby {
            base {
                text "字幕"
            }
            annotation.blue {
                text "じまく"
            }
        }
    }
}

check_test! {
    name = breaks_before,
    size = (48, 60),
    inline.noto_sans_jp {
        // FIXME: The output currently includes the space
        //        (i.e. it is not collapsed on break)
        //        It should instead get trimmed after we line-break the line.
        span.green_bg {
            text "hello "
        }
        ruby {
            base {
                text "世界"
            }
            annotation.blue {
                text "world"
            }
        }
    }
}

// TODO: On Chromium the annotation background is actually on top of the base (background *and text*).
// Also Chromium places the annotations a bit higher than us but I'm not sure why and its precise
// positioning is not specified by the standard sooooo.
check_test! {
    name = base_and_annotation_backgrounds,
    size = (16 * 10, 32),
    inline.noto_sans_jp {
        ruby {
            base.green_bg {
                text "字幕"
            }
            annotation.blue.red_bg {
                text "じまく"
            }
        }
    }
}
