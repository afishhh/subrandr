#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::missing_transmute_annotations)]

use std::{cell::Cell, fmt::Debug};

pub use rasterize;
pub use util::math::I26Dot6;

use log::Logger;

pub mod srv3;
pub mod vtt;

mod capi;
mod display;
mod html;
mod layout;
mod log;
mod style;
mod text;

#[derive(Default, Debug, Clone)]
struct DebugFlags {
    draw_version_string: bool,
    draw_perf_info: bool,
    dpi_override: Option<u32>,
}

impl DebugFlags {
    fn from_env() -> Self {
        let mut result = Self::default();

        if let Ok(s) = std::env::var("SBR_DEBUG") {
            for token in s.split(",") {
                match token {
                    "draw_version" => result.draw_version_string = true,
                    "draw_perf" => result.draw_perf_info = true,
                    #[allow(clippy::single_match)]
                    _ => match token.split_once("=") {
                        Some(("override_dpi", value_str)) => {
                            if let Ok(value) = value_str.parse::<u32>() {
                                result.dpi_override = Some(value)
                            }
                        }
                        _ => (),
                    },
                }
            }
        }

        result
    }
}

#[derive(Debug)]
pub struct Subrandr {
    logger: log::Logger,
    did_log_version: Cell<bool>,
    debug: DebugFlags,
}

impl Subrandr {
    pub fn init() -> Self {
        Self {
            logger: log::Logger::Default,
            did_log_version: Cell::new(false),
            debug: DebugFlags::from_env(),
        }
    }
}

// allows for convenient logging with log!(sbr, ...)
impl log::AsLogger for Subrandr {
    fn as_logger(&self) -> &Logger {
        &self.logger
    }
}

mod renderer;
pub use renderer::{Renderer, SubtitleContext, Subtitles};

#[cfg(all(test, feature = "_layout_tests"))]
#[path = "../tests/layout/mod.rs"]
mod layout_tests;
