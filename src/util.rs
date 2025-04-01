pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

pub trait Sealed {}

use std::{cmp::Ordering, fmt::Debug, hash::Hash, mem::MaybeUninit, ops::Range};

mod rcarray;
pub use rcarray::*;
pub mod array_vec;
#[cfg_attr(not(test), expect(unused_imports))]
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

pub struct BlitRectangle {
    pub xs: Range<usize>,
    pub ys: Range<usize>,
}

pub fn calculate_blit_rectangle(
    x: i32,
    y: i32,
    target_width: usize,
    target_height: usize,
    source_width: usize,
    source_height: usize,
) -> Option<BlitRectangle> {
    let isx = if x < 0 { (-x) as usize } else { 0 };
    let isy = if y < 0 { (-y) as usize } else { 0 };
    let msx = (source_width as i32).min(target_width as i32 - x);
    let msy = (source_height as i32).min(target_height as i32 - y);
    if msx <= 0 || msy <= 0 {
        return None;
    }
    let msx = msx as usize;
    let msy = msy as usize;

    Some(BlitRectangle {
        xs: isx..msx,
        ys: isy..msy,
    })
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
