use std::{
    cell::{Cell, UnsafeCell},
    collections::{HashMap, HashSet},
    ffi::{c_char, c_void},
    hash::{Hash, Hasher},
    io::IsTerminal,
    marker::PhantomData,
    num::NonZero,
    str::FromStr,
    sync::{Arc, Mutex, OnceLock},
};

struct Indent(u32);

impl std::fmt::Display for Indent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for _ in 0..self.0 {
            f.write_str(" ")?;
        }

        Ok(())
    }
}

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
    fn log(&self, level: Level, fmt: std::fmt::Arguments, mut source: &str) {
        const STRIPPED_PREFIXES: &[&str] = &["subrandr::", "sbr_"];
        for &pref in STRIPPED_PREFIXES {
            source = source.strip_prefix(pref).unwrap_or(source);
        }

        match self {
            Self::Default => {
                let filter = ENV_LOG_FILTER.get_or_init(|| parse_log_env_var().unwrap_or_default());
                if !filter.filter(level) {
                    return;
                }

                log_default(level, fmt, source)
            }
            &Self::C {
                callback,
                user_data,
            } => {
                if let Some(literal) = fmt.as_str() {
                    callback(
                        level,
                        source.as_ptr().cast(),
                        source.len(),
                        literal.as_ptr().cast(),
                        literal.len(),
                        user_data,
                    )
                } else {
                    let string = fmt.to_string();
                    callback(
                        level,
                        source.as_ptr().cast(),
                        source.len(),
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
    const SUPPORTS_SPANS: bool;

    fn log(&self, level: Level, fmt: std::fmt::Arguments, source: &str);
    fn span(&self, level: Level, fmt: std::fmt::Arguments, source: &str) -> EnteredSpan<'_> {
        _ = level;
        _ = fmt;
        _ = source;
        EnteredSpan::_inactive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SpanId(NonZero<u32>);

impl SpanId {
    const ONE: Self = Self(NonZero::new(1).unwrap());

    fn next(self) -> Self {
        NonZero::new(self.0.get().wrapping_add(1)).map_or(Self::ONE, Self)
    }
}

#[derive(Debug)]
struct SpanState {
    start: std::time::Instant,
    level: Level,
    source: Box<str>,
    parent: Option<SpanId>,
    refcnt: NonZero<u32>,
}

#[derive(Debug)]
struct RootLoggerImpl {
    callback: MessageCallback,
    spans: HashMap<SpanId, SpanState>,
    next_span_id: SpanId,
}

impl Logger for RootLoggerImpl {
    const SUPPORTS_SPANS: bool = false;

    fn log(&self, level: Level, fmt: std::fmt::Arguments, module_path: &str) {
        self.callback.log(level, fmt, module_path)
    }
}

impl sealed::Sealed for RootLoggerImpl {}

#[derive(Debug)]
pub struct RootLogger {
    root: Arc<Mutex<RootLoggerImpl>>,
}

impl RootLogger {
    pub fn new() -> Self {
        Self {
            root: Arc::new(Mutex::new(RootLoggerImpl {
                callback: MessageCallback::Default,
                spans: HashMap::new(),
                next_span_id: SpanId::ONE,
            })),
        }
    }

    pub fn set_message_callback(&mut self, callback: MessageCallback) {
        self.root.lock().unwrap().callback = callback;
    }

    pub fn new_ctx(&self) -> LogContext {
        LogContext {
            root: self.root.clone(),
            current_span: Cell::new(None),
            current_depth: Cell::new(0),
        }
    }
}

impl Logger for RootLogger {
    const SUPPORTS_SPANS: bool = false;

    fn log(&self, level: Level, fmt: std::fmt::Arguments, source: &str) {
        self.root.lock().unwrap().callback.log(level, fmt, source)
    }
}

impl AsLogger for RootLoggerImpl {
    fn as_logger(&self) -> &impl Logger {
        self
    }
}

impl sealed::Sealed for RootLogger {}

impl Drop for RootLogger {
    fn drop(&mut self) {
        let strong = Arc::strong_count(&self.root);
        let weak = Arc::weak_count(&self.root);
        if strong != 1 || weak != 0 {
            warn!(
                self,
                "Logger dropped with unexpected references! strong={strong} weak={weak}"
            )
        }
    }
}

impl RootLoggerImpl {
    fn insert_new_span(
        &mut self,
        start: std::time::Instant,
        level: Level,
        source: Box<str>,
        parent: Option<SpanId>,
    ) -> Option<SpanId> {
        let id = self.next_span_id;
        self.next_span_id = id.next();
        match self.spans.entry(id) {
            std::collections::hash_map::Entry::Occupied(_) => {
                warn!(self, "Logger span id wrapped around to {id:?} and encountered live span, something is leaking spans!");
                None
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(SpanState {
                    start,
                    level,
                    source,
                    parent,
                    refcnt: const { NonZero::new(1).unwrap() },
                });
                Some(id)
            }
        }
    }
}

#[derive(Debug)]
pub struct LogContext {
    root: Arc<Mutex<RootLoggerImpl>>,
    current_span: Cell<Option<SpanId>>,
    current_depth: Cell<u32>,
}

impl Logger for LogContext {
    const SUPPORTS_SPANS: bool = true;

    fn log(&self, level: Level, fmt: std::fmt::Arguments, source: &str) {
        self.root.lock().unwrap().log(
            level,
            format_args!("{}{}", Indent(self.current_depth.get()), fmt),
            source,
        );
    }

    fn span(&self, level: Level, fmt: std::fmt::Arguments, source: &str) -> EnteredSpan<'_> {
        let start = std::time::Instant::now();
        let mut root = self.root.lock().unwrap();
        let inner = root
            .insert_new_span(start, level, source.into(), self.current_span.get())
            .map(|id| {
                self.current_span.set(Some(id));
                let depth = self.current_depth.get();
                self.current_depth.set(depth + 1);
                root.log(
                    level,
                    format_args!("[ {}]{} {}", id.0, Indent(depth), fmt),
                    source,
                );

                EnteredSpanInner {
                    id,
                    root: self.root.clone(),
                    ctx: self,
                    _unsend_unsync: PhantomData,
                }
            });

        EnteredSpan { inner }
    }
}

impl sealed::Sealed for LogContext {}

struct EnteredSpanInner<'ctx> {
    id: SpanId,
    root: Arc<Mutex<RootLoggerImpl>>,
    ctx: &'ctx LogContext,
    _unsend_unsync: PhantomData<*mut ()>,
}

#[must_use]
pub struct EnteredSpan<'ctx> {
    inner: Option<EnteredSpanInner<'ctx>>,
}

impl EnteredSpan<'_> {
    const fn _inactive() -> Self {
        Self { inner: None }
    }
}

impl Drop for EnteredSpan<'_> {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };

        let mut root = inner.root.lock().unwrap();
        let mut entry = match root.spans.entry(inner.id) {
            std::collections::hash_map::Entry::Occupied(occupied) => occupied,
            std::collections::hash_map::Entry::Vacant(_) => {
                unreachable!("Live span must have a corresponding state in root logger")
            }
        };

        let state = entry.get_mut();
        inner.ctx.current_span.set(state.parent);
        let depth = inner.ctx.current_depth.get() - 1;
        inner.ctx.current_depth.set(depth);
        match NonZero::new(state.refcnt.get() - 1) {
            Some(new_refcnt) => state.refcnt = new_refcnt,
            None => {
                let state = entry.remove();
                let end = std::time::Instant::now();
                root.log(
                    state.level,
                    format_args!(
                        "[/{}]{} Span exited in {:.3}ms",
                        inner.id.0,
                        Indent(depth),
                        (end - state.start).as_secs_f32() * 1000.
                    ),
                    &state.source,
                );
            }
        }
    }
}

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

impl AsLogger for LogContext {
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
macro_rules! span {
    ($logger: expr, $level: expr, $($fmt: tt)*) => {
        $crate::Logger::span(
            $crate::AsLogger::as_logger(&$logger),
            $level, format_args!($($fmt)*), module_path!()
        )
    };
    (@mkmacro $dollar: tt, $name: ident, $level: ident) => {
        #[macro_export]
        #[clippy::format_args]
        macro_rules! $name {
            ($dollar logger: expr, $dollar ($dollar rest: tt)*) => {
                $crate::span!($dollar logger, $crate::Level::$level, $dollar ($dollar rest)*)
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
log!(@mkmacro $, warn, Warn);
log!(@mkmacro $, info, Info);
log!(@mkmacro $, error, Error);
span!(@mkmacro $, trace_span, Trace);
span!(@mkmacro $, debug_span, Debug);
span!(@mkmacro $, warn_span, Warn);
span!(@mkmacro $, info_span, Info);
span!(@mkmacro $, error_span, Error);
