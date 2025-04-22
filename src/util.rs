pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

pub trait Sealed {}

use std::{borrow::Borrow, hash::Hash, mem::MaybeUninit, ops::Deref, ptr::NonNull};

pub const unsafe fn array_assume_init_ref<const N: usize, T>(
    array: &[MaybeUninit<T>; N],
) -> &[T; N] {
    unsafe { &*(array as *const [_] as *const [T; N]) }
}

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

pub struct ReadonlyAliasableBox<T>(NonNull<T>);

impl<T> ReadonlyAliasableBox<T> {
    pub fn new(value: T) -> Self {
        Self(unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(value))) })
    }

    pub fn as_nonnull(this: &Self) -> NonNull<T> {
        this.0
    }
}

impl<T: Hash> Hash for ReadonlyAliasableBox<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl<T: PartialEq> PartialEq for ReadonlyAliasableBox<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: Eq> Eq for ReadonlyAliasableBox<T> {}

impl<T> Deref for ReadonlyAliasableBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T> Borrow<T> for ReadonlyAliasableBox<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T> AsRef<T> for ReadonlyAliasableBox<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T> Drop for ReadonlyAliasableBox<T> {
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
