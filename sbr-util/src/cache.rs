use std::{
    alloc::Layout,
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    collections::{hash_map::Entry, HashMap},
    convert::Infallible,
    fmt::Debug,
    hash::Hash,
    mem::{offset_of, ManuallyDrop},
    ptr::NonNull,
};

#[derive(Debug)]
pub struct CacheConfiguration {
    /// Trim the cache once it reaches the specified approximate memory footprint.
    pub trim_memory_threshold: usize,
    /// Keep the last `n-1` generations while trimming the cache.
    pub trim_kept_generations: u32,
}

#[derive(Debug)]
struct CacheSlotHeader {
    generation: Cell<u32>,
    state: Cell<CacheSlotState>,
    // FIXME: Workaround for the absence of `#![feature(layout_for_ptr)]`.
    //        (https://github.com/rust-lang/rust/issues/69835)
    //        Since without the above feature we cannot get the layout from
    //        behind the vtable we have to store another copy of the allocated
    //        `Layout` here. Once that feature is stabilized this can be removed.
    //        For now we'll have to live with these extra 16-bytes (+ padding).
    layout: Layout,
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

// Caution! This structure must be used *very* carefully.
// The `value` field may actually be in an uninitialized state if
// `header.state` is not `Init`. If that is the case creating a
// reference to this struct is immediate UB.
struct CacheSlot<V: ?Sized + 'static> {
    header: CacheSlotHeader,
    value: V,
}

impl<V: 'static> CacheSlot<V> {
    unsafe fn init<'a>(this: NonNull<Self>, value: V) -> &'a V {
        let value_ptr = this.byte_add(offset_of!(Self, value)).cast::<V>();
        value_ptr.write(value);
        Self::set_state(this, CacheSlotState::Init);
        value_ptr.as_ref()
    }

    unsafe fn set_state(this: NonNull<Self>, state: CacheSlotState) {
        Self::header(this).state.set(state);
    }
}

impl<V: ?Sized + 'static> CacheSlot<V> {
    fn header_ptr(this: NonNull<Self>) -> NonNull<CacheSlotHeader> {
        unsafe {
            this
                // In practice this is always zero but technically not guaranteed?
                .byte_add(offset_of!(CacheSlot<V>, header))
                .cast::<CacheSlotHeader>()
        }
    }

    unsafe fn header<'a>(this: NonNull<Self>) -> &'a CacheSlotHeader {
        unsafe { Self::header_ptr(this).as_ref() }
    }
}

struct CacheBox<V: ?Sized + 'static>(NonNull<CacheSlot<V>>);

impl<V: 'static> CacheBox<V> {
    fn new(generation: u32) -> Self {
        Self(unsafe {
            let layout = Layout::new::<CacheSlot<V>>();
            let ptr = std::alloc::alloc(layout).cast();
            let Some(ptr) = NonNull::new(ptr) else {
                std::alloc::handle_alloc_error(layout)
            };

            CacheSlot::header_ptr(ptr).write(CacheSlotHeader {
                generation: Cell::new(generation),
                state: Cell::new(CacheSlotState::Uninit),
                layout,
            });

            ptr
        })
    }
}

impl<V: ?Sized + 'static> CacheBox<V> {
    fn header(&self) -> &CacheSlotHeader {
        unsafe { CacheSlot::header(self.0) }
    }

    fn value(&self) -> Option<&V> {
        if self.header().state.get() == CacheSlotState::Init {
            Some(unsafe { self.value_assume_init_ref() })
        } else {
            None
        }
    }

    unsafe fn value_assume_init_ref(&self) -> &V {
        &unsafe { &*self.0.as_ptr() }.value
    }
}

impl<V: ?Sized + 'static> Drop for CacheBox<V> {
    fn drop(&mut self) {
        if self.header().state.get() == CacheSlotState::Init {
            unsafe { std::ptr::drop_in_place(self.0.as_ptr()) };
        }

        let layout = self.header().layout;
        unsafe {
            std::alloc::dealloc(self.0.as_ptr() as *mut u8, layout);
        }
    }
}

pub trait CacheValue: Any + 'static {
    fn memory_footprint(&self) -> usize;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ErasedKey<K: PartialEq + Eq + Hash> {
    data: K,
    value_type: TypeId,
}

#[derive(Debug, Clone, Copy)]
pub struct CacheStats {
    pub total_memory_footprint: usize,
    pub total_entries: usize,
    pub generation: u32,
}

// TODO: Also erase keys, but maybe do it in a smart way?
//       Maybe that's overkill though
pub struct Cache<K: PartialEq + Eq + Hash> {
    generation: Cell<u32>,
    total_memory_footprint: Cell<usize>,
    config: CacheConfiguration,
    glyphs: RefCell<HashMap<ErasedKey<K>, CacheBox<dyn CacheValue>>>,
}

impl<K: PartialEq + Eq + Hash> Debug for Cache<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlyphCache")
            .field("generation", &self.generation.get())
            .field("total_memory_footprint", &self.total_memory_footprint.get())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<K: PartialEq + Eq + Hash> Cache<K> {
    pub fn new(config: CacheConfiguration) -> Self {
        Self {
            generation: Cell::new(0),
            total_memory_footprint: Cell::new(0),
            config,
            glyphs: RefCell::new(HashMap::new()),
        }
    }

    pub fn advance_generation(&mut self) {
        let last_generation = self.generation.get();
        let keep_after = last_generation.saturating_sub(self.config.trim_kept_generations);
        if self.total_memory_footprint.get() >= self.config.trim_memory_threshold {
            let mut new_footprint = 0;
            self.glyphs.get_mut().retain(|_, slot| {
                let slot_generation = slot.header().generation.get();
                let Some(value) = slot.value() else {
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
            total_entries: self.glyphs.borrow().len(),
            generation: self.generation.get(),
        }
    }

    unsafe fn try_init_slot<V: CacheValue, E>(
        &self,
        slot_ptr: NonNull<CacheSlot<V>>,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&'static V, E> {
        debug_assert_eq!(
            CacheSlot::header(slot_ptr).state.get(),
            CacheSlotState::Uninit
        );

        match insert() {
            Ok(new_value) => {
                self.total_memory_footprint
                    .set(self.total_memory_footprint.get() + new_value.memory_footprint());
                Ok(unsafe { CacheSlot::init(slot_ptr, new_value) })
            }
            Err(err) => {
                unsafe { CacheSlot::set_state(slot_ptr, CacheSlotState::Failed) };
                Err(err)
            }
        }
    }

    pub fn get_or_try_insert_with<V: CacheValue, E>(
        &self,
        key: K,
        insert: impl FnOnce() -> Result<V, E>,
    ) -> Result<&V, E> {
        let mut glyphs = self.glyphs.borrow_mut();

        // TODO: This could be partially monomorphised
        let slot = match glyphs.entry(ErasedKey {
            data: key,
            value_type: TypeId::of::<V>(),
        }) {
            Entry::Occupied(occupied) => {
                let slot = occupied.get();
                slot.header().generation.set(self.generation.get());
                let value = match slot.header().state.get() {
                    CacheSlotState::Uninit => {
                        panic!(
                            concat!(
                                "Uninitialized cache slot accessed cyclically during initialization\n",
                                "Value type: {}"
                            ),
                            std::any::type_name::<V>()
                        );
                    }
                    CacheSlotState::Init => {
                        let value = unsafe { slot.value_assume_init_ref() };
                        debug_assert_eq!(value.type_id(), std::any::TypeId::of::<V>());
                        value
                    }
                    CacheSlotState::Failed => {
                        let slot_ptr =
                            unsafe { NonNull::new_unchecked(slot.0.as_ptr() as *mut CacheSlot<V>) };
                        drop(glyphs);

                        unsafe {
                            CacheSlot::set_state(slot_ptr, CacheSlotState::Uninit);
                            self.try_init_slot(slot_ptr, insert)?
                        }
                    }
                };

                // SAFETY: This reference is behind an allocation which won't be removed from the map
                //         without a &mut reference and is only ever accessed immutably for the
                //         remainder of its lifetime.
                unsafe { std::mem::transmute::<&dyn CacheValue, &'static dyn CacheValue>(value) }
            }
            Entry::Vacant(vacant) => {
                let uninit_box = CacheBox::<V>::new(self.generation.get());
                let slot_ptr = uninit_box.0;
                let erased_box = {
                    unsafe {
                        CacheBox(NonNull::new_unchecked(
                            ManuallyDrop::new(uninit_box).0.as_ptr()
                                as *mut CacheSlot<dyn CacheValue>,
                        ))
                    }
                };
                // Insert the still uninitialized box into the map (we'll initialize it later).
                vacant.insert(erased_box);
                // Drop the borrow on the map so we don't panic on reentrancy.
                drop(glyphs);

                unsafe { self.try_init_slot(slot_ptr, insert)? }
            }
        };

        // SAFETY: Either this value has just been inserted into the map so we
        //         know its type or the `TypeId` has already been checked since
        //         it's part of the key.
        Ok(unsafe { &*(slot as *const dyn CacheValue as *const () as *const V) })
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
    use std::{convert::Infallible, ops::Range};

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
}
