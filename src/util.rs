pub type AnyError = Box<dyn std::error::Error + Send + Sync>;

pub trait Sealed {}

pub mod math;
pub use math::*;
mod rcarray;
pub use rcarray::*;

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

pub fn fmt_from_fn<F>(f: F) -> FormatterFn<F>
where
    F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result,
{
    FormatterFn(f)
}
