use std::{
    alloc::{Layout, LayoutError},
    fmt::{Debug, Display},
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

// Must have the same representation as `usize` for safety.
// If the value of `self` as a `usize` matches `usize::MAX` then
// modifying `self` is forbidden (it may be in read-only memory!).
#[doc(hidden)]
pub unsafe trait Refcount {
    fn get(&self) -> usize;
    unsafe fn fetch_inc(&self) -> usize;
    unsafe fn dec(&self) -> bool;
    fn is_unique(&self) -> bool;
}

// A max reference count that will trigger a panic once surpassed.
pub(super) const REFCOUNT_MAX: usize = if (u32::MAX as usize) < (usize::MAX >> 1) {
    u32::MAX as usize
} else {
    usize::MAX >> 1
};

#[cold]
#[track_caller]
fn panic_refcount_overflow() {
    panic!("Reference-counted pointer surpassed {REFCOUNT_MAX} references")
}

// `AtomicUsize` may actually have a stricter alignment than `usize` on some platforms.
// This type is thus used in place of `usize` where we want a generic `usize` or `AtomicUsize`
// type.
#[repr(C)]
#[cfg_attr(target_pointer_width = "64", repr(align(8)))]
#[cfg_attr(target_pointer_width = "32", repr(align(4)))]
#[cfg_attr(target_pointer_width = "16", repr(align(2)))]
#[doc(hidden)]
pub struct AtomicAlignedUsize(pub usize);

#[repr(C)]
#[doc(hidden)]
pub struct RcBox<R, T: ?Sized> {
    pub refs: R,
    pub value: T,
}

fn layout_for_rcbox(inner: Layout) -> Result<Layout, LayoutError> {
    Ok(Layout::new::<AtomicAlignedUsize>()
        .extend(inner)?
        .0
        .pad_to_align())
}

impl<R: Refcount, T: ?Sized> RcBox<R, T> {
    pub const unsafe fn from_static(
        ptr: &'static RcBox<AtomicAlignedUsize, T>,
    ) -> *mut RcBox<R, T> {
        debug_assert!(ptr.refs.0 == usize::MAX);

        ptr as *const _ as *mut RcBox<AtomicAlignedUsize, T> as *mut _
    }
}

impl<R: Refcount> RcBox<R, ()> {
    fn allocate(inner: Layout) -> Result<NonNull<Self>, LayoutError> {
        unsafe {
            let layout = layout_for_rcbox(inner)?;
            let data = std::alloc::alloc(layout);
            let ptr = NonNull::new(data as *mut RcBox<R, ()>).unwrap_or_else(|| {
                std::alloc::handle_alloc_error(layout);
            });
            (&raw mut (*ptr.as_ptr()).refs).write(std::mem::transmute_copy(&1usize));
            Ok(ptr)
        }
    }
}

#[macro_export]
macro_rules! rc_static {
    // `str` currently can't be unsized via an unsizing coercion so we
    // are forced to go through a `[u8]` and then assert that it is,
    // in fact, UTF-8.
    (str $byte_literal: literal) => {
        const {
            let storage: &'static $crate::rc::base::RcBox<_, [u8]> = const {
                &$crate::rc::base::RcBox {
                    refs: $crate::rc::base::AtomicAlignedUsize(usize::MAX),
                    value: *$byte_literal,
                }
            };

            assert!(
                std::str::from_utf8($byte_literal).is_ok(),
                "byte literal must be valid UTF-8"
            );

            unsafe {
                $crate::rc::base::RcBase::from_raw_box(::std::ptr::NonNull::new_unchecked(
                    $crate::rc::base::RcBox::from_static(::std::mem::transmute(storage)),
                ))
            }
        }
    };
    ($value: expr) => {
        const {
            let storage: &'static $crate::rc::base::RcBox<_, _> = const {
                &$crate::rc::base::RcBox {
                    refs: $crate::rc::base::AtomicAlignedUsize(usize::MAX),
                    value: $value,
                }
            };

            unsafe {
                $crate::rc::base::RcBase::from_raw_box(::std::ptr::NonNull::new_unchecked(
                    $crate::rc::base::RcBox::from_static(storage),
                ))
            }
        }
    };
}
pub use rc_static;

pub struct UniqueRcBase<R: Refcount, T: ?Sized> {
    ptr: NonNull<RcBox<R, T>>,
}

impl<R: Refcount, T: ?Sized> UniqueRcBase<R, T> {
    #[inline]
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        let raw_box = unsafe {
            NonNull::new_unchecked(Box::into_raw(Box::new(RcBox {
                refs: std::mem::transmute_copy(&1usize),
                value,
            })))
        };

        Self::from_raw_box(raw_box)
    }

    #[doc(hidden)]
    #[inline]
    pub const fn from_raw_box(raw_box: NonNull<RcBox<R, T>>) -> Self {
        Self { ptr: raw_box }
    }

    fn inner(&self) -> &RcBox<R, T> {
        unsafe { self.ptr.as_ref() }
    }

    fn inner_mut(&mut self) -> &mut RcBox<R, T> {
        unsafe { self.ptr.as_mut() }
    }

    #[inline]
    pub const fn into_shared(this: Self) -> RcBase<R, T> {
        unsafe { RcBase::from_raw_box(this.ptr) }
    }
}

impl<R: Refcount, T> UniqueRcBase<R, [T]> {
    #[inline]
    pub fn new_uninit_slice(len: usize) -> UniqueRcBase<R, [MaybeUninit<T>]>
    where
        T: Sized,
    {
        let erased_box = Layout::array::<T>(len)
            .and_then(RcBox::<R, ()>::allocate)
            .expect("rcbox slice length overflowed");

        unsafe {
            let raw_box = NonNull::new_unchecked(std::ptr::slice_from_raw_parts_mut(
                erased_box.as_ptr(),
                len,
            ) as *mut RcBox<R, [MaybeUninit<T>]>);

            UniqueRcBase::from_raw_box(raw_box)
        }
    }

    #[inline]
    pub fn new_zeroed_slice(len: usize) -> UniqueRcBase<R, [MaybeUninit<T>]> {
        let mut result = Self::new_uninit_slice(len);
        unsafe { result.as_mut_ptr().write_bytes(0, len) };
        result
    }
}

impl<R: Refcount, T> UniqueRcBase<R, [MaybeUninit<T>]> {
    #[inline]
    pub unsafe fn assume_init(this: Self) -> UniqueRcBase<R, [T]> {
        UniqueRcBase {
            ptr: unsafe { NonNull::new_unchecked(this.ptr.as_ptr() as *mut _) },
        }
    }
}

impl<R: Refcount, T> From<T> for UniqueRcBase<R, T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<R: Refcount, T> From<Vec<T>> for UniqueRcBase<R, [T]> {
    fn from(mut value: Vec<T>) -> Self {
        let mut storage = Self::new_uninit_slice(value.len());

        unsafe {
            std::ptr::copy_nonoverlapping(
                value.as_ptr(),
                storage.as_mut_ptr() as *mut T,
                value.len(),
            );
            value.set_len(0);
            UniqueRcBase::assume_init(storage)
        }
    }
}

impl<R: Refcount> From<&str> for UniqueRcBase<R, str> {
    fn from(value: &str) -> Self {
        let mut storage = UniqueRcBase::<R, [u8]>::new_uninit_slice(value.len());

        unsafe {
            std::ptr::copy_nonoverlapping(
                value.as_ptr(),
                storage.as_mut_ptr() as *mut u8,
                value.len(),
            );

            UniqueRcBase {
                ptr: NonNull::new_unchecked(storage.ptr.as_ptr() as *mut RcBox<R, str>),
            }
        }
    }
}

impl<R: Refcount, T: Default> Default for UniqueRcBase<R, T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<R: Refcount, T: Debug + ?Sized> Debug for UniqueRcBase<R, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.inner().value, f)
    }
}

impl<R: Refcount, T: Display + ?Sized> Display for UniqueRcBase<R, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.inner().value, f)
    }
}

impl<R: Refcount, T: ?Sized> Deref for UniqueRcBase<R, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().value
    }
}

impl<R: Refcount, T: ?Sized> AsRef<T> for UniqueRcBase<R, T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.inner().value
    }
}

impl<R: Refcount, T: ?Sized> DerefMut for UniqueRcBase<R, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner_mut().value
    }
}

impl<R: Refcount, T: ?Sized> AsMut<T> for UniqueRcBase<R, T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        &mut self.inner_mut().value
    }
}

unsafe impl<R: Refcount, T: Send + ?Sized> Send for UniqueRcBase<R, T> {}
unsafe impl<R: Refcount, T: Sync + ?Sized> Sync for UniqueRcBase<R, T> {}

pub struct RcBase<R: Refcount, T: ?Sized> {
    ptr: NonNull<RcBox<R, T>>,
}

impl<R: Refcount, T: ?Sized> RcBase<R, T> {
    #[inline]
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        UniqueRcBase::into_shared(UniqueRcBase::new(value))
    }

    #[doc(hidden)]
    #[inline]
    pub const unsafe fn from_raw_box(raw_box: NonNull<RcBox<R, T>>) -> Self {
        Self { ptr: raw_box }
    }

    fn inner(&self) -> &RcBox<R, T> {
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    pub fn strong_count(this: &Self) -> usize {
        this.inner().refs.get()
    }

    // TODO: Generalize to `CloneToUninit` once stable.
    pub fn make_mut(this: &mut Self) -> &mut T
    where
        T: Clone,
    {
        if !this.inner().refs.is_unique() {
            *this = Self::new(this.inner().value.clone());
        }

        unsafe { &mut this.ptr.as_mut().value }
    }

    pub fn make_unique(this: Self) -> UniqueRcBase<R, T>
    where
        T: Clone,
    {
        if this.inner().refs.is_unique() {
            UniqueRcBase { ptr: this.ptr }
        } else {
            UniqueRcBase::new(this.inner().value.clone())
        }
    }

    #[inline]
    pub fn hash_ptr(this: &Self, hasher: &mut impl std::hash::Hasher) {
        std::hash::Hash::hash(&this.ptr, hasher)
    }

    #[inline]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        std::ptr::eq(this.ptr.as_ptr() as *mut (), other.ptr.as_ptr() as *mut ())
    }
}

impl<R: Refcount, T> From<T> for RcBase<R, T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

impl<R: Refcount, T> From<Vec<T>> for RcBase<R, [T]> {
    fn from(value: Vec<T>) -> Self {
        UniqueRcBase::into_shared(UniqueRcBase::from(value))
    }
}

impl<R: Refcount> From<&str> for RcBase<R, str> {
    fn from(value: &str) -> Self {
        UniqueRcBase::into_shared(UniqueRcBase::from(value))
    }
}

impl<R: Refcount, T: Default> Default for RcBase<R, T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::default())
    }
}

impl<R: Refcount, T: PartialEq + ?Sized> PartialEq for RcBase<R, T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        if Self::ptr_eq(self, other) {
            return true;
        }

        *self == *other
    }
}

impl<R: Refcount, T: Eq + ?Sized> Eq for RcBase<R, T> {}

impl<R: Refcount, T: Debug + ?Sized> Debug for RcBase<R, T> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.inner().value, f)
    }
}

impl<R: Refcount, T: Display + ?Sized> Display for RcBase<R, T> {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.inner().value, f)
    }
}

impl<R: Refcount, T: ?Sized> Deref for RcBase<R, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner().value
    }
}

impl<R: Refcount, T: ?Sized> AsRef<T> for RcBase<R, T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.inner().value
    }
}

impl<R: Refcount, T: ?Sized> Clone for RcBase<R, T> {
    #[inline]
    fn clone(&self) -> Self {
        if self.inner().refs.get() != usize::MAX {
            let old_count = unsafe { self.inner().refs.fetch_inc() };

            if old_count >= REFCOUNT_MAX {
                panic_refcount_overflow();
            }
        }

        Self { ptr: self.ptr }
    }
}

impl<R: Refcount, T: ?Sized> Drop for RcBase<R, T> {
    #[inline]
    fn drop(&mut self) {
        if self.inner().refs.get() != usize::MAX {
            unsafe {
                if self.inner().refs.dec() {
                    drop(Box::from_raw(self.ptr.as_ptr()));
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{fmt::Debug, sync::Barrier};

    use crate::rc::{
        base::{RcBase, Refcount, UniqueRcBase},
        Arc, Rc,
    };

    fn pass_around_single_thread<R: Refcount, T: Debug + PartialEq + Clone>(rc: RcBase<R, T>) {
        {
            let cloned = rc.clone();
            assert_eq!(RcBase::strong_count(&cloned), 2);

            let unique = RcBase::make_unique(cloned);
            assert_eq!(*unique, *rc);
            assert_eq!(RcBase::strong_count(&rc), 1);

            let shared = UniqueRcBase::into_shared(unique);
            assert_eq!(RcBase::strong_count(&shared), 1);
        }

        {
            let cloned = rc.clone();
            let another = cloned.clone();
            assert_eq!(RcBase::strong_count(&cloned), 3);
            RcBase::ptr_eq(&cloned, &rc);
            drop(cloned);
            RcBase::ptr_eq(&another, &rc);
            assert_eq!(RcBase::strong_count(&rc), 2);
        }
    }

    fn concurrent_clone<T: Send + Sync>(rc: Arc<T>) {
        let barrier = Barrier::new(4);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let barrier = &barrier;
                let clone = rc.clone();
                scope.spawn(move || {
                    barrier.wait();
                    let mut rcs = Vec::new();
                    for _ in 0..1000 {
                        rcs.push(clone.clone());
                    }
                    std::hint::black_box(rcs);
                    barrier.wait();
                });
            }
        });

        assert_eq!(Arc::strong_count(&rc), 1);
    }

    fn static_str<R: Refcount>(value: RcBase<R, str>) {
        assert_eq!(&*value, "hello");
        assert_eq!(RcBase::strong_count(&value), usize::MAX);

        {
            let c1 = value.clone();
            {
                let c2 = value.clone();
                assert_eq!(RcBase::strong_count(&c2), usize::MAX);
            }

            assert_eq!(value, c1);
            assert!(RcBase::ptr_eq(&value, &c1));
        }
    }

    #[test]
    fn static_str_rc() {
        static_str(rc_static!(str b"hello") as Rc<str>);
    }

    #[test]
    fn static_str_arc() {
        static_str(rc_static!(str b"hello") as Arc<str>);
    }

    #[test]
    fn from_vec_from_str() {
        let rc = Rc::<[Rc<str>]>::from(vec![
            Rc::from("one"),
            Rc::from("two"),
            Rc::from("three"),
            Rc::from("four"),
        ]);
        _ = rc.clone();
        std::hint::black_box(rc);
    }

    #[test]
    fn pass_around_rc_single_thraed() {
        pass_around_single_thread(Rc::new(20i32));
        pass_around_single_thread(Rc::new(String::from("world")));
    }

    #[test]
    fn pass_around_arc_single_thraed() {
        pass_around_single_thread(Arc::new(20i32));
        pass_around_single_thread(Arc::new(String::from("world")));
    }

    #[test]
    fn arc_concurrent_clone() {
        concurrent_clone(Arc::new(10i32));
        concurrent_clone(Arc::new(String::from("hello")));
    }
}
