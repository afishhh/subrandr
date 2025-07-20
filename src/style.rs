use std::fmt::Debug;

use icu_segmenter::options::{LineBreakStrictness, LineBreakWordOption};
use rasterize::color::BGRA8;
use util::{
    math::{I16Dot16, I26Dot6},
    small_type_map::SmallTypeMap,
};

use crate::text::layout::TextWrapMode;

pub mod types;

#[doc(hidden)]
pub trait StyleValue: 'static {
    type Inner: Debug + Clone + 'static;
    type Inherited<'a>;

    fn retrieve_inherited<'a>(map: &CascadingStyleMap<'a>) -> Self::Inherited<'a>;
}

fn retrieve_inherited_default<'a, V: StyleValue>(
    map: &CascadingStyleMap<'a>,
) -> Option<&'a V::Inner> {
    for map in map.iter_chain() {
        if let Some(value) = map.get::<V>() {
            return Some(value);
        }
    }

    None
}

fn retrieve_inherited_list<T: Clone, V: StyleValue<Inner = Vec<T>>>(
    map: &CascadingStyleMap,
) -> Vec<T> {
    let mut result = Vec::new();

    for map in map.iter_chain() {
        if let Some(value) = map.get::<V>() {
            result.extend(value.iter().rev().cloned());
        }
    }

    result
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

pub struct CascadingStyleMap<'a> {
    prev: Option<&'a CascadingStyleMap<'a>>,
    map: &'a StyleMap,
}

impl<'a> CascadingStyleMap<'a> {
    pub fn new(map: &'a StyleMap) -> Self {
        Self { prev: None, map }
    }

    fn iter_chain(&self) -> IterChain<'_, 'a> {
        IterChain {
            current: Some(self),
        }
    }

    pub fn get<V: StyleValue>(&self) -> V::Inherited<'a> {
        V::retrieve_inherited(self)
    }

    pub fn get_copy_or<V: StyleValue<Inherited<'a> = Option<&'a T>>, T: Copy + 'static>(
        &self,
        default: T,
    ) -> T {
        match self.get::<V>() {
            Some(value) => *value,
            None => default,
        }
    }

    pub fn get_copy_or_default<
        V: StyleValue<Inherited<'a> = Option<&'a T>>,
        T: Default + Copy + 'static,
    >(
        &self,
    ) -> T {
        match self.get::<V>() {
            Some(value) => *value,
            None => T::default(),
        }
    }

    #[must_use]
    pub fn push<'b: 'a>(&'b self, next: &'b StyleMap) -> CascadingStyleMap<'b> {
        Self {
            prev: Some(self),
            map: next,
        }
    }
}

struct IterChain<'i, 'a> {
    current: Option<&'i CascadingStyleMap<'a>>,
}

impl<'a> Iterator for IterChain<'_, 'a> {
    type Item = &'a StyleMap;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        self.current = current.prev;
        Some(current.map)
    }
}

macro_rules! make_keys {
    (
        $($(#[$($attrs: tt)*])*
        pub struct $name: ident: $inner: ty;)*
    ) => {
        $(
            make_keys!(@build_struct {$name $inner}; {}; $([$($attrs)*])*);

            #[automatically_derived]
            impl StyleValue for $name {
                type Inner = $inner;

                make_keys!(@build_impl {$name $inner}; {(inherit)}; $([$($attrs)*])*);
            }
        )*
    };
    (@build_struct {$name: ident $inner: ty}; { $($processed: tt)* };) => {
        #[derive(Debug, Clone)]
        $($processed)*
        // FIXME: I think I actually managed to trigger a `dead_code` false positive with this
        //        trait :)
        pub(crate) struct $name(#[allow(dead_code)] $inner);
    };
    (@build_struct $pt: tt; { $($processed: tt)* }; [inherit$($args: tt)*] $($rest: tt)*) => {
        make_keys!(@build_struct $pt; { $($processed)* }; $($rest)*);
    };
    (@build_struct $pt: tt; { $($processed: tt)* }; [copy] $($rest: tt)*) => {
        make_keys!(@build_struct $pt; { $($processed)* #[derive(Copy) ]}; $($rest)*);
    };
    (@build_struct { $($processed: tt)* }; $($value: tt)*) => { #[$($value)*] };

    (@build_impl $pt: tt; {$seen_inherit: tt};) => { make_keys!(@default_inherit $seen_inherit); };
    (@default_inherit ()) => { };
    (@default_inherit (inherit)) => { make_keys!(@inherit (default)); };

    (@build_impl $pt: tt; {$seen_ignore: tt $($seen_rest: tt)*}; [inherit$($args: tt)*] $($rest: tt)*) => {
        make_keys!(@inherit $($args)*);
        make_keys!(@build_impl $pt; {() $($seen_rest)*}; $($rest)*);
    };
    (@build_impl $pt: tt; $seen: tt; [copy$($args: tt)*] $($rest: tt)*) => {
        make_keys!(@build_impl $pt; $seen; $($rest)*);
    };

    (@inherit (default)) => {
        type Inherited<'a> = Option<&'a Self::Inner>;

        fn retrieve_inherited<'a>(map: &CascadingStyleMap<'a>) -> Self::Inherited<'a> {
            retrieve_inherited_default::<Self>(map)
        }
    };
    (@inherit (list)) => {
        type Inherited<'a> = Self::Inner;

        fn retrieve_inherited<'a>(map: &CascadingStyleMap<'a>) -> Self::Inherited<'a> {
            retrieve_inherited_list::<_, Self>(map)
        }
    };
    (@inherit (no)) => {
        type Inherited<'a> = Option<&'a Self::Inner>;

        fn retrieve_inherited<'a>(map: &CascadingStyleMap<'a>) -> Self::Inherited<'a> {
            map.map.get::<Self>()
        }
    };
}

make_keys! {
    #[copy] pub struct Color: BGRA8;
    #[copy] pub struct BackgroundColor: BGRA8;

    #[inherit(list)]
    pub struct FontFamily: Vec<Box<str>>;
    #[copy] pub struct FontWeight: I16Dot16;
    #[copy] pub struct FontSize: I26Dot6;
    #[copy] pub struct FontStyle: types::FontSlant;

    #[copy] pub struct TextAlign: types::HorizontalAlignment;
    #[inherit(list)]
    pub struct TextShadows: Vec<types::TextShadow>;
    #[inherit(no)]
    pub struct TextDecoration: types::TextDecorations;

    #[copy] pub struct TextWrapStyle: TextWrapMode;
    #[copy] pub struct LineBreak: LineBreakStrictness;
    #[copy] pub struct WordBreak: LineBreakWordOption;
}
