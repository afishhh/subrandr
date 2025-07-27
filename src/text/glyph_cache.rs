use std::{fmt::Debug, hash::Hash};

use util::{
    cache::{Cache, CacheConfiguration, CacheStats, CacheValue},
    math::{I16Dot16, I26Dot6, Vec2},
    HashF32,
};

use crate::text::Face;

const CACHE_CONFIGURATION: CacheConfiguration = CacheConfiguration {
    // Trim the cache once it reaches 2MiB approximate memory footprint.
    trim_memory_threshold: 2 * 1024 * 1024,
    // Keep the last two cache generations while trimming.
    trim_kept_generations: 3,
};

#[derive(Debug, Clone)]
#[repr(transparent)]
struct FaceByAddr(Face);

impl PartialEq for FaceByAddr {
    fn eq(&self, other: &Self) -> bool {
        self.0.addr() == other.0.addr()
    }
}

impl Eq for FaceByAddr {}

impl Hash for FaceByAddr {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.addr().hash(state)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Key {
    // It would be nice if this was a weak pointer but
    // it cannot be because `FT_Face` does not support that.
    // TODO: Technically I think face extra data should
    //       allow one to implement weak refs on top of FT_Face.
    face: FaceByAddr,
    point_size: I26Dot6,
    dpi: u32,
    coords: [I16Dot16; text_sys::T1_MAX_MM_AXIS as usize],
    glyph: u32,
    blur_sigma: HashF32,
    subpixel_bucket: u8,
}

#[derive(Debug)]
pub struct GlyphCache(Cache<Key>);

impl GlyphCache {
    pub fn new() -> Self {
        Self(Cache::new(CACHE_CONFIGURATION))
    }

    pub fn advance_generation(&mut self) {
        self.0.advance_generation();
    }

    pub fn stats(&self) -> CacheStats {
        self.0.stats()
    }

    pub(super) fn get_or_try_insert_with<V: CacheValue, E>(
        &self,
        key: Key,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&V, E> {
        self.0.get_or_try_insert_with(key, insert)
    }
}

pub(super) struct FontSizeCacheKey {
    point_size: I26Dot6,
    dpi: u32,
    coords: [I16Dot16; text_sys::T1_MAX_MM_AXIS as usize],
}

impl FontSizeCacheKey {
    pub fn new(
        point_size: I26Dot6,
        dpi: u32,
        coords: [I16Dot16; text_sys::T1_MAX_MM_AXIS as usize],
    ) -> FontSizeCacheKey {
        Self {
            point_size,
            dpi,
            coords,
        }
    }

    pub(super) fn get_subpixel_bucket(offset: I26Dot6, y_axis: bool) -> (Vec2<I26Dot6>, u8) {
        let offset_trunc = I26Dot6::from_raw(offset.into_raw() & 0b110000);
        let bucket = (offset_trunc.into_raw() >> 3) as u8 | y_axis as u8;
        let render_offset = if y_axis {
            Vec2::new(I26Dot6::ZERO, offset_trunc)
        } else {
            Vec2::new(offset_trunc, I26Dot6::ZERO)
        };

        (render_offset, bucket)
    }

    pub(super) fn for_glyph(
        &self,
        face: Face,
        glyph: u32,
        blur_sigma: f32,
        subpixel_bucket: u8,
    ) -> Key {
        Key {
            face: FaceByAddr(face),
            point_size: self.point_size,
            dpi: self.dpi,
            coords: self.coords,
            glyph,
            blur_sigma: HashF32::new(blur_sigma),
            subpixel_bucket,
        }
    }
}
