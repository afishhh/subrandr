use super::common::*;

test_define_style! {
    .rotate_90deg "transform: rotate(90deg)"
    .rotate_180deg "transform: rotate(180deg)"
    .rotate_270deg "transform: rotate(270deg)"
    .scale_3_2_translate_0_10 "transform: scale(3, 2) translate(0, 10px)"
    .width50 "width: 50px"
    .size100x50 "width: 100px; height: 50px"
    .size50x100 "width: 50px; height: 100px"
    .padding10 "padding-left: 10px; padding-right: 10px; padding-top: 10px; padding-bottom: 10px"
    .vpadding10 "padding-top: 10px; padding-bottom: 10px"
    .hpadding30 "padding-left: 30px; padding-right: 30px"
    .tpadding10 "padding-top: 10px"
    .hmargin_auto "margin-left: auto; margin-right: auto"

    .noto_sans_jp_noto_color_emoji "font-family: Noto Sans JP, Noto Color Emoji"
}

check_test! {
    name = rotate_hello_world,
    size = (120, 120),
    block.noto_serif.yellow_bg.padding10 {
        block.rotate_90deg {
            block.size100x50.blue_bg { inline {
                text "hello world!!!"
            } }
            block.size100x50.green_bg {}
        }
    }
}

check_test! {
    name = rotate_emoji,
    size = (110, 450),
    block.noto_color_emoji.fs20.yellow_bg.vpadding10.hpadding30 {
        block.size50x100.blue_bg {
            inline { text "😀🧱😭⭕️" }
        }

        block.tpadding10 {}

        block.size50x100.blue_bg.rotate_90deg {
            inline { text "😀🧱😭⭕️" }
        }

        block.tpadding10 {}

        block.size50x100.blue_bg.rotate_180deg {
            inline { text "😀🧱😭⭕️" }
        }

        block.tpadding10 {}

        block.size50x100.blue_bg.rotate_270deg {
            inline { text "😀🧱😭⭕️" }
        }
    }
}

test_define_style! {
    .vertical_lr "writing-mode: vertical-lr"
    .underline "text-decoration-line: underline; text-decoration-color: red"
    .strike "text-decoration-line: line-through; text-decoration-color: black"

    .fs256 "font-size: 256px"
}

check_test! {
    name = rotate_orthogonal,
    size = (140, 120),
    block.yellow_bg.padding10 {
        block.rotate_180deg {
            block.size100x50.noto_sans_jp.vertical_lr.blue_bg {
                inline { span.underline { span.strike { text "ハロー世界！" } } }
            }
            block.size100x50.noto_serif.green_bg {
                inline { span.underline { text "I'm upside down!" } }
            }
        }
    }
}

// This triggers the sparse glyph rasterization path and makes sure it works with
// the rotation transform applied by vertical-lr to sideways glyphs.
check_test! {
    name = big_vertical_lr,
    size = (340, 1550),
    inline.noto_sans_jp.vertical_lr.fs256 {
        text "Hello, world!"
    }
}

check_test! {
    name = scale_translate,
    size = (180, 105),
    block.noto_sans_jp_noto_color_emoji.fs20.yellow_bg.padding10 {
        block.width50.blue_bg {
            inline { text "Hi!😀" }
        }

        block.width50.hmargin_auto.scale_3_2_translate_0_10.blue_bg {
            inline { text "Hi!😀" }
        }
    }
}
