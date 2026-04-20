#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::missing_transmute_annotations)]

pub use rasterize;
pub use util::math::I26Dot6;

pub mod srv3;
pub mod vtt;

mod capi;
pub mod config;
mod display;
mod html;
mod layout;
mod style;
mod text;
pub use config::Config;

mod renderer;
pub use renderer::{Renderer, SubtitleContext, Subtitles};

#[cfg(all(test, feature = "_layout_tests"))]
#[path = "../tests/layout/mod.rs"]
mod layout_tests;
