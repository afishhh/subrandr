use macros::test_define_style;
use rasterize::color::BGRA8;

use crate::layout::{FixedL, Point2L, Vec2L};

pub use super::common::*;

#[test]
fn hello_world() {
    test_define_style! {
        .world {
            color: BGRA8::new(0, 0, 255, 255);
            background_color: BGRA8::new(255, 0, 0, 255);
        }
    }

    check_inline(
        "hello_world",
        Point2L::new(FixedL::ZERO, FixedL::ZERO),
        Vec2L::new(FixedL::new(192), FixedL::new(16)),
        crate::style::computed::HorizontalAlignment::Left,
        test_make_tree! {
            inline.ahem {
                text "Hello, "
                span.world {
                    text "横縦pÉ!"
                }
            }
        },
    )
}
