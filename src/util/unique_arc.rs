//! Makeshift polyfill for the yet-to-be-stabilised Rust [`UniqueArc`][https://github.com/rust-lang/rust/issues/112566].

use std::{
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    sync::Arc,
};

// PhantomData here to make the type invariant, although
// because this type does not permit weak references unlike
// std's UniqueArc so I don't think this is strictly required.
pub struct UniqueArc<T: ?Sized>(Arc<T>, PhantomData<*mut T>);

impl<T: ?Sized> UniqueArc<T> {
    pub fn into_raw(this: Self) -> *mut T {
        Arc::into_raw(this.0) as *mut T
    }

    pub unsafe fn from_raw(ptr: *mut T) -> Self {
        Self(Arc::from_raw(ptr), PhantomData)
    }

    pub fn into_arc(this: Self) -> Arc<T> {
        this.0
    }
}

impl<T> UniqueArc<[MaybeUninit<T>]> {
    pub fn new_uninit_slice(len: usize) -> Self {
        Self(Arc::new_uninit_slice(len), PhantomData)
    }
}

impl<T: ?Sized> Deref for UniqueArc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: ?Sized> DerefMut for UniqueArc<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { Arc::get_mut(&mut self.0).unwrap_unchecked() }
    }
}
