use std::{
    ffi::{c_char, c_void},
    panic::Location,
    str::FromStr,
    sync::OnceLock,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

pub enum Logger {
    Default,
    C {
        fun: extern "C" fn(Level, *const c_char, usize, *const c_void),
        user_data: *const c_void,
    },
    // possible future variant: rust's log crate for rust projects
}

pub trait AsLogger {
    fn as_logger(&self) -> &Logger;
}

impl<T: AsLogger> AsLogger for &T {
    fn as_logger(&self) -> &Logger {
        <T as AsLogger>::as_logger(*self)
    }
}

impl AsLogger for Logger {
    fn as_logger(&self) -> &Logger {
        self
    }
}

impl Logger {
    #[track_caller]
    pub fn log(&self, level: Level, fmt: std::fmt::Arguments, module_path: &'static str) {
        let filter = ENV_LOG_FILTER.get_or_init(|| parse_log_env_var().unwrap_or_default());
        if !filter.filter(level) {
            return;
        }

        match self {
            Logger::Default => {
                log_default(level, fmt, core::panic::Location::caller(), module_path)
            }
            &Logger::C { fun, user_data } => {
                if let Some(literal) = fmt.as_str() {
                    fun(
                        level,
                        literal.as_ptr() as *const i8,
                        literal.len(),
                        user_data,
                    )
                } else {
                    let string = fmt.to_string();
                    fun(level, string.as_ptr() as *const i8, string.len(), user_data)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum LevelFilter {
    Level(Level),
    None,
}

impl LevelFilter {
    fn filter(self, level: Level) -> bool {
        match self {
            LevelFilter::Level(filter) => level >= filter,
            LevelFilter::None => false,
        }
    }
}

impl FromStr for LevelFilter {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "trace" => Self::Level(Level::Trace),
            "debug" => Self::Level(Level::Debug),
            "info" => Self::Level(Level::Info),
            "warn" => Self::Level(Level::Warn),
            "error" => Self::Level(Level::Error),
            "none" => Self::None,
            _ => return Err(()),
        })
    }
}

struct LogFilter {
    top_level: LevelFilter,
}

impl LogFilter {
    fn filter(&self, level: Level) -> bool {
        self.top_level.filter(level)
    }
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            #[cfg(not(debug_assertions))]
            top_level: LevelFilter::Level(Level::Warn),
            #[cfg(debug_assertions)]
            top_level: LevelFilter::Level(Level::Debug),
        }
    }
}

fn parse_log_env_var() -> Option<LogFilter> {
    let text = std::env::var("SBR_LOG").ok()?;

    Some(LogFilter {
        top_level: text.parse().ok()?,
    })
}

static ENV_LOG_FILTER: OnceLock<LogFilter> = OnceLock::new();

fn log_default(
    level: Level,
    fmt: std::fmt::Arguments,
    _location: &Location<'static>,
    module_path: &'static str,
) {
    // TODO: check if tty, disable on windows
    let level_str = match level {
        Level::Trace => "\x1b[1;37mtrace\x1b[0m",
        Level::Debug => "\x1b[1;35mdebug\x1b[0m",
        Level::Info => "\x1b[1;34m info\x1b[0m",
        Level::Warn => "\x1b[1;33m warn\x1b[0m",
        Level::Error => "\x1b[1;31merror\x1b[0m",
    };

    let module_rel = module_path
        .strip_prefix("subrandr::")
        .or_else(|| module_path.strip_prefix("subrandr"))
        .unwrap_or(module_path);
    let module_space = if module_rel.is_empty() { "" } else { " " };
    eprintln!("[sbr {level_str}{module_space}{module_rel}] {fmt}");
}

macro_rules! log {
    ($logger: expr, $level: expr, once_set($set: expr, $value: expr), $($fmt: tt)*) => {
        let set = &mut $set;
        let value = $value;
        if !set.contains(value) {
            $crate::log::log!($logger, $level, $($fmt)*);
            set.insert(value.to_owned());
        }
    };
    ($logger: expr, $level: expr, $($fmt: tt)*) => {
        $crate::log::AsLogger::as_logger(&$logger).log($level, format_args!($($fmt)*), module_path!())
    };
    (@mkmacro $dollar: tt, $name: ident, $level: ident) => {
        macro_rules! $name {
            ($dollar logger: expr, $dollar ($dollar rest: tt)*) => {
                $crate::log::log!($dollar logger, $crate::log::Level::$level, $dollar ($dollar rest)*)
            }
        }
    }
}

macro_rules! log_once_state {
    ($ident: ident: set $(, $($rest: tt)*)?) => {
        let mut $ident = std::collections::HashSet::new();
        $($crate::log::log_once_state!($($rest)*))?
    };
}

pub(crate) use {log, log_once_state};

#[cfg(debug_assertions)]
log!(@mkmacro $, trace, Trace);
#[cfg(not(debug_assertions))]
macro_rules! trace {
    ($($anything: tt)*) => {};
}

log!(@mkmacro $, debug, Debug);
log!(@mkmacro $, warning, Warn);
log!(@mkmacro $, info, Info);
log!(@mkmacro $, error, Error);

#[rustfmt::skip]
pub(crate) use {trace, debug, warning, info, error};
