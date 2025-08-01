use std::{
    cell::UnsafeCell,
    collections::HashSet,
    ffi::{c_char, c_void},
    hash::{Hash, Hasher},
    io::IsTerminal,
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

pub type CLogCallback =
    extern "C" fn(Level, *const c_char, usize, *const c_char, usize, *const c_void);

#[derive(Debug)]
pub enum Logger {
    Default,
    C {
        callback: CLogCallback,
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
    pub fn log(&self, level: Level, fmt: std::fmt::Arguments, module_path: &'static str) {
        const CRATE_MODULE_PREFIX: &str = concat!(env!("CARGO_PKG_NAME"), "::");

        let module_rel = module_path
            .strip_prefix(CRATE_MODULE_PREFIX)
            .unwrap_or(module_path);

        match self {
            Logger::Default => {
                let filter = ENV_LOG_FILTER.get_or_init(|| parse_log_env_var().unwrap_or_default());
                if !filter.filter(level) {
                    return;
                }

                log_default(level, fmt, module_rel)
            }
            &Logger::C {
                callback,
                user_data,
            } => {
                if let Some(literal) = fmt.as_str() {
                    callback(
                        level,
                        module_rel.as_ptr().cast(),
                        module_rel.len(),
                        literal.as_ptr().cast(),
                        literal.len(),
                        user_data,
                    )
                } else {
                    let string = fmt.to_string();
                    callback(
                        level,
                        module_rel.as_ptr().cast(),
                        module_rel.len(),
                        string.as_ptr().cast(),
                        string.len(),
                        user_data,
                    )
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

fn log_default(level: Level, fmt: std::fmt::Arguments, module_path: &'static str) {
    let level_str = if std::io::stderr().is_terminal() {
        match level {
            Level::Trace => "\x1b[1;37mtrace\x1b[0m",
            Level::Debug => "\x1b[1;35mdebug\x1b[0m",
            Level::Info => "\x1b[1;34m info\x1b[0m",
            Level::Warn => "\x1b[1;33m warn\x1b[0m",
            Level::Error => "\x1b[1;31merror\x1b[0m",
        }
    } else {
        match level {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => " info",
            Level::Warn => " warn",
            Level::Error => "error",
        }
    };

    let module_space = if module_path.is_empty() { "" } else { " " };
    eprintln!("[sbr {level_str}{module_space}{module_path}] {fmt}");
}

#[doc(hidden)]
pub trait LogOnceKey: Sized + 'static {}

// Allows us to use one hashset instead of many
// hashsets for each type of event
#[doc(hidden)]
pub struct LogOnceSet {
    items: UnsafeCell<HashSet<(std::any::TypeId, u64)>>,
}

impl LogOnceSet {
    pub fn new() -> Self {
        Self {
            items: UnsafeCell::new(HashSet::new()),
        }
    }

    fn hash<V: Hash>(&self, value: V) -> u64 {
        let mut hasher = std::hash::DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    pub fn insert<K: LogOnceKey, V: Hash>(&self, _key: K, value: V) {
        unsafe { &mut *self.items.get() }.insert((std::any::TypeId::of::<K>(), self.hash(value)));
    }

    pub fn contains<K: LogOnceKey, V: Hash>(&self, _key: K, value: V) -> bool {
        unsafe { &*self.items.get() }.contains(&(std::any::TypeId::of::<K>(), self.hash(value)))
    }
}

#[doc(hidden)]
pub struct LogOnceRef<'a, K: LogOnceKey>(pub &'a LogOnceSet, pub K);

macro_rules! log {
    ($logger: expr, $level: expr, once($set: expr $(, $value: expr)?), $($fmt: tt)*) => {{
        let set = &$set;
        let value = $crate::log::log!(@logset_value $($value)?);
        if !set.0.contains(set.1 , value) {
            $crate::log::log!($logger, $level, $($fmt)*);
            set.0.insert(set.1, value);
        }
    }};
    ($logger: expr, $level: expr, $($fmt: tt)*) => {
        $crate::log::AsLogger::as_logger(&$logger).log($level, format_args!($($fmt)*), module_path!())
    };
    (@logset_value $value: expr) => { $value };
    (@logset_value) => { () };
    (@mkmacro $dollar: tt, $name: ident, $level: ident) => {
        #[allow(unused_macros)]
        #[clippy::format_args]
        macro_rules! $name {
            ($dollar logger: expr, $dollar ($dollar rest: tt)*) => {
                $crate::log::log!($dollar logger, $crate::log::Level::$level, $dollar ($dollar rest)*)
            }
        }
    }
}

macro_rules! log_once_state {
    (@mkkey $set: ident $ident: ident $(, $($rest: tt)*)?) => {
        let $ident = {
            #[derive(Clone, Copy)]
            struct K; impl $crate::log::LogOnceKey for K {}
            $crate::log::LogOnceRef(
                &$set,
                K,
            )
        };
        $($crate::log::log_once_state!(@mkkey $set $($rest)*))?
    };
    (@mkkey $($rest: tt)*) => {
        compile_error!("log_once_state: invalid syntax")
    };
    (in $set: expr; $($tokens: tt)*) => {
        let set = $set;
        $crate::log::log_once_state!(@mkkey set $($tokens)*)
    };
    ($($tokens: tt)*) => {
        $crate::log::log_once_state!(in $crate::log::LogOnceSet::new(); $($tokens)*)
    };
}

pub(crate) use {log, log_once_state};

log!(@mkmacro $, trace, Trace);
log!(@mkmacro $, debug, Debug);
log!(@mkmacro $, warning, Warn);
log!(@mkmacro $, info, Info);
log!(@mkmacro $, error, Error);

#[rustfmt::skip]
#[allow(unused_imports, clippy::single_component_path_imports)]
pub(crate) use {trace, debug, warning, info, error};
