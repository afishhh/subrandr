pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

pub trait Sealed {}

use std::{
    borrow::Borrow, cmp::Ordering, fmt::Debug, hash::Hash, mem::MaybeUninit, ops::Deref,
    ptr::NonNull,
};

mod rcarray;
pub use rcarray::*;
#[expect(dead_code)]
mod array_vec;
#[expect(unused_imports)]
pub use array_vec::ArrayVec;

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

pub fn vec_parts<T>(v: &mut Vec<T>) -> (*mut T, usize, usize) {
    let ptr = v.as_mut_ptr();
    let len = v.len();
    let capacity = v.capacity();
    (ptr, len, capacity)
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

#[derive(Debug, Clone)]
#[repr(transparent)]
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

impl Hash for OrderedF32 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.0.to_bits());
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
