use rasterize::color::BGRA8;
use util::{
    math::{I26Dot6, Vec2},
    rc_static,
};

use super::common::*;
use crate::{
    layout::FixedL,
    style::computed::{Length, TextShadow},
};

test_define_style! {
    .green_shadow {
        text_shadows: rc_static![[TextShadow {
            offset: Vec2::splat(Length::from_pixels(FixedL::new(5))),
            blur_radius: Length::ZERO,
            color: BGRA8::GREEN,
        }]],
    }
    .green_shadow_blurred {
        text_shadows: rc_static![[TextShadow {
            offset: Vec2::splat(Length::from_pixels(FixedL::new(5))),
            blur_radius: Length::from_pixels(FixedL::new(3)),
            color: BGRA8::GREEN,
        }]],
    }
    .blue_shadow_blurred {
        text_shadows: rc_static![[TextShadow {
            offset: Vec2::splat(Length::from_pixels(FixedL::new(5))),
            blur_radius: Length::from_pixels(FixedL::new(3)),
            color: BGRA8::BLUE,
        }]],
    }
    .many_shadows {
        text_shadows: rc_static![[
            TextShadow {
                offset: Vec2::splat(Length::from_pixels(FixedL::new(3))),
                blur_radius: Length::ZERO,
                color: BGRA8::RED,
            },
            TextShadow {
                offset: Vec2::splat(Length::from_pixels(FixedL::new(5))),
                blur_radius: Length::from_pixels(FixedL::new(3)),
                color: BGRA8::GREEN,
            },
            TextShadow {
                offset: Vec2::splat(Length::from_pixels(FixedL::new(7))),
                blur_radius: Length::ZERO,
                color: BGRA8::BLUE,
            },
        ]],
    }
    .very_large { font_size: I26Dot6::new(64) }
    .red_shadow_very_blurred {
        text_shadows: rc_static![[TextShadow {
            offset: Vec2::splat(Length::from_pixels(FixedL::new(5))),
            blur_radius: Length::from_pixels(FixedL::new(8)),
            color: BGRA8::RED,
        }]],
    }
}

check_test! {
    name = simple,
    size = (140, 30),
    inline.noto_serif {
        span.green_shadow {
            text "hello world"
        }
    }
}

check_test! {
    name = blurred_line_broken,
    size = (60, 50),
    inline.noto_serif {
        span.green_shadow_blurred {
            text "hello world"
        }
    }
}

// TODO: Is this correct? Since we use the broken gamma-encoded blending it's hard to tell...
check_test! {
    name = many,
    size = (60, 50),
    inline.noto_serif {
        span.many_shadows {
            text "hello world"
        }
    }
}

check_test! {
    name = emoji,
    size = (90, 32),
    inline.noto_serif.green_shadow {
        span.noto_color_emoji {
            text "😀🧱"
        }
        span.noto_color_emoji.blue_shadow_blurred {
            text "😭⭕️"
        }
    }
}

check_test! {
    name = large,
    size = (155, 105),
    inline.noto_sans_jp.very_large.red_shadow_very_blurred {
        span.noto_color_emoji {
            text "⭕️"
        }
        text "赤"
    }
}
