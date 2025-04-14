//! An implementation of some concepts from [css-syntax].
//!
//! This includes the tokenizer and select parser algorithms.
//!
//! This module currently also implements a [`ParseStream`] abstraction for parsing
//! CSS values but that should probably be moved out along with [`ParseError`].
//!
//! [css-syntax]: https://drafts.csswg.org/css-syntax-3/
//! [`ParseStream`]: parse_stream::ParseStream

pub mod algorithms;
pub mod buffer;
pub mod error;
pub mod parse_stream;
pub mod peek;
pub mod token;
mod tokenizer;

#[cfg_attr(not(test), expect(unused_imports))]
pub use buffer::TokenBuffer;
pub use error::ParseError;
pub use peek::Peek;
pub use token::{Span, Spanned};
