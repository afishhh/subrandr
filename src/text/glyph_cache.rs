use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    collections::{hash_map::Entry, HashMap},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
};

use util::{
    math::{I16Dot16, I26Dot6, Vec2},
    ReadonlyAliasableBox,
};

use super::Face;

// Trim the cache once it reaches 2MiB approximate memory footprint.
const TRIM_MEMORY_THRESHOLD: usize = 2 * 1024 * 1024;
// Keep the last two cache generations while trimming.
const TRIM_KEPT_GENERATIONS: u32 = 3;

// TODO: Generation *and* LRU-based eviction?
struct CacheSlot<V: ?Sized + 'static> {
    generation: Cell<u32>,
    value: V,
}

impl<V: CacheValue> CacheSlot<V> {
    fn new(value: V) -> Self {
        Self {
            generation: Cell::new(0),
            value,
        }
    }
}

pub(super) trait CacheValue: Any + 'static {
    fn memory_footprint(&self) -> usize;
}

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

#[derive(Debug, Clone)]
pub(super) struct CacheKey<V: CacheValue> {
    inner: ErasedKey,
    _type: PhantomData<fn(V) -> V>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ErasedKey {
    // It would be nice if this was a weak pointer but
    // it cannot be because `FT_Face` does not support that.
    // TODO: Technically I think face extra data should
    //       allow one to implement weak refs on top of FT_Face.
    face: FaceByAddr,
    point_size: I26Dot6,
    dpi: u32,
    coords: [I16Dot16; text_sys::T1_MAX_MM_AXIS as usize],
    glyph: u32,
    subpixel_bucket: u8,
    value_type: TypeId,
}

#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub total_memory_footprint: usize,
    pub total_entries: usize,
    pub generation: u32,
}

pub struct GlyphCache {
    generation: Cell<u32>,
    total_memory_footprint: Cell<usize>,
    glyphs: RefCell<HashMap<ErasedKey, ReadonlyAliasableBox<CacheSlot<dyn CacheValue>>>>,
}

impl Debug for GlyphCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphCache")
            .field("generation", &self.generation.get())
            .field("total_memory_footprint", &self.total_memory_footprint.get())
            .finish_non_exhaustive()
    }
}

impl GlyphCache {
    pub fn new() -> Self {
        Self {
            generation: Cell::new(0),
            total_memory_footprint: Cell::new(0),
            glyphs: RefCell::new(HashMap::new()),
        }
    }

    pub fn advance_generation(&mut self) {
        let last_generation = self.generation.get();
        let keep_after = last_generation.saturating_sub(TRIM_KEPT_GENERATIONS);
        if self.total_memory_footprint.get() >= TRIM_MEMORY_THRESHOLD {
            let mut new_footprint = 0;
            self.glyphs.get_mut().retain(|_, slot| {
                let slot_generation = slot.generation.get();
                // The extra `slot <= last` check ensures that if the generation wraps
                // the pre-wrap slots will get properly disposed of.
                let retained = slot_generation > keep_after && slot_generation <= last_generation;
                if retained {
                    new_footprint += slot.value.memory_footprint();
                }
                retained
            });
            self.total_memory_footprint.set(new_footprint);
        }

        self.generation.set(last_generation.wrapping_add(1));
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            total_memory_footprint: self.total_memory_footprint.get(),
            total_entries: self.glyphs.borrow().len(),
            generation: self.generation.get(),
        }
    }

    pub(super) fn get_or_try_insert_with<V: CacheValue, E>(
        &self,
        key: CacheKey<V>,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&V, E> {
        let mut glyphs = self.glyphs.borrow_mut();

        let occupied = match glyphs.entry(key.inner) {
            Entry::Occupied(occupied) => {
                debug_assert_eq!(occupied.get().value.type_id(), std::any::TypeId::of::<V>());
                occupied
            }
            Entry::Vacant(vacant) => vacant.insert_entry({
                let new_value = insert()?;
                self.total_memory_footprint
                    .set(self.total_memory_footprint.get() + new_value.memory_footprint());
                ReadonlyAliasableBox::from(
                    Box::new(CacheSlot::new(new_value)) as Box<CacheSlot<dyn CacheValue>>
                )
            }),
        };

        let slot = occupied.get();
        slot.generation.set(self.generation.get());
        // SAFETY: This reference is behind a Box and won't be removed from the map
        //         without a &mut reference to the glyph cache.
        //         The entry is only ever accessed immutably.
        Ok(unsafe {
            std::mem::transmute::<&V, &'static V>(
                (&slot.value as &dyn Any)
                    .downcast_ref::<V>()
                    .unwrap_unchecked(),
            )
        })
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

    pub(super) fn for_glyph<V: CacheValue>(
        &self,
        face: Face,
        glyph: u32,
        subpixel_bucket: u8,
    ) -> CacheKey<V> {
        CacheKey {
            inner: ErasedKey {
                face: FaceByAddr(face),
                point_size: self.point_size,
                dpi: self.dpi,
                coords: self.coords,
                glyph,
                subpixel_bucket,
                value_type: std::any::TypeId::of::<V>(),
            },
            _type: PhantomData,
        }
    }
}
