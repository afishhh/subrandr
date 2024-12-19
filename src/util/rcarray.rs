use std::{
    fmt::Debug,
    ops::{Deref, Range, RangeBounds},
};

pub struct RcArray<T> {
    refcount: *mut u32,
    elements: *const [T],
    range: Range<usize>,
}

impl<T> RcArray<T> {
    unsafe fn from_raw(elements: *const [T]) -> Self {
        Self {
            refcount: Box::into_raw(Box::new(1)),
            elements,
            range: 0..elements.len(),
        }
    }

    #[inline(always)]
    pub fn from_boxed(slice: Box<[T]>) -> Self {
        unsafe { Self::from_raw(Box::into_raw(slice)) }
    }

    pub fn slice(array: Self, range: impl RangeBounds<usize>) -> Self {
        // TODO: std::slice::range here
        let array = std::mem::ManuallyDrop::new(array);
        Self {
            refcount: array.refcount,
            elements: array.elements,
            range: match range.start_bound() {
                std::ops::Bound::Included(i) => array.range.start + *i,
                std::ops::Bound::Excluded(i) => array.range.start + *i + 1,
                std::ops::Bound::Unbounded => array.range.start,
            }..match range.end_bound() {
                std::ops::Bound::Included(i) => array.range.start + *i + 1,
                std::ops::Bound::Excluded(i) => array.range.start + *i,
                std::ops::Bound::Unbounded => array.range.end,
            },
        }
    }
}

impl<T> Deref for RcArray<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        unsafe { &(*self.elements)[self.range.clone()] }
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
            range: self.range.clone(),
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
