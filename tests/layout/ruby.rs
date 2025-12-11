use rasterize::color::BGRA8;
use util::math::I26Dot6;

use super::common::*;

test_define_style! {
    .blue { color: BGRA8::BLUE }
    .green_bg { background_color: BGRA8::GREEN }
    .red_bg { background_color: BGRA8::RED }
    .larger { font_size: I26Dot6::new(20) }
    .much_larger { font_size: I26Dot6::new(32) }
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
    size = (16 * 3, 36),
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

check_test! {
    name = stacked,
    size = (36, 90),
    inline.noto_sans_jp {
        ruby {
            base.green_bg {
                text "行"
            }
            annotation.green_bg.blue {
                text "い"
            }
        }
        span.green_bg.larger { text "く\n" }
        ruby {
            base.green_bg {
                text "行"
            }
            annotation.green_bg.blue {
                text "い"
            }
        }
        span.green_bg { text "く\n" }
    }
}

// TODO: On Chromium the annotation background is actually on top of the base (background *and text*).
// Also Chromium places the annotations a bit higher than us but I'm not sure why and its precise
// positioning is not specified by the standard sooooo.
check_test! {
    name = base_and_annotation_backgrounds,
    size = (16 * 3, 36),
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

check_test! {
    name = large_in_base,
    size = (16 * 5, 40),
    inline.noto_sans_jp {
        ruby {
            base.green_bg {
                span.larger {
                    text "大"
                }
                text "小"
            }
            annotation.blue.red_bg {
                text "だいしょう"
            }
        }
    }
}

// TODO: This matches Firefox, but Chromium takes the maximum of all base ascenders
//       for positioning annotations. Which is correct?
check_test! {
    name = single_base_ascender_only,
    size = (16 * 5, 54),
    inline.noto_sans_jp {
        ruby {
            base.green_bg {
                span.much_larger {
                    text "大"
                }
            }
            annotation.blue.red_bg {
                text "だい"
            }
            base {
                text "小"
            }
            annotation {
                text "しょう"
            }
        }
    }
}
