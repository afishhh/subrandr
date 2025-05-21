use std::{fmt::Debug, rc::Rc};

use icu_segmenter::{LineBreakStrictness, LineBreakWordOption};

use crate::{
    color::BGRA8,
    math::{I16Dot16, I26Dot6},
    text::layout::TextWrapMode,
    util::SmallTypeMap,
};

pub mod types;

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
pub struct StyleMap(SmallTypeMap<32>);

impl StyleMap {
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
}

// Generates style keys for all properties along with `ComputedStyle`.
//
// `ComputedStyle` is bascially a tree of `Rc`s, property access has to
// deref through all groups on the path while modification has to `make_mut`
// all of them. Immutable and mutable getters are automatically generated
// and the tree structure itself is entirely private.
//
// Also currently the macro only supports one layer but it's not like that's
// too difficult to change.
subrandr_macros::implement_style_module! {
    rc font {
        #[copy(no)] font_family: [Rc<str>] = Rc::new(["serif".into()]),
        font_weight: I16Dot16 = I16Dot16::new(400),
        // TODO: This would ideally scale with DPI but we can also just
        //       not let it fall back to this value :)
        //       Actually the correct solution would probably be to make scaling by
        //       the context pixel scale happen during layout.
        font_size: I26Dot6 = I26Dot6::new(16),
        font_style: types::FontSlant,
    }

    rc text_inherited {
        #[copy(no)] text_shadows: [types::TextShadow],
        text_wrap_style: TextWrapMode,
        line_break: LineBreakStrictness = LineBreakStrictness::Normal,
        word_break: LineBreakWordOption = LineBreakWordOption::Normal,
        text_align: types::HorizontalAlignment = types::HorizontalAlignment::Left,
    }

    rc uninherited {
        #[inherit(no)] display: types::Display,
        #[inherit(no)] text_decoration: types::TextDecorations,
    }

    rc misc {
        color: BGRA8 = BGRA8::WHITE,
        background_color: BGRA8 = BGRA8::ZERO,
    }
}

#[cfg(test)]
mod test {
    use std::rc::Rc;

    use crate::{
        color::BGRA8,
        math::{I16Dot16, Vec2},
        miniweb::style::{
            self,
            types::{TextDecorations, TextShadow},
            ComputedStyle, StyleMap,
        },
        I26Dot6,
    };

    #[test]
    fn computed_style() {
        let style = ComputedStyle::default();

        let mut child = style.create_child();

        assert!(child.text_shadows().is_empty());
        assert_eq!(child.text_decoration(), TextDecorations::default());

        let new_shadows = Rc::new([TextShadow {
            offset: Vec2::ZERO,
            blur_radius: I26Dot6::new(10),
            color: BGRA8::ORANGERED,
        }]);
        child.apply_all(&{
            let mut map = StyleMap::new();
            map.set::<style::FontWeight>(I16Dot16::new(700));
            map.set::<style::TextShadows>(new_shadows.clone());
            map
        });
        *child.make_text_decoration_mut() = TextDecorations {
            underline: true,
            underline_color: BGRA8::RED,
            ..Default::default()
        };

        assert_eq!(child.font_weight(), I16Dot16::new(700));
        assert_eq!(child.text_shadows(), &*new_shadows);

        let mut child_of_child = child.create_child();

        assert_eq!(child.font_weight(), I16Dot16::new(700));
        assert_eq!(child_of_child.text_shadows(), &*new_shadows);
        assert_eq!(child_of_child.text_decoration(), TextDecorations::default());

        child_of_child.apply_all(&{
            let mut map = StyleMap::new();
            map.set::<style::TextShadows>(new_shadows.clone());
            map
        });

        assert_eq!(
            child_of_child.text_shadows(),
            &new_shadows
                .iter()
                .chain(new_shadows.iter())
                .cloned()
                .collect::<Vec<_>>()
        );
    }
}
