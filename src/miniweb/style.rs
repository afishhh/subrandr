use std::{fmt::Debug, rc::Rc};

use icu_segmenter::{LineBreakStrictness, LineBreakWordOption};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    small_type_map::SmallTypeMap,
};

use crate::{miniweb::style::restyle::StylingContext, text::layout::TextWrapMode};

pub mod computed;
pub mod restyle;
pub mod sheet;
pub mod specified;

#[doc(hidden)]
pub trait StyleValue: 'static {
    type Inner: Debug + Clone + 'static;
}

#[repr(transparent)]
struct StyleSlot<V: StyleValue>(V::Inner);

impl<V: StyleValue> Clone for StyleSlot<V> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<V: StyleValue> Debug for StyleSlot<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <V::Inner as Debug>::fmt(&self.0, f)
    }
}

#[derive(Default, Debug, Clone)]
pub struct DeclarationMap(SmallTypeMap<32>);

impl DeclarationMap {
    pub fn new() -> Self {
        Self(SmallTypeMap::new())
    }

    pub fn set<V: StyleValue>(&mut self, value: V::Inner) {
        self.0.set::<StyleSlot<V>>(StyleSlot(value));
    }

    pub fn get<V: StyleValue>(&self) -> Option<&V::Inner> {
        self.0.get::<StyleSlot<V>>().map(|x| &x.0)
    }

    pub fn get_copy_or<V: StyleValue<Inner: Copy>>(&self, default: V::Inner) -> V::Inner {
        match self.get::<V>() {
            Some(value) => *value,
            None => default,
        }
    }

    fn merge(&mut self, other: &Self) {
        self.0.merge(&other.0);
    }
}

macro_rules! style_map {
    (@main $result: ident; $key: ident : $value: expr; $($rest: tt)*) => {
        $result.set::<$crate::miniweb::style::$key>($value);
        style_map!(@main $result; $($rest)*);
    };
    (@main $result: ident;) => {};
    (@main $result: ident; $($rest: tt)*) => {
        compile_error!("style_map! syntax error")
    };
    () => { $crate::miniweb::style::DeclarationMap::new() };
    ($($args: tt)*) => {{
        let mut result = $crate::miniweb::style::DeclarationMap::new();
        style_map!(@main result; $($args)*);
        result
    }};
}
pub(crate) use style_map;

use super::layout::FixedL;

// Generates style keys for all properties along with `ComputedStyle`.
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
        #[copy(no)] font_family: [Rc<str>] = Rc::new(["serif".into()]),
        font_weight: I16Dot16 = I16Dot16::new(400),
        font_size: I26Dot6 = I26Dot6::new(16),
        font_style: computed::FontSlant,

        // compute font_size {
        //     value.compute(parent.font_size())
        // };
    }

    rc text_inherited {
        #[copy(no)] text_shadows: [computed::TextShadow],
        text_wrap_style: TextWrapMode,
        line_break: LineBreakStrictness = LineBreakStrictness::Normal,
        word_break: LineBreakWordOption = LineBreakWordOption::Normal,
        text_align: computed::HorizontalAlignment = computed::HorizontalAlignment::Left,
    }

    rc uninherited {
        #[inherit(no)] display: computed::Display,
        #[inherit(no)] text_decoration: computed::TextDecorations,
    }

    rc misc {
        color: BGRA8 = BGRA8::WHITE,
        background_color: BGRA8 = BGRA8::ZERO,

        // compute background_color {
        //     in color;

        //     value.compute(color);
        // };

        position: computed::Position,
        // These should theoretically default to `auto` but we don't support that
        left: specified::LengthOrPercentage -> computed::PixelsOrPercentage
            = computed::PixelsOrPercentage::Pixels(computed::Pixels(FixedL::ZERO)),
        top: specified::LengthOrPercentage -> computed::PixelsOrPercentage
            = computed::PixelsOrPercentage::Pixels(computed::Pixels(FixedL::ZERO)),

        #[inherit(no)]
        sbr_simple_transform: computed::SbrSimpleTransform,
    }
}

#[cfg(test)]
mod test {
    use std::rc::Rc;

    use rasterize::color::BGRA8;
    use util::math::{I16Dot16, Vec2};

    use crate::{
        miniweb::style::{
            self,
            computed::{TextDecorations, TextShadow},
            restyle::StylingContext,
            ComputedStyle, DeclarationMap,
        },
        I26Dot6,
    };

    #[test]
    fn computed_style() {
        let ctx = StylingContext {
            time: 0,
            viewport_size: Vec2::ZERO,
        };
        let style = ComputedStyle::default();

        let child = style.create_child();

        assert!(child.text_shadows().is_empty());
        assert_eq!(child.text_decoration(), TextDecorations::default());

        let new_shadows = Rc::new([TextShadow {
            offset: Vec2::ZERO,
            blur_radius: I26Dot6::new(10),
            color: BGRA8::ORANGERED,
        }]);

        let mut child_of_child = child.create_child_with(&ctx, &{
            let mut map = DeclarationMap::new();
            map.set::<style::FontWeight>(I16Dot16::new(700));
            map.set::<style::TextShadows>(new_shadows.clone());
            map
        });

        *child_of_child.make_text_decoration_mut() = TextDecorations {
            underline: true,
            underline_color: BGRA8::RED,
            ..Default::default()
        };

        assert_eq!(child_of_child.font_weight(), I16Dot16::new(700));
        assert_eq!(child_of_child.text_shadows(), &*new_shadows);

        let another_child = child_of_child.create_child();

        assert_eq!(child_of_child.font_weight(), I16Dot16::new(700));
        assert_eq!(another_child.text_shadows(), &*new_shadows);
        assert_eq!(another_child.text_decoration(), TextDecorations::default());

        let more_child = another_child.create_child_with(&ctx, &{
            let mut map = DeclarationMap::new();
            map.set::<style::TextShadows>(new_shadows.clone());
            map
        });

        assert_eq!(
            more_child.text_shadows(),
            &new_shadows
                .iter()
                .chain(new_shadows.iter())
                .cloned()
                .collect::<Vec<_>>()
        );
    }
}
