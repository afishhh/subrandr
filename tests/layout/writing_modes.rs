use super::common::*;

test_define_style! {
    .horizontal_tb "writing-mode: horizontal-tb"
    .vertical_lr "writing-mode: vertical-lr"
    .vertical_rl "writing-mode: vertical-rl"
    .sideways_rl "writing-mode: sideways-rl"
    .annotation "font-feature-settings: 'ruby'"

    .underline "text-decoration-line: underline; text-decoration-color: red"
    .gunderline "text-decoration-line: underline; text-decoration-color: green"
    .ounderline "text-decoration-line: underline; text-decoration-color: orangered"
    .ystrike "text-decoration-line: line-through; text-decoration-color: yellow"
    .mstrike "text-decoration-line: line-through; text-decoration-color: magenta"

    .vmargin30 "margin-top: 30px; margin-bottom: 30px"
    .hpadding20 "padding-left: 20px; padding-right: 20px"
    .height200 "height: 200px"
    .height400 "height: 400px"

    .center "text-align: center"
    .right "text-align: right"
}

check_test! {
    name = simple_vertical_lr,
    size = (100, 180),
    inline.noto_sans_jp.fs32.vertical_lr {
        span.blue_bg {
            text "ハロー"
            span.red_bg { text "ABC" }
            text "バイバイdef"
        }
    }
}

check_test! {
    name = simple_vertical_rl,
    size = (100, 180),
    inline.noto_sans_jp.fs32.vertical_rl {
        span.blue_bg {
            text "ハロー"
            span.red_bg { text "ABC" }
            text "バイバイdef"
        }
    }
}

check_test! {
    name = simple_sideways_rl,
    size = (100, 180),
    inline.noto_sans_jp.fs32.sideways_rl {
        span.blue_bg {
            text "ハロー"
            span.red_bg { text "ABC" }
            text "バイバイdef"
        }
    }
}

check_test! {
    name = inline_blocks,
    size = (750, 450),
    block.height400 {
        inline.noto_sans_jp.fs64 {
            block.green_bg.vertical_rl {
                inline {
                    span.blue_bg {
                        span.gunderline { text "ハロー" }
                        span.red_bg { text "ABC\n" }
                        span.ystrike { text "defバイバイ" }
                        ruby {
                            base { text "縦書き" }
                            annotation.fs32.annotation { text "たてがき" }
                        }
                        span { text "aaaaaa" }
                        text "\nあ"
                        block.vmargin30.hpadding20.red_bg.horizontal_tb { inline { text "abcd" } }
                        text "あ"
                    }
                }
            }
            text "more stuff"
        }
    }
}

check_test! {
    name = ruby_vertical_rl,
    size = (150, 180),
    inline.noto_sans_jp.fs32.vertical_rl {
        span.blue_bg {
            span.gunderline { text "ハロー" }
            span.red_bg { text "ABC\n" }
            span.ystrike { text "defバイバイ" }
            ruby.mstrike {
                base.ounderline  { text "縦書き" }
                annotation.fs16.annotation.ounderline  { text "たてがき" }
            }
            text "　"
            // FIXME: These decorations seem to have weird AA
            ruby.mstrike {
                base.ounderline  { text "上" }
                annotation.fs16.annotation.ounderline  { text "じょう" }
            }
        }
    }
}

check_test! {
    name = ruby_sideways_rl,
    size = (150, 180),
    inline.noto_sans_jp.fs32.sideways_rl {
        span.blue_bg {
            span.gunderline { text "ハロー" }
            span.red_bg { text "ABC\n" }
            span.ystrike { text "defバイバイ" }
            ruby {
                base { text "縦書き" }
                annotation.fs16.annotation { text "たてがき" }
            }
        }
    }
}

check_test! {
    name = tofu,
    size = (100, 200),
    block.fs32.vertical_rl {
        block.yellow_bg.noto_serif.sideways_rl {
            inline {
                text "hello "
                span.red_bg { text "世界" }
            }
        }
        block.cyan_bg.noto_sans_jp.vertical_rl {
            inline {
                span { text "ハロー " }
                span.noto_sans_jp.red_bg { text "❌❌" }
            }
        }
    }
}

check_test! {
    name = text_align,
    size = (96, 200),
    block.fs16.height200.ahem.vertical_lr {
        block.yellow_bg {
            inline {
                text "left"
            }
        }
        block.blue_bg.center {
            inline {
                text "center"
            }
        }
        block.red_bg.right {
            inline {
                text "right"
            }
        }
        block.cyan_bg.right.horizontal_tb {
            inline {
                text "rig t"
            }
        }
    }
}

check_test! {
    name = text_align2,
    size = (64, 200),
    block.fs16.height200.ahem.vertical_lr {
        block.horizontal_tb {
            block.blue_bg.center.vertical_lr {
                inline {
                    text "center"
                }
            }
        }
    }
}

test_define_style! {
    .width100 "width: 100px"
    .height100 "height: 100px"
    .vmargin0 "margin-top: 0px; margin-bottom: 0px"
    .vmarginauto "margin-top: auto; margin-bottom: auto"
    .tmarginauto "margin-top: auto"
}

check_test! {
    name = orthogonal_block_align,
    size = (100, 400),
    block.width100.height200.yellow_bg.vertical_rl.ahem {
        block.blue_bg { block.height400.vmargin0 { inline { text "01234567890123456789" } } }
        block.cyan_bg.height100.vmarginauto.center { inline { text "123" } }
        block.cyan_bg.height100.tmarginauto.right { inline { text "123" } }
    }
}
