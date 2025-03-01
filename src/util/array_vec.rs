use std::{
    fmt::Debug,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

use super::{slice_assume_init_mut, slice_assume_init_ref};

// TODO: allow customizing length size
pub struct ArrayVec<const CAP: usize, T> {
    data: [MaybeUninit<T>; CAP],
    length: usize,
}

impl<const CAP: usize, T> ArrayVec<CAP, T> {
    pub const fn new() -> Self {
        Self {
            data: [const { MaybeUninit::uninit() }; CAP],
            length: 0,
        }
    }

    pub fn from_array<const N: usize>(array: [T; N]) -> Self {
        assert!(N <= CAP);

        let mut result = Self::new();
        for value in array {
            result.push(value);
        }
        result
    }

    pub const fn push(&mut self, value: T) {
        self.data[self.length].write(value);
        self.length += 1;
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.length == 0 {
            None
        } else {
            self.length -= 1;
            Some(unsafe { self.data[self.length].assume_init_read() })
        }
    }

    pub fn insert(&mut self, index: usize, value: T) {
        assert!(index <= self.len());
        assert!(self.length < CAP);

        unsafe {
            let ptr = self.data.as_mut_ptr();
            std::ptr::copy(ptr.add(index), ptr.add(index + 1), self.length - index);
            self.data.get_unchecked_mut(index).write(value);
            self.length += 1;
        }
    }

    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len());

        unsafe {
            let result = self.data[index].assume_init_read();

            let ptr = self.data.as_mut_ptr();
            std::ptr::copy(ptr.add(index + 1), ptr.add(index), self.length - index - 1);
            self.length -= 1;

            result
        }
    }

    pub fn as_slice(&self) -> &[T] {
        unsafe { slice_assume_init_ref(&self.data[..self.length]) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice_assume_init_mut(&mut self.data[..self.length]) }
    }

    pub const fn len(&self) -> usize {
        self.length
    }

    pub const unsafe fn set_len(&mut self, len: usize) {
        self.length = len;
    }
}

impl<const CAP: usize, T: Clone> ArrayVec<CAP, T> {
    pub fn from_slice(slice: &[T]) -> Self {
        assert!(slice.len() <= CAP, "slice is larger than ArrayVec capacity");

        let mut result = Self::new();
        for value in slice {
            result.push(value.clone());
        }
        result
    }

    pub fn extend_from_slice(&mut self, slice: &[T]) {
        assert!(CAP - self.length > slice.len());

        for element in slice {
            self.push(element.clone());
        }
    }
}

impl<const CAP: usize, T> IntoIterator for ArrayVec<CAP, T> {
    type Item = T;
    type IntoIter = std::iter::Map<
        std::iter::Take<<[MaybeUninit<T>; CAP] as IntoIterator>::IntoIter>,
        // sadly this requires a function pointer :(((
        // I wonder whether this can be optimised out...
        fn(MaybeUninit<T>) -> T,
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.data
            .into_iter()
            .take(self.length)
            .map(|x| unsafe { MaybeUninit::assume_init(x) })
    }
}

impl<const CAP: usize, T: Clone> Clone for ArrayVec<CAP, T> {
    fn clone(&self) -> Self {
        let mut result = Self {
            data: [const { MaybeUninit::uninit() }; CAP],
            length: self.length,
        };

        for (i, value) in self.iter().enumerate() {
            result.data[i].write(value.clone());
        }

        result
    }
}

impl<const CAP: usize, T: Copy> Copy for ArrayVec<CAP, T> {}

impl<const CAP: usize, T> Default for ArrayVec<CAP, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize, T> Deref for ArrayVec<CAP, T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: usize, T> DerefMut for ArrayVec<CAP, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<const CAP: usize, T: Debug> Debug for ArrayVec<CAP, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ArrayVec<{CAP}> ")?;
        let mut list = f.debug_list();
        for value in self.iter() {
            list.entry(value);
        }
        list.finish()
    }
}
