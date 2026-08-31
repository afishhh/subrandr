use std::{borrow::Borrow, hash::Hash, mem::MaybeUninit, ops::Deref, ptr::NonNull};

pub mod cache;
pub mod math;
pub mod rc;
pub mod rev_if;

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

pub fn vec_into_parts<T>(mut v: Vec<T>) -> (*mut T, usize, usize) {
    let parts = vec_parts(&mut v);
    std::mem::forget(v);
    parts
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
        Self(NonNull::from(Box::leak(Box::new(value))))
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
        Self(NonNull::from(Box::leak(value)))
    }
}

impl<T: ?Sized> Drop for ReadonlyAliasableBox<T> {
    fn drop(&mut self) {
        unsafe { _ = Box::from_raw(self.0.as_ptr()) };
    }
}

fn binary_unit_prefix(quantity: usize) -> (usize, &'static str) {
    const TABLE: &[&str] = &["", "Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];

    let k = (usize::BITS - quantity.leading_zeros()).saturating_sub(1) / 10;
    (1 << (k * 10), TABLE[k as usize])
}

pub struct HumanSize(pub usize);

impl std::fmt::Display for HumanSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (divisor, prefix) = binary_unit_prefix(self.0);
        write!(
            f,
            "{:.3}{prefix}B",
            if divisor > 1 {
                (self.0 / (divisor >> 10)) as f32 / 1024.0
            } else {
                self.0 as f32
            }
        )
    }
}

#[cfg(test)]
mod test {
    use super::{binary_unit_prefix, HumanSize, ReadonlyAliasableBox};

    #[test]
    fn readonly_aliasable_box() {
        _ = ReadonlyAliasableBox::new(String::from("hello"));
        let boxed = ReadonlyAliasableBox::new(String::from("second"));
        let aliasing = boxed.as_ptr();
        _ = boxed;
        _ = aliasing;
    }

    fn check_binary_unit(size: usize, exp_div: usize, exp_suffix: &str) {
        assert_eq!(binary_unit_prefix(size), (exp_div, exp_suffix));
    }

    // Make sure not to break this on 32-bit by running it on a 32-bit miri target.
    #[test]
    fn binary_unit() {
        const KB: usize = 1024;
        const MB: usize = KB * 1024;
        const GB: usize = MB * 1024;
        #[cfg(target_pointer_width = "64")]
        const TB: usize = GB * 1024;
        #[cfg(target_pointer_width = "64")]
        const PB: usize = TB * 1024;
        #[cfg(target_pointer_width = "64")]
        const EB: usize = PB * 1024;

        binary_unit_prefix(usize::MAX);

        check_binary_unit(0, 1, "");
        check_binary_unit(1023, 1, "");
        check_binary_unit(KB + 1, KB, "Ki");
        check_binary_unit(MB, MB, "Mi");
        check_binary_unit(1749685123, GB, "Gi");
        #[cfg(target_pointer_width = "64")]
        check_binary_unit(1000 * PB, PB, "Pi");
        #[cfg(target_pointer_width = "64")]
        check_binary_unit(usize::MAX, EB, "Ei");
    }

    #[test]
    fn human_size() {
        assert_eq!(format!("{}", HumanSize(0)), "0.000B");
        assert_eq!(format!("{}", HumanSize(42)), "42.000B");
        assert_eq!(format!("{}", HumanSize(1040)), "1.016KiB");
        #[cfg(target_pointer_width = "64")]
        assert_eq!(format!("{}", HumanSize(534900675635793436)), "475.087PiB");
        #[cfg(target_pointer_width = "64")]
        assert_eq!(format!("{}", HumanSize(usize::MAX)), "15.999EiB");
    }
}
