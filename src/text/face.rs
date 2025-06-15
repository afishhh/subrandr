use std::{
    cell::{Cell, UnsafeCell},
    collections::HashMap,
    fmt::{Debug, Display},
    hash::Hash,
    ops::RangeInclusive,
    path::Path,
    sync::Arc,
};

use once_cell::unsync::OnceCell;
use rasterize::{Rasterizer, Texture};
use text_sys::hb_font_t;
use util::math::{I16Dot16, I26Dot6, Vec2};

use super::FreeTypeError;

mod freetype;
pub use freetype::GlyphRenderError;
mod tofu;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct OpenTypeTag(u32);

impl OpenTypeTag {
    pub const fn from_bytes(text: [u8; 4]) -> Self {
        Self(
            ((text[0] as u32) << 24)
                + ((text[1] as u32) << 16)
                + ((text[2] as u32) << 8)
                + (text[3] as u32),
        )
    }

    pub const fn to_bytes(self) -> [u8; 4] {
        self.0.to_be_bytes()
    }

    pub fn to_bytes_in(self, buf: &mut [u8; 4]) -> &[u8] {
        *buf = self.to_bytes();
        let offset = buf.iter().position(|b| *b != b'0').unwrap_or(buf.len());
        &buf[offset..]
    }
}

impl Display for OpenTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0; 4];
        let bytes = self.to_bytes_in(&mut buf);
        if let Ok(string) = std::str::from_utf8(bytes) {
            write!(f, "{string}")
        } else {
            write!(f, "{}", bytes.escape_ascii())
        }
    }
}

impl Debug for OpenTypeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut buf = [0; 4];
        let bytes = self.to_bytes_in(&mut buf);
        if let Ok(string) = std::str::from_utf8(bytes) {
            write!(f, "{string:?}")
        } else {
            write!(f, "{bytes:?}")
        }
    }
}

pub const WEIGHT_AXIS: OpenTypeTag = OpenTypeTag::from_bytes(*b"wght");
#[expect(dead_code)]
pub const WIDTH_AXIS: OpenTypeTag = OpenTypeTag::from_bytes(*b"wdth");
pub const ITALIC_AXIS: OpenTypeTag = OpenTypeTag::from_bytes(*b"ital");

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
#[expect(dead_code)]
pub struct GlyphMetrics {
    pub width: I26Dot6,
    pub height: I26Dot6,
    pub hori_bearing_x: I26Dot6,
    pub hori_bearing_y: I26Dot6,
    pub hori_advance: I26Dot6,
    pub vert_bearing_x: I26Dot6,
    pub vert_bearing_y: I26Dot6,
    pub vert_advance: I26Dot6,
}

#[derive(Debug, Clone, Copy)]
#[expect(dead_code)]
pub struct FontMetrics {
    pub ascender: I26Dot6,
    pub descender: I26Dot6,
    pub height: I26Dot6,
    pub max_advance: I26Dot6,

    pub underline_top_offset: I26Dot6,
    pub underline_thickness: I26Dot6,
    pub strikeout_top_offset: I26Dot6,
    pub strikeout_thickness: I26Dot6,
}

impl FontMetrics {
    pub fn line_gap(&self) -> I26Dot6 {
        self.height - self.ascender + self.descender
    }
}

trait FaceImpl: Sized {
    type Font: FontImpl<Face = Self>;

    fn family_name(&self) -> &str;

    fn axes(&self) -> &[Axis];
    fn axis(&self, tag: OpenTypeTag) -> Option<Axis> {
        self.axes().iter().find(|x| x.tag == tag).copied()
    }
    fn set_axis(&mut self, index: usize, value: I16Dot16);

    fn weight(&self) -> I16Dot16;
    fn italic(&self) -> bool;

    type Error;
    fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Self::Font, Self::Error>;
}

trait FontImpl: Sized {
    type Face;
    fn face(&self) -> &Self::Face;

    fn metrics(&self) -> &FontMetrics;
    fn point_size(&self) -> I26Dot6;

    type FontSizeKey: Debug + Eq + Hash;
    fn font_size_key(&self) -> Self::FontSizeKey;

    fn glyph_cache(&self) -> &GlyphCache<Self>;

    type MeasureError;
    fn measure_glyph_uncached(&self, index: u32) -> Result<GlyphMetrics, Self::MeasureError>;

    type RenderError;
    fn render_glyph_uncached(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset: Vec2<I26Dot6>,
    ) -> Result<SingleGlyphBitmap, Self::RenderError>;
}

struct CacheSlot {
    generation: u64,
    metrics: OnceCell<GlyphMetrics>,
    bitmap: [OnceCell<Box<SingleGlyphBitmap>>; 8],
}

impl CacheSlot {
    fn new() -> Self {
        Self {
            generation: 0,
            metrics: OnceCell::new(),
            bitmap: [const { OnceCell::new() }; 8],
        }
    }
}

struct GlyphCache<F: FontImpl> {
    generation: Cell<u64>,
    glyphs: UnsafeCell<HashMap<(u32, F::FontSizeKey), CacheSlot>>,
}

impl<F: FontImpl> GlyphCache<F> {
    fn new() -> Self {
        Self {
            generation: Cell::new(0),
            glyphs: UnsafeCell::new(HashMap::new()),
        }
    }

    pub fn advance_generation(&self) {
        let glyphs = unsafe { &mut *self.glyphs.get() };

        let keep_after = self.generation.get().saturating_sub(2);
        // TODO: A scan-resistant LRU?
        if glyphs.len() > 200 {
            glyphs.retain(|_, slot| slot.generation > keep_after);
        }
        self.generation.set(self.generation.get() + 1);
    }

    #[allow(clippy::mut_from_ref)] // This is why it's unsafe
    unsafe fn slot(&self, font: &F, index: u32) -> &mut CacheSlot {
        let glyphs = unsafe { &mut *self.glyphs.get() };
        let size_info = font.font_size_key();

        let slot = glyphs
            .entry((index, size_info))
            .or_insert_with(CacheSlot::new);
        slot.generation = self.generation.get();
        slot
    }

    pub fn get_or_try_measure(
        &self,
        font: &F,
        index: u32,
    ) -> Result<&GlyphMetrics, F::MeasureError> {
        unsafe { self.slot(font, index) }
            .metrics
            .get_or_try_init(|| font.measure_glyph_uncached(index))
    }

    pub fn get_or_try_render(
        &self,
        rasterizer: &mut dyn Rasterizer,
        font: &F,
        index: u32,
        offset_value: I26Dot6,
        offset_axis_is_y: bool,
    ) -> Result<&SingleGlyphBitmap, F::RenderError> {
        let offset_trunc = I26Dot6::from_raw(offset_value.into_raw() & 0b110000);
        let bucket_idx = (offset_trunc.into_raw() >> 3) as usize | offset_axis_is_y as usize;
        let render_offset = if offset_axis_is_y {
            Vec2::new(I26Dot6::ZERO, offset_trunc)
        } else {
            Vec2::new(offset_trunc, I26Dot6::ZERO)
        };

        unsafe { self.slot(font, index) }.bitmap[bucket_idx]
            .get_or_try_init(|| {
                font.render_glyph_uncached(rasterizer, index, render_offset)
                    .map(Box::new)
            })
            .map(Box::as_ref)
    }
}

#[derive(Clone)]
pub struct SingleGlyphBitmap {
    pub offset: Vec2<I26Dot6>,
    pub texture: Texture,
}

macro_rules! forward_methods {
    (
        variants = $enum: ident :: $variants: tt;
        $(pub fn $name: ident $selfref: tt $params: tt -> $ret: ty;)*
    ) => {
        $(forward_methods!(@once $enum $variants $name $selfref $params $params $ret);)*
    };
    (@once $enum: ident [$($variant: ident),*] $name: ident [$($selfref: tt)*] $params: tt ($($params_unwrapped: tt)*) $ret: ty) => {
        #[allow(dead_code)]
        pub fn $name($($selfref)* self, $($params_unwrapped)*) -> $ret {
            match $($selfref)* self.0 {
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
enum FaceRepr {
    FreeType(freetype::Face),
    Tofu(tofu::Face),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Face(FaceRepr);

impl Face {
    pub fn load_from_file(path: impl AsRef<Path>, index: i32) -> Result<Self, FreeTypeError> {
        freetype::Face::load_from_file(path, index)
            .map(FaceRepr::FreeType)
            .map(Face)
    }

    pub fn load_from_bytes(bytes: Arc<[u8]>, index: i32) -> Result<Self, FreeTypeError> {
        freetype::Face::load_from_bytes(bytes, index)
            .map(FaceRepr::FreeType)
            .map(Face)
    }

    pub const fn tofu() -> Self {
        Face(FaceRepr::Tofu(tofu::Face))
    }

    forward_methods!(
        variants = FaceRepr::[FreeType, Tofu];
        pub fn family_name[&]() -> &str;

        pub fn axes[&]() -> &[Axis];
        pub fn axis[&](tag: OpenTypeTag) -> Option<Axis>;
        pub fn set_axis[&mut](index: usize, value: I16Dot16) -> ();

        pub fn weight[&]() -> I16Dot16;
        pub fn italic[&]() -> bool;
    );

    pub fn with_size(&self, point_size: I26Dot6, dpi: u32) -> Result<Font, FreeTypeError> {
        match &self.0 {
            FaceRepr::FreeType(face) => face
                .with_size(point_size, dpi)
                .map(FontRepr::FreeType)
                .map(Font),
            FaceRepr::Tofu(face) => match face.with_size(point_size, dpi) {
                Ok(font) => Ok(Font(FontRepr::Tofu(font))),
            },
        }
    }

    pub fn advance_cache_generation(&self) {
        match &self.0 {
            FaceRepr::FreeType(face) => face.glyph_cache().advance_generation(),
            FaceRepr::Tofu(_) => (),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum FontRepr {
    FreeType(freetype::Font),
    Tofu(tofu::Font),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Font(FontRepr);

impl Font {
    forward_methods!(
        variants = FontRepr::[FreeType, Tofu];

        pub fn metrics[&]() -> &FontMetrics;
        pub fn point_size[&]() -> I26Dot6;
    );

    pub fn glyph_extents(&self, index: u32) -> Result<&GlyphMetrics, FreeTypeError> {
        match &self.0 {
            FontRepr::FreeType(font) => font.glyph_cache().get_or_try_measure(font, index),
            FontRepr::Tofu(font) => Ok(font.glyph_metrics()),
        }
    }

    pub fn as_harfbuzz_font(&self) -> Result<*mut hb_font_t, FreeTypeError> {
        match &self.0 {
            FontRepr::FreeType(font) => Ok(font.with_applied_size_and_hb()?.1),
            FontRepr::Tofu(font) => Ok(font.as_harfbuzz_font()),
        }
    }

    pub fn render_glyph(
        &self,
        rasterizer: &mut dyn Rasterizer,
        index: u32,
        offset_value: I26Dot6,
        offset_axis_is_y: bool,
    ) -> Result<&SingleGlyphBitmap, GlyphRenderError> {
        match &self.0 {
            FontRepr::FreeType(font) => font.glyph_cache().get_or_try_render(
                rasterizer,
                font,
                index,
                offset_value,
                offset_axis_is_y,
            ),
            FontRepr::Tofu(font) => Ok(font
                .glyph_cache()
                .get_or_try_render(rasterizer, font, index, offset_value, offset_axis_is_y)
                .unwrap()),
        }
    }
}
