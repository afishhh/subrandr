use std::{fmt::Debug, hash::Hash, ops::RangeInclusive, path::Path};

use rasterize::scene::{FixedS, Scene, SubsceneKind, Vec2S};
use text_sys::hb_font_t;
use util::{
    cache::CacheValue,
    math::{I16Dot16, I26Dot6},
};

use super::FreeTypeError;
use crate::text::{FontSizeCacheKey, GlyphCache, OpenTypeTag};

pub mod freetype;
pub use freetype::GlyphDisplayError;
mod tofu;

#[derive(Debug, Clone, Copy)]
pub struct Axis {
    pub tag: OpenTypeTag,
    pub index: usize,
    pub minimum: I16Dot16,
    pub maximum: I16Dot16,
}

impl Axis {
    #[inline(always)]
    pub fn range(&self) -> RangeInclusive<I16Dot16> {
        self.minimum..=self.maximum
    }

    #[inline(always)]
    pub fn is_value_in_range(&self, value: I16Dot16) -> bool {
        self.range().contains(&value)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphMetrics {
    pub width: I26Dot6,
    pub height: I26Dot6,
    pub hori_bearing_x: I26Dot6,
    pub hori_bearing_y: I26Dot6,
    pub hori_advance: I26Dot6,
    #[expect(dead_code)]
    pub vert_bearing_x: I26Dot6,
    #[expect(dead_code)]
    pub vert_bearing_y: I26Dot6,
    #[expect(dead_code)]
    pub vert_advance: I26Dot6,
}

#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    pub ascender: I26Dot6,
    pub descender: I26Dot6,
    pub height: I26Dot6,
    #[expect(dead_code)]
    pub max_advance: I26Dot6,

    pub underline_top_offset: I26Dot6,
    pub underline_thickness: I26Dot6,
    pub strikeout_top_offset: I26Dot6,
    pub strikeout_thickness: I26Dot6,
}

impl FontMetrics {
    pub const ZERO: Self = Self {
        ascender: I26Dot6::ZERO,
        descender: I26Dot6::ZERO,
        height: I26Dot6::ZERO,
        max_advance: I26Dot6::ZERO,
        underline_top_offset: I26Dot6::ZERO,
        underline_thickness: I26Dot6::ZERO,
        strikeout_top_offset: I26Dot6::ZERO,
        strikeout_thickness: I26Dot6::ZERO,
    };

    pub fn line_gap(&self) -> I26Dot6 {
        self.height - self.ascender + self.descender
    }
}

trait FaceImpl: Sized {
    type Font: FontImpl<Face = Self>;

    fn family_name(&self) -> &str;
    fn addr(&self) -> usize;

    fn axes(&self) -> &[Axis];
    fn axis(&self, tag: OpenTypeTag) -> Option<Axis> {
        self.axes().iter().find(|x| x.tag == tag).copied()
    }
    fn set_axis(&mut self, index: usize, value: I16Dot16);

    fn weight(&self) -> I16Dot16;
    fn italic(&self) -> bool;

    fn contains_codepoint(&self, codepoint: u32) -> bool;

    type Error;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Self::Font, Self::Error>;
}

#[derive(Clone)]
pub struct GlyphSubscene(pub SubsceneKind);

impl GlyphSubscene {
    fn empty() -> Self {
        Self(SubsceneKind::Scene(Scene::empty()))
    }
}

trait FontImpl: Sized {
    type Face;
    fn face(&self) -> &Self::Face;

    fn metrics(&self) -> &FontMetrics;
    fn point_size(&self) -> I26Dot6;
    // Used to fix HarfBuzz metrics for scaled bitmap fonts which HarfBuzz sees in
    // their unscaled form. It would be ideal to instead handle this in
    // the font funcs but that's non-trivial so this works.
    fn harfbuzz_scale_factor_for(&self, glyph: u32) -> I26Dot6;

    fn size_cache_key(&self) -> FontSizeCacheKey;

    type DisplayError;
    fn glyph_subscene_uncached(
        &self,
        index: u32,
        subpixel_offset: Vec2S,
    ) -> Result<GlyphSubscene, Self::DisplayError>;
}

macro_rules! forward_methods {
    (
        variants = $enum: ident :: $variants: tt;
        $($vis: vis fn $name: ident $selfref: tt $params: tt -> $ret: ty;)*
    ) => {
        $(forward_methods!(@once $enum $variants $vis $name $selfref $params $params $ret);)*
    };
    (@once $enum: ident [$($variant: ident),*] $vis: vis $name: ident [$($selfref: tt)*] $params: tt ($($params_unwrapped: tt)*) $ret: ty) => {
        #[allow(dead_code)]
        $vis fn $name($($selfref)* self, $($params_unwrapped)*) -> $ret {
            match self {
                $($enum :: $variant(value) => forward_methods!(@build_call {value.$name} $params),)*
            }
        }
    };
    (@build_call $pre: tt  ($($params: tt)*)) => {
        forward_methods!(@build_args_rec $pre [] $($params)*)
    };
    (@build_args_rec $pre: tt [] $name: tt : $type: ty $(, $($rest: tt)*)?) => {
        forward_methods!(@build_args_rec $pre [$name] $($($rest)*)?)
    };
    (@build_args_rec $pre: tt [$($result: tt)*] $name: tt : $type: ty $(, $($rest: tt)*)?) => {
        forward_methods!(@build_args_rec $pre [$($result)*, $name] $($($rest)*)?)
    };
    (@build_args_rec {$($pre: tt)*} [$($result: tt)*]) => {
        $($pre)* ($($result)*)
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Face {
    FreeType(freetype::Face),
    Tofu(tofu::Face),
}

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>, index: i32) -> Result<Self, FreeTypeError> {
        freetype::Face::load_from_file(path, index).map(Face::FreeType)
    }

    #[cfg_attr(
        not(any(target_arch = "wasm32", font_provider = "directwrite")),
        expect(dead_code)
    )]
    pub fn load_from_bytes(bytes: std::sync::Arc<[u8]>, index: i32) -> Result<Self, FreeTypeError> {
        freetype::Face::load_from_bytes(bytes, index).map(Face::FreeType)
    }

    #[cfg(all(test, feature = "_layout_tests"))]
    pub fn load_from_static_bytes(bytes: &'static [u8], index: i32) -> Result<Self, FreeTypeError> {
        freetype::Face::load_from_static_bytes(bytes, index).map(Face::FreeType)
    }

    pub const fn tofu() -> Self {
        Face::Tofu(tofu::Face)
    }

    forward_methods!(
        variants = Face::[FreeType, Tofu];
        pub fn family_name[&]() -> &str;

        pub fn axes[&]() -> &[Axis];
        pub fn axis[&](tag: OpenTypeTag) -> Option<Axis>;
        pub fn set_axis[&mut](index: usize, value: I16Dot16) -> ();

        pub fn weight[&]() -> I16Dot16;
        pub fn italic[&]() -> bool;

        pub fn contains_codepoint[&](codepoint: u32) -> bool;
    );

    pub fn addr(&self) -> usize {
        match self {
            Face::FreeType(face) => face.addr(),
            Face::Tofu(face) => face.addr(),
        }
    }

    pub fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Font, FreeTypeError> {
        match &self {
            Face::FreeType(face) => face.with_size(point_size, dpi).map(Font::FreeType),
            Face::Tofu(face) => match face.with_size(point_size, dpi) {
                Ok(font) => Ok(Font::Tofu(font)),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Font {
    FreeType(freetype::Font),
    Tofu(tofu::Font),
}

impl Font {
    forward_methods!(
        variants = Font::[FreeType, Tofu];

        fn size_cache_key[&]() -> FontSizeCacheKey;
        pub fn metrics[&]() -> &FontMetrics;
        pub fn point_size[&]() -> I26Dot6;
        pub fn harfbuzz_scale_factor_for[&](glyph: u32) -> I26Dot6;
    );

    pub fn face(&self) -> Face {
        match self {
            Font::FreeType(font) => Face::FreeType(font.face().clone()),
            Font::Tofu(_) => Face::tofu(),
        }
    }

    pub fn as_harfbuzz_font(&self) -> Result<*mut hb_font_t, FreeTypeError> {
        match self {
            Self::FreeType(font) => Ok(font.with_applied_size_and_hb()?.1),
            Self::Tofu(font) => Ok(font.as_harfbuzz_font()),
        }
    }

    pub fn glyph_subscene<'c>(
        &self,
        cache: &'c GlyphCache,
        glyph: u32,
        offset_value: FixedS,
        offset_axis_is_y: bool,
    ) -> Result<&'c GlyphSubscene, GlyphDisplayError> {
        let (render_offset, subpixel_bucket) =
            FontSizeCacheKey::get_subpixel_bucket(offset_value, offset_axis_is_y);
        let key = self
            .size_cache_key()
            .for_glyph(self.face(), glyph, subpixel_bucket);

        cache.get_or_try_insert_with(key, || match self {
            Self::FreeType(font) => font.glyph_subscene_uncached(glyph, render_offset),
            Self::Tofu(font) => Ok(font.glyph_subscene_uncached(glyph, render_offset).unwrap()),
        })
    }
}

impl CacheValue for GlyphMetrics {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of_val(self)
    }
}

impl CacheValue for GlyphSubscene {
    fn memory_footprint(&self) -> usize {
        std::mem::size_of_val(self)
            + match &self.0 {
                SubsceneKind::External(external) => std::mem::size_of_val(&**external),
                SubsceneKind::Scene(scene) => std::mem::size_of_val(&scene.memory_footprint()),
            }
    }
}
