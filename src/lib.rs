#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::missing_transmute_annotations)]

use std::{cell::Cell, fmt::Debug};

use log::Logger;

pub mod srv3;
pub mod vtt;

mod capi;
mod color;
mod html;
mod layout;
mod log;
mod math;
mod outline;
pub mod rasterize;
mod style;
mod text;
mod util;

pub use math::I26Dot6;

#[derive(Default, Debug, Clone)]
struct DebugFlags {
    draw_version_string: bool,
    draw_perf_info: bool,
    draw_layout_info: bool,
}

impl DebugFlags {
    fn from_env() -> Self {
        let mut result = Self::default();

        if let Ok(s) = std::env::var("SBR_DEBUG") {
            for token in s.split(",") {
                match token {
                    "draw_version" => result.draw_version_string = true,
                    "draw_perf" => result.draw_perf_info = true,
                    "draw_layout" => result.draw_layout_info = true,
                    _ => (),
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
