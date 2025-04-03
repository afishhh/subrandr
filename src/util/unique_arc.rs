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
    pub unsafe fn transmute<U>(this: Self) -> UniqueArc<U> {
        UniqueArc(
            unsafe { Arc::from_raw(Arc::into_raw(this.0) as *mut U) },
            PhantomData,
        )
    }

    pub fn freeze(this: Self) -> Arc<T> {
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
