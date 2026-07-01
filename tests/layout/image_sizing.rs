use rasterize::color::BGRA8;
use util::math::Vec2;

use super::common::*;
use crate::layout::FixedL;

test_define_style! {
    .stretch "-sbr-inline-sizing: stretch"

    .lpadding10 "padding-left: 10px"
    .tpadding10 "padding-top: 10px"
    .lmarginauto "margin-left: auto"
    .lmargin10 "margin-left: 10px"
    .rmarginauto "margin-right: auto"
    .rmargin10 "margin-right: 10px"
}

// TODO: test with width and height set

check_test! {
    name = inline,
    size = (62, 33 + 16),
    inline.ahem {
        span.stretch.green_bg {
            text "A"
            image.lpadding10.tpadding10.orangered_bg {
                natural_size = Vec2::new(FixedL::new(20), FixedL::new(20)),
                color = BGRA8::RED
            }
            text "A\n"
        }
        span.stretch.yellow_bg {
            text "B"
            image.lmargin10.rmarginauto {
                natural_size = Vec2::new(FixedL::new(20), FixedL::new(10)),
                color = BGRA8::GREEN
            }
            text "B"
        }
    }
}

check_test! {
    name = block,
    size = (160, 90),
    block.ahem.red_bg {
        image.lmarginauto.rmarginauto.lpadding10 {
            natural_size = Vec2::new(FixedL::new(20), FixedL::new(40)),
            color = BGRA8::GREEN
        }

        image.tpadding10.lmarginauto.rmargin10 {
            natural_size = Vec2::new(FixedL::new(20), FixedL::new(20)),
            color = BGRA8::MAGENTA
        }

        image {
            natural_size = Vec2::new(FixedL::new(20), FixedL::new(20)),
            color = BGRA8::BLUE
        }
    }
}
