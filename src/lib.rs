#![allow(dead_code)] // tmp
#![allow(clippy::too_many_arguments)]
#![allow(clippy::type_complexity)]
#![allow(clippy::missing_transmute_annotations)]

use std::{cell::Cell, fmt::Debug};

use log::Logger;

pub mod srv3;
pub mod vtt;

mod capi;
mod color;
mod css;
mod html;
mod log;
mod math;
mod miniweb;
mod outline;
pub mod rasterize;
mod text;
mod util;

pub use math::I26Dot6;

#[derive(Default, Debug, Clone)]
struct DebugFlags {
    draw_version_string: bool,
    draw_perf_info: bool,
    draw_layout_info: bool,
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
                    "draw_layout" => result.draw_layout_info = true,
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
