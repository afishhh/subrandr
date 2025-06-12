use std::fmt::Debug;

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    rc::Rc,
    rc_static,
};

pub mod computed;
use computed::*;

// Generates `ComputedStyle`.
//
// `ComputedStyle` is bascially a tree of `Rc`s, property access has to
// deref through all groups on the path while modification has to `make_mut`
// all of them. Immutable and mutable getters are automatically generated
// and the tree structure itself is entirely private.
//
// Also currently the macro only supports one layer but it's not like that's
// too difficult to change.
macros::implement_style_module! {
    rc font {
        #[copy(no)] font_family: [Rc<str>] = rc_static!([rc_static!(str b"serif")]),
        font_weight: I16Dot16 = I16Dot16::new(400),
        font_size: I26Dot6 = I26Dot6::new(16),
        font_slant: FontSlant = FontSlant::Regular,
    }

    rc text_inherited {
        #[copy(no)] text_shadows: [TextShadow] = rc_static!([]),
        line_break: LineBreakStrictness = LineBreakStrictness::Normal,
        word_break: LineBreakWordOption = LineBreakWordOption::Normal,
        text_align: HorizontalAlignment = HorizontalAlignment::Left,
    }

    rc uninherited {
        #[inherit(no)] background_color: BGRA8 = BGRA8::ZERO,
        #[inherit(no)] text_decoration: TextDecorations = TextDecorations::NONE,
    }

    rc misc {
        color: BGRA8 = BGRA8::WHITE,
    }
}
