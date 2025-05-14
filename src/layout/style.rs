use std::fmt::Debug;

use icu_segmenter::{LineBreakStrictness, LineBreakWordOption};

use crate::{
    color::BGRA8, math::I16Dot16, text::layout::TextWrapMode, util::SmallTypeMap,
    HorizontalAlignment, I26Dot6, TextShadow,
};

pub trait StyleValue: Debug + Clone + 'static {}

#[derive(Debug, Clone)]
pub struct StyleMap(SmallTypeMap<32>);

impl StyleMap {
    pub fn new() -> Self {
        Self(SmallTypeMap::new())
    }

    pub fn set<V: StyleValue>(&mut self, value: V) {
        self.0.set(value);
    }

    pub fn get<V: StyleValue>(&self) -> Option<&V> {
        self.0.get::<V>()
    }

    pub fn get_copy_or_default<V: Default + Copy + StyleValue>(&self) -> V {
        self.0.get_copy_or_default::<V>()
    }

    pub fn merge(&mut self, other: &Self) {
        self.0.merge(&other.0);
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

    pub fn get<V: StyleValue>(&self) -> Option<&V> {
        let mut current = self;
        loop {
            if let Some(value) = current.map.get::<V>() {
                return Some(value);
            }

            if let Some(prev) = current.prev {
                current = prev;
            } else {
                break;
            }
        }

        None
    }

    pub fn get_copy_or<V: Copy + StyleValue>(&self, default: V) -> V {
        match self.get::<V>() {
            Some(value) => *value,
            None => default,
        }
    }

    pub fn get_unwrap_copy_or<V: StyleWrapperValue>(&self, default: V::Inner) -> V::Inner
    where
        V::Inner: Copy,
    {
        match self.get::<V>() {
            Some(value) => *value.unwrap_ref(),
            None => default,
        }
    }

    pub fn get_copy_or_default<V: Default + Copy + StyleValue>(&self) -> V {
        match self.get::<V>() {
            Some(value) => *value,
            None => V::default(),
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

pub trait StyleWrapperValue: StyleValue {
    type Inner;
    fn unwrap(self) -> Self::Inner;
    fn unwrap_ref(&self) -> &Self::Inner;
}

macro_rules! make_wrappers {
    (
        $($(#[$attr: meta])* pub struct $name: ident(pub $inner: ty);)*
    ) => {
        $(
            #[derive(Debug, Clone)]
            $(#[$attr])*
            pub(crate) struct $name(pub $inner);
            impl StyleValue for $name {}
            impl StyleWrapperValue for $name {
                type Inner = $inner;
                fn unwrap(self) -> Self::Inner { self.0 }
                fn unwrap_ref(&self) -> &Self::Inner { &self.0 }
            }
        )*
    };
}

make_wrappers! {
    pub struct Color(pub BGRA8);
    pub struct BackgroundColor(pub BGRA8);

    pub struct FontFamily(pub Vec<Box<str>>);

    #[derive(Copy)]
    pub struct FontWeight(pub I16Dot16);

    pub struct FontSize(pub I26Dot6);

    #[derive(Copy)]
    pub struct TextAlign(pub HorizontalAlignment);

    pub struct TextShadows(pub Vec<TextShadow>);
}

#[derive(Debug, Clone, Copy)]
pub enum FontSlant {
    Regular,
    Italic,
}
impl StyleValue for FontSlant {}

// text-wrap-style in CSS land.
impl StyleValue for TextWrapMode {}
// line-break in CSS land.
impl StyleValue for LineBreakStrictness {}
// word-break in CSS land.
impl StyleValue for LineBreakWordOption {}
