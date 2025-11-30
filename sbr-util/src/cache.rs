use std::{
    any::Any,
    cell::{Cell, RefCell, UnsafeCell},
    convert::Infallible,
    fmt::Debug,
    hash::{BuildHasher, Hash, Hasher, RandomState},
    mem::{ManuallyDrop, MaybeUninit},
    ptr::NonNull,
};

use hashbrown::{hash_table::Entry, HashTable};

use crate::ReadonlyAliasableBox;

#[derive(Debug)]
pub struct CacheConfiguration {
    /// Trim the cache once it reaches the specified approximate memory footprint.
    pub trim_memory_threshold: usize,
    /// Keep this many most recent generations while trimming the cache.
    pub trim_kept_generations: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheSlotState {
    /// Currently initializing or initialization was aborted via unwinding.
    Uninit,
    /// Initialized.
    Init,
    /// Initialization returned [`Err`].
    Failed,
}

trait ErasedCacheSlotValue: Any + 'static {
    unsafe fn assume_init_ref(&self) -> &dyn CacheValue;
    unsafe fn assume_init_drop(&mut self);
}

#[repr(C)]
struct CacheSlotValue<V> {
    value: UnsafeCell<MaybeUninit<V>>,
}

impl<V: CacheValue> ErasedCacheSlotValue for CacheSlotValue<V> {
    unsafe fn assume_init_ref(&self) -> &dyn CacheValue {
        unsafe { (&*self.value.get()).assume_init_ref() }
    }

    unsafe fn assume_init_drop(&mut self) {
        unsafe { self.value.get_mut().assume_init_drop() }
    }
}

struct CacheSlot<V: ErasedCacheSlotValue + ?Sized + 'static> {
    generation: Cell<u32>,
    state: Cell<CacheSlotState>,
    data: V,
}

impl<V: CacheValue + 'static> CacheSlot<CacheSlotValue<V>> {
    fn new(generation: u32) -> Self {
        Self {
            generation: Cell::new(generation),
            state: Cell::new(CacheSlotState::Uninit),
            data: CacheSlotValue {
                value: UnsafeCell::new(MaybeUninit::uninit()),
            },
        }
    }

    unsafe fn init(&self, value: V) -> &V {
        (*self.data.value.get()).write(value);
        self.state.set(CacheSlotState::Init);
        (*self.data.value.get()).assume_init_ref()
    }
}

impl<V: ErasedCacheSlotValue + ?Sized> CacheSlot<V> {
    fn value(&self) -> Option<&dyn CacheValue> {
        if self.state.get() == CacheSlotState::Init {
            Some(unsafe { self.data.assume_init_ref() })
        } else {
            None
        }
    }
}

impl<V: ErasedCacheSlotValue + ?Sized> Drop for CacheSlot<V> {
    fn drop(&mut self) {
        if self.state.get() == CacheSlotState::Init {
            unsafe { self.data.assume_init_drop() };
        }
    }
}

struct CacheEntry<K, V: ErasedCacheSlotValue + ?Sized + 'static> {
    key: K,
    slot: ReadonlyAliasableBox<CacheSlot<V>>,
}

impl<K: Hash, V: ErasedCacheSlotValue + ?Sized + 'static> Hash for CacheEntry<K, V> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.slot.data.type_id().hash(state);
    }
}

pub trait CacheValue: Any + 'static {
    fn memory_footprint(&self) -> usize;
}

#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub total_memory_footprint: usize,
    pub total_entries: usize,
    pub generation: u32,
}

pub struct Cache<K: 'static> {
    generation: Cell<u32>,
    total_memory_footprint: Cell<usize>,
    config: CacheConfiguration,
    entries: RefCell<HashTable<CacheEntry<K, dyn ErasedCacheSlotValue>>>,
    hasher: std::hash::RandomState,
}

impl<K> Debug for Cache<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphCache")
            .field("generation", &self.generation.get())
            .field("total_memory_footprint", &self.total_memory_footprint.get())
            .field("config", &self.config)
            .field(
                "entries",
                &crate::fmt_from_fn(|f| f.debug_list().finish_non_exhaustive()),
            )
            .finish_non_exhaustive()
    }
}

impl<K: Hash + PartialEq + Eq + 'static> Cache<K> {
    pub fn new(config: CacheConfiguration) -> Self {
        Self {
            generation: Cell::new(0),
            total_memory_footprint: Cell::new(0),
            config,
            entries: RefCell::new(HashTable::new()),
            hasher: RandomState::new(),
        }
    }

    pub fn advance_generation(&mut self) {
        let last_generation = self.generation.get();
        let keep_after = last_generation.saturating_sub(self.config.trim_kept_generations);
        if self.total_memory_footprint.get() >= self.config.trim_memory_threshold {
            let mut new_footprint = 0;
            self.entries.get_mut().retain(|entry| {
                let slot_generation = entry.slot.generation.get();
                let Some(value) = entry.slot.value() else {
                    return false;
                };
                // The extra `slot <= last` check ensures that if the generation wraps
                // the pre-wrap slots will get properly disposed of.
                let retained = slot_generation > keep_after && slot_generation <= last_generation;
                if retained {
                    new_footprint += value.memory_footprint();
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
            total_entries: self.entries.borrow().len(),
            generation: self.generation.get(),
        }
    }

    unsafe fn try_init_slot<'s, V: CacheValue, E>(
        &self,
        slot: &'s CacheSlot<CacheSlotValue<V>>,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&'s V, E> {
        debug_assert_eq!(slot.state.get(), CacheSlotState::Uninit);

        match insert() {
            Ok(new_value) => {
                self.total_memory_footprint
                    .set(self.total_memory_footprint.get() + new_value.memory_footprint());
                Ok(unsafe { slot.init(new_value) })
            }
            Err(err) => {
                slot.state.set(CacheSlotState::Failed);
                Err(err)
            }
        }
    }

    pub fn get_or_try_insert_with<V: CacheValue, E>(
        &self,
        key: K,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&V, E> {
        let mut entries = self.entries.borrow_mut();

        let key_hash = {
            // NOTE: This has to be the same as `CacheEntry::hash`
            let mut hasher = self.hasher.build_hasher();
            key.hash(&mut hasher);
            std::any::TypeId::of::<CacheSlotValue<V>>().hash(&mut hasher);
            hasher.finish()
        };
        let value = match entries.entry(
            key_hash,
            |entry| {
                entry.key == key
                    && entry.slot.data.type_id() == std::any::TypeId::of::<CacheSlotValue<V>>()
            },
            |entry| self.hasher.hash_one(entry),
        ) {
            Entry::Occupied(occupied) => {
                // SAFETY: The `TypeId` of the slot's value has just been checked above.
                //         This also implicitly casts the lifetime of the reference to
                //         the lifetime of &self, this is safe because we know the slot
                //         isn't going to be moved (it's behind a pointer) and it's not
                //         going to be removed without a &mut self reference invalidating
                //         the returned one.
                let slot = unsafe {
                    &*(occupied.get().slot.0.as_ptr() as *const _
                        as *const CacheSlot<CacheSlotValue<V>>)
                };
                slot.generation.set(self.generation.get());
                let value = match slot.state.get() {
                    CacheSlotState::Uninit => {
                        panic!(
                            concat!(
                                "Uninitialized cache slot accessed cyclically during initialization\n",
                                "Value type: {}"
                            ),
                            std::any::type_name::<V>()
                        );
                    }
                    CacheSlotState::Init => unsafe { (*slot.data.value.get()).assume_init_ref() },
                    CacheSlotState::Failed => {
                        drop(entries);

                        unsafe {
                            slot.state.set(CacheSlotState::Uninit);
                            self.try_init_slot(slot, insert)?
                        }
                    }
                };

                value
            }
            Entry::Vacant(vacant) => {
                let uninit_slot =
                    ManuallyDrop::new(ReadonlyAliasableBox::new(
                        CacheSlot::<CacheSlotValue<V>>::new(self.generation.get()),
                    ));
                // SAFETY: This is just an unsizing cast
                let erased_slot = unsafe {
                    ReadonlyAliasableBox(NonNull::new_unchecked(
                        uninit_slot.0.as_ptr() as *mut CacheSlot<dyn ErasedCacheSlotValue>
                    ))
                };
                // Insert the still uninitialized box into the map (we'll initialize it later).
                vacant.insert(CacheEntry {
                    key,
                    slot: erased_slot,
                });
                // Drop the borrow on the map so we don't panic on reentrancy.
                drop(entries);

                // SAFETY: This forges a lifetime, see explanation in the `Entry::Occupied` case.
                unsafe { self.try_init_slot(uninit_slot.0.as_ref(), insert)? }
            }
        };

        Ok(value)
    }

    #[inline]
    pub fn get_or_insert_with<V: CacheValue>(&self, key: K, insert: impl FnOnce() -> V) -> &V {
        match self.get_or_try_insert_with(key, || Ok::<_, Infallible>(insert())) {
            Ok(value) => value,
        }
    }
}

#[cfg(test)]
mod test {
    use std::{convert::Infallible, ops::Range, rc::Rc};

    use super::*;

    const TEST_CONFIGURATION: CacheConfiguration = CacheConfiguration {
        trim_memory_threshold: 2048,
        trim_kept_generations: 1,
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CachedInt(i64);

    impl CacheValue for CachedInt {
        fn memory_footprint(&self) -> usize {
            512
        }
    }

    #[test]
    fn simple() {
        let mut cache = Cache::<u32>::new(TEST_CONFIGURATION);

        fn insert_ints(cache: &Cache<u32>, range: Range<u32>, invocations: &mut u32) {
            for i in range {
                assert_eq!(
                    cache.get_or_insert_with(i, || {
                        *invocations += 1;
                        CachedInt(i.into())
                    }),
                    &CachedInt(i.into())
                );
            }
        }

        let mut invocations = 0;

        insert_ints(&cache, 0..3, &mut invocations);
        assert_eq!(invocations, 3);
        cache.advance_generation();

        insert_ints(&cache, 2..5, &mut invocations);
        assert_eq!(invocations, 5);
        cache.advance_generation();

        insert_ints(&cache, 0..6, &mut invocations);
        assert_eq!(invocations, 8);
    }

    #[test]
    fn init_error() {
        let cache = Cache::<u32>::new(TEST_CONFIGURATION);

        // First populate the map with a cache slot in failed state
        cache
            .get_or_try_insert_with(0, || Err::<CachedInt, _>(()))
            .unwrap_err();

        // Insert something else for good measure
        _ = cache.get_or_insert_with(1, || CachedInt(2));

        // Then check whether we are able to re-use the failed cache slot
        assert_eq!(cache.get_or_insert_with(0, || CachedInt(1)), &CachedInt(1));
    }

    #[test]
    fn reentrant_fibonacci() {
        let cache = Cache::<u32>::new(TEST_CONFIGURATION);

        let mut invocations = 0;
        cache.get_or_insert_with(0, || CachedInt(0));
        cache.get_or_insert_with(1, || CachedInt(1));
        fn compute_fibonacci(n: u32, cache: &Cache<u32>, invocations: &mut u32) -> i64 {
            cache
                .get_or_insert_with(n, || {
                    let a = compute_fibonacci(n - 2, cache, invocations);
                    let b = compute_fibonacci(n - 1, cache, invocations);

                    *invocations += 1;
                    CachedInt(a + b)
                })
                .0
        }

        assert_eq!(compute_fibonacci(32, &cache, &mut invocations), 2178309);
        assert_eq!(invocations, 31);
        assert_eq!(compute_fibonacci(48, &cache, &mut invocations), 4807526976);
        assert_eq!(invocations, 47);
    }

    #[test]
    #[should_panic]
    fn cycle_panics() {
        let cache = Cache::<u32>::new(TEST_CONFIGURATION);
        _ = cache.get_or_try_insert_with(0, || -> Result<_, Infallible> {
            cache
                .get_or_try_insert_with(0, || Ok(CachedInt(0)))
                .cloned()
        })
    }

    #[test]
    fn drops_values() {
        #[derive(Clone)]
        struct Dropped(Rc<Cell<bool>>);

        impl CacheValue for Dropped {
            fn memory_footprint(&self) -> usize {
                TEST_CONFIGURATION.trim_memory_threshold
            }
        }

        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let dropped = Dropped(Rc::new(Cell::new(false)));

        let mut cache = Cache::<u32>::new(TEST_CONFIGURATION);
        cache.get_or_insert_with(1, || dropped.clone());
        cache.advance_generation();

        assert!(dropped.0.get());
    }
}
