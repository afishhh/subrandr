use std::{borrow::Borrow, hash::Hash, mem::MaybeUninit, ops::Deref, ptr::NonNull};

pub mod cache;
pub mod math;
pub mod rc;

pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

/// Asserts that the entirety of `slice` is initialized and returns a mutable slice
/// of the initialized contents.
///
/// # Safety
///
/// `slice` must be fully initialized.
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

#[derive(Debug)]
pub struct ReadonlyAliasableBox<T: ?Sized>(NonNull<T>);

impl<T: ?Sized> ReadonlyAliasableBox<T> {
    pub fn new(value: T) -> Self
    where
        T: Sized,
    {
        Self(unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(value))) })
    }

    pub fn as_nonnull(this: &Self) -> NonNull<T> {
        this.0
    }
}

impl<T: Hash + ?Sized> Hash for ReadonlyAliasableBox<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state);
    }
}

impl<T: PartialEq + ?Sized> PartialEq for ReadonlyAliasableBox<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_ref() == other.as_ref()
    }
}

impl<T: Eq + ?Sized> Eq for ReadonlyAliasableBox<T> {}

impl<T: ?Sized> Deref for ReadonlyAliasableBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> Borrow<T> for ReadonlyAliasableBox<T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: ?Sized> AsRef<T> for ReadonlyAliasableBox<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: ?Sized> From<Box<T>> for ReadonlyAliasableBox<T> {
    fn from(value: Box<T>) -> Self {
        unsafe { Self(NonNull::new_unchecked(Box::into_raw(value))) }
    }
}

impl<T: ?Sized> Drop for ReadonlyAliasableBox<T> {
    fn drop(&mut self) {
        unsafe { _ = Box::from_raw(self.0.as_ptr()) };
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HashF32(f32);

impl HashF32 {
    pub fn new(value: f32) -> Self {
        Self(if value == 0.0 {
            // normalize negative zero
            0.0
        } else if value.is_nan() {
            // normalize NaN
            f32::NAN
        } else {
            value
        })
    }

    pub fn to_inner(self) -> f32 {
        self.0
    }
}

impl PartialEq for HashF32 {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for HashF32 {}

impl Hash for HashF32 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

pub fn human_size_suffix(size: usize) -> (usize, &'static str) {
    const TABLE: &[&str] = &["", "Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];

    let mut current_pow = 1;
    let mut next_pow = 1024;
    let mut current_idx = 0;
    while next_pow <= size && current_idx < TABLE.len() - 1 {
        current_pow = next_pow;
        current_idx += 1;
        next_pow = match next_pow.checked_mul(1024) {
            Some(next_pow) => next_pow,
            None => break,
        };
    }

    (current_pow, TABLE[current_idx])
}

#[cfg(test)]
mod test {
    use super::{human_size_suffix, ReadonlyAliasableBox};

    #[test]
    fn readonly_aliasable_box() {
        _ = ReadonlyAliasableBox::new(String::from("hello"));
        let boxed = ReadonlyAliasableBox::new(String::from("second"));
        let aliasing = boxed.as_ptr();
        _ = boxed;
        _ = aliasing;
    }

    fn human_size_one(size: usize, exp_div: usize, exp_suffix: &str) {
        assert_eq!(human_size_suffix(size), (exp_div, exp_suffix));
    }

    // Make sure not to break this on 32-bit by running it on a 32-bit miri target.
    #[test]
    fn human_size() {
        const KB: usize = 1024;
        const MB: usize = KB * 1024;
        const GB: usize = MB * 1024;
        #[cfg(target_pointer_width = "64")]
        const TB: usize = GB * 1024;
        #[cfg(target_pointer_width = "64")]
        const PB: usize = TB * 1024;
        #[cfg(target_pointer_width = "64")]
        const EB: usize = PB * 1024;

        human_size_one(0, 1, "");
        human_size_one(1023, 1, "");
        human_size_one(KB + 1, KB, "Ki");
        human_size_one(MB, MB, "Mi");
        human_size_one(1749685123, GB, "Gi");
        #[cfg(target_pointer_width = "64")]
        human_size_one(1000 * PB, PB, "Pi");
        #[cfg(target_pointer_width = "64")]
        human_size_one(5 * EB, EB, "Ei");
    }
}
