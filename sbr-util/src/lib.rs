use std::{borrow::Borrow, hash::Hash, mem::MaybeUninit, ops::Deref, ptr::NonNull};

pub mod math;
pub mod rc;
pub mod small_type_map;

pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

/// Asserts that the entirety of `slice` is initialized and returns a mutable slice
/// of the initialized contents.
///
/// # Safety
///
/// `slice` must be fully initialized.
pub const unsafe fn slice_assume_init_mut<T>(slice: &mut [MaybeUninit<T>]) -> &mut [T] {
    unsafe { &mut *(slice as *mut [_] as *mut [T]) }
}

pub fn vec_parts<T>(v: &mut Vec<T>) -> (*mut T, usize, usize) {
    let ptr = v.as_mut_ptr();
    let len = v.len();
    let capacity = v.capacity();
    (ptr, len, capacity)
}

// Formatting helpers
// Remove once [debug_closure_helpers](https://github.com/rust-lang/rust/issues/117729) is stabilized.

pub struct FormatterFn<F>(pub F)
where
    F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result;

impl<F> std::fmt::Debug for FormatterFn<F>
where
    F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.0)(f)
    }
}

impl<F> std::fmt::Display for FormatterFn<F>
where
    F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.0)(f)
    }
}

pub const fn fmt_from_fn<F>(f: F) -> FormatterFn<F>
where
    F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result,
{
    FormatterFn(f)
}

#[derive(Debug)]
pub struct ReadonlyAliasableBox<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> ReadonlyAliasableBox<T> {
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        Self(unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(value))) })
    }

    pub fn as_nonnull(this: &Self) -> NonNull<T> {
        this.0
    }
}

impl<T: Hash + ?Sized> Hash for ReadonlyAliasableBox<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl<T: PartialEq + ?Sized> PartialEq for ReadonlyAliasableBox<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: Eq + ?Sized> Eq for ReadonlyAliasableBox<T> {}

impl<T: ?Sized> Deref for ReadonlyAliasableBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> Borrow<T> for ReadonlyAliasableBox<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: ?Sized> AsRef<T> for ReadonlyAliasableBox<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: ?Sized> From<Box<T>> for ReadonlyAliasableBox<T> {
    fn from(value: Box<T>) -> Self {
        unsafe { Self(NonNull::new_unchecked(Box::into_raw(value))) }
    }
}

impl<T: ?Sized> Drop for ReadonlyAliasableBox<T> {
    fn drop(&mut self) {
        unsafe { _ = Box::from_raw(self.0.as_ptr()) };
    }
}

#[test]
fn readonly_aliasable_box() {
    _ = ReadonlyAliasableBox::new(String::from("hello"));
    let boxed = ReadonlyAliasableBox::new(String::from("second"));
    let aliasing = boxed.as_ptr();
    _ = boxed;
    _ = aliasing;
}
