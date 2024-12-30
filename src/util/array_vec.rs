use std::{
    fmt::Debug,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

use super::{slice_assume_init_mut, slice_assume_init_ref};

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

    pub fn as_slice(&self) -> &[T] {
        unsafe { slice_assume_init_ref(&self.data[..self.length]) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        unsafe { slice_assume_init_mut(&mut self.data[..self.length]) }
    }

    pub const fn len(&self) -> usize {
        self.length
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
