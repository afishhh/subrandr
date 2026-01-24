use std::{
    cell::UnsafeCell,
    collections::HashSet,
    ffi::{c_char, c_void},
    hash::{Hash, Hasher},
    io::IsTerminal,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
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

fn log_default(level: Level, fmt: std::fmt::Arguments, source: &str) {
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

    let module_space = if source.is_empty() { "" } else { " " };
    eprintln!("[sbr {level_str}{module_space}{source}] {fmt}");
}

pub type CLogCallback =
    extern "C" fn(Level, *const c_char, usize, *const c_char, usize, *const c_void);

#[derive(Debug)]
pub enum MessageCallback {
    Default,
    C {
        callback: CLogCallback,
        user_data: *const c_void,
    },
}

unsafe impl Send for MessageCallback {}

impl MessageCallback {
    fn log(&self, level: Level, fmt: std::fmt::Arguments, source: &str) {
        const CRATE_MODULE_PREFIX: &str = "subrandr::";

        let module_rel = source.strip_prefix(CRATE_MODULE_PREFIX).unwrap_or(source);

        match self {
            Self::Default => {
                let filter = ENV_LOG_FILTER.get_or_init(|| parse_log_env_var().unwrap_or_default());
                if !filter.filter(level) {
                    return;
                }

                log_default(level, fmt, module_rel)
            }
            &Self::C {
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

mod sealed {
    pub trait Sealed {}
}

pub trait Logger: sealed::Sealed {
    fn log(&self, level: Level, fmt: std::fmt::Arguments, source: &str);
}

#[derive(Debug)]
struct RootLoggerImpl {
    callback: MessageCallback,
}

#[derive(Debug)]
pub struct RootLogger {
    root: Arc<Mutex<RootLoggerImpl>>,
}

impl RootLogger {
    pub fn new() -> Self {
        Self {
            root: Arc::new(Mutex::new(RootLoggerImpl {
                callback: MessageCallback::Default,
            })),
        }
    }

    pub fn set_message_callback(&mut self, callback: MessageCallback) {
        self.root.lock().unwrap().callback = callback;
    }
}

impl Logger for RootLogger {
    fn log(&self, level: Level, fmt: std::fmt::Arguments, module_path: &str) {
        self.root
            .lock()
            .unwrap()
            .callback
            .log(level, fmt, module_path)
    }
}

impl sealed::Sealed for RootLogger {}

pub trait AsLogger {
    fn as_logger(&self) -> &impl Logger;
}

impl<T: AsLogger> AsLogger for &T {
    fn as_logger(&self) -> &impl Logger {
        <T as AsLogger>::as_logger(*self)
    }
}

impl<T: AsLogger> AsLogger for &mut T {
    fn as_logger(&self) -> &impl Logger {
        <T as AsLogger>::as_logger(*self)
    }
}

impl AsLogger for RootLogger {
    fn as_logger(&self) -> &impl Logger {
        self
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

#[macro_export]
macro_rules! log {
    ($logger: expr, $level: expr, once($set: expr $(, $value: expr)?), $($fmt: tt)*) => {{
        let set = &$set;
        let value = $crate::log!(@logset_value $($value)?);
        if !set.0.contains(set.1 , value) {
            $crate::log!($logger, $level, $($fmt)*);
            set.0.insert(set.1, value);
        }
    }};
    ($logger: expr, $level: expr, $($fmt: tt)*) => {
        $crate::Logger::log(
            $crate::AsLogger::as_logger(&$logger),
            $level, format_args!($($fmt)*), module_path!()
        )
    };
    (@logset_value $value: expr) => { $value };
    (@logset_value) => { () };
    (@mkmacro $dollar: tt, $name: ident, $level: ident) => {
        #[macro_export]
        #[clippy::format_args]
        macro_rules! $name {
            ($dollar logger: expr, $dollar ($dollar rest: tt)*) => {
                $crate::log!($dollar logger, $crate::Level::$level, $dollar ($dollar rest)*)
            }
        }
    }
}

#[macro_export]
macro_rules! log_once_state {
    (@mkkey $set: ident $ident: ident $(, $($rest: tt)*)?) => {
        let $ident = {
            #[derive(Clone, Copy)]
            struct K; impl $crate::LogOnceKey for K {}
            $crate::LogOnceRef(
                &$set,
                K,
            )
        };
        $($crate::log_once_state!(@mkkey $set $($rest)*))?
    };
    (@mkkey $($rest: tt)*) => {
        compile_error!("log_once_state: invalid syntax")
    };
    (in $set: expr; $($tokens: tt)*) => {
        let set = $set;
        $crate::log_once_state!(@mkkey set $($tokens)*)
    };
    ($($tokens: tt)*) => {
        $crate::log_once_state!(in $crate::LogOnceSet::new(); $($tokens)*)
    };
}

log!(@mkmacro $, trace, Trace);
log!(@mkmacro $, debug, Debug);
log!(@mkmacro $, warning, Warn);
log!(@mkmacro $, info, Info);
log!(@mkmacro $, error, Error);
