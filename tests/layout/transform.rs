use super::common::*;

test_define_style! {
    .rotate_90deg "transform: rotate(90deg)"
    .rotate_180deg "transform: rotate(180deg)"
    .rotate_270deg "transform: rotate(270deg)"
    .size100x50 "width: 100px; height: 50px"
    .size50x100 "width: 50px; height: 100px"
    .padding10 "padding-left: 10px; padding-right: 10px; padding-top: 10px; padding-bottom: 10px"
    .vpadding10 "padding-top: 10px; padding-bottom: 10px"
    .hpadding30 "padding-left: 30px; padding-right: 30px"
    .tpadding10 "padding-top: 10px"
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
