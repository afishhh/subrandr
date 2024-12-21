pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

pub trait Sealed {}

use std::{
    cmp::Ordering,
    fmt::Debug,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
};

mod rcarray;
pub use rcarray::*;

pub const unsafe fn array_assume_init_ref<const N: usize, T>(
    array: &[MaybeUninit<T>; N],
) -> &[T; N] {
    unsafe { &*(array as *const [_] as *const [T; N]) }
}

pub const unsafe fn slice_assume_init_ref<T>(slice: &[MaybeUninit<T>]) -> &[T] {
    unsafe { &*(slice as *const [_] as *const [T]) }
}

pub const unsafe fn slice_assume_init_mut<T>(slice: &mut [MaybeUninit<T>]) -> &mut [T] {
    unsafe { &mut *(slice as *mut [_] as *mut [T]) }
}

pub const fn ref_to_slice<T>(reference: &T) -> &[T; 1] {
    unsafe { std::mem::transmute(reference) }
}

pub fn rgb_to_hsl(r: u8, g: u8, b: u8) -> [f32; 3] {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h;
    let s;
    let l = (max + min) / 2.0;

    #[allow(clippy::collapsible_else_if)]
    if delta == 0.0 {
        h = 0.0;
        s = 0.0;
    } else {
        s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };
        let h_ = (max - r) / delta;

        h = if r == max {
            if g == b {
                5.0 + h_
            } else {
                1.0 - h_
            }
        } else if g == max {
            if b == r {
                1.0 + h_
            } else {
                3.0 - h_
            }
        } else {
            if r == g {
                3.0 + h_
            } else {
                5.0 - h_
            }
        } / 6.0;
    }

    [h, s, l]
}

pub fn hsl_to_rgb(h: f32, s: f32, l: f32) -> [u8; 3] {
    if s == 0.0 {
        [(l * 255.0) as u8; 3]
    } else {
        fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
            if t < 0.0 {
                t += 1.0;
            } else if t > 1.0 {
                t -= 1.0;
            }

            if t < (1.0 / 6.0) {
                p + (q - p) * 6.0 * t
            } else if t < (1.0 / 2.0) {
                q
            } else if t < (2.0 / 3.0) {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;

        [
            (hue_to_rgb(p, q, h + 1.0 / 3.0) * 255.0) as u8,
            (hue_to_rgb(p, q, h) * 255.0) as u8,
            (hue_to_rgb(p, q, h - 1.0 / 3.0) * 255.0) as u8,
        ]
    }
}

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

pub struct OrderedF32(pub f32);

impl PartialEq for OrderedF32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.total_cmp(&other.0) == Ordering::Equal
    }
}

impl Eq for OrderedF32 {}

impl PartialOrd for OrderedF32 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedF32 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
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
