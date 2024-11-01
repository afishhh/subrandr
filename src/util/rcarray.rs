use std::{
    fmt::Debug,
    ops::{Deref, Index},
    slice::SliceIndex,
};

pub struct RcArray<T> {
    refcount: *mut u32,
    elements: *const [T],
}

impl<T> RcArray<T> {
    unsafe fn from_raw(elements: *const [T]) -> Self {
        Self {
            refcount: Box::into_raw(Box::new(1)),
            elements,
        }
    }

    #[inline(always)]
    pub fn from_boxed(slice: Box<[T]>) -> Self {
        unsafe { Self::from_raw(Box::into_raw(slice)) }
    }

    pub fn slice(array: RcArray<T>, range: impl SliceIndex<[T], Output = [T]>) -> Self {
        Self {
            refcount: array.refcount,
            elements: unsafe { (*array.elements).index(range) },
        }
    }
}

impl<T> Deref for RcArray<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.elements }
    }
}

impl<T> Clone for RcArray<T> {
    fn clone(&self) -> Self {
        Self {
            refcount: unsafe {
                *self.refcount += 1;
                self.refcount
            },
            elements: self.elements,
        }
    }
}

impl<T> Drop for RcArray<T> {
    fn drop(&mut self) {
        if unsafe {
            *self.refcount -= 1;
            *self.refcount == 0
        } {
            drop(unsafe { Box::from_raw(self.elements as *mut [T]) });
        }
    }
}

impl<T: Debug> Debug for RcArray<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self[..], f)
    }
}
