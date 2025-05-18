use std::cell::Cell;

struct TlsState {
    installed: Cell<bool>,
    logger: Cell<*const super::Logger>,
}

thread_local! {
    static LOGGER: TlsState = const {
        TlsState {
            installed: Cell::new(false),
            logger: Cell::new(std::ptr::null())
        }
    };
}

impl From<log::Level> for super::Level {
    fn from(value: log::Level) -> Self {
        match value {
            log::Level::Error => super::Level::Error,
            log::Level::Warn => super::Level::Warn,
            log::Level::Info => super::Level::Info,
            log::Level::Debug => super::Level::Debug,
            log::Level::Trace => super::Level::Trace,
        }
    }
}

struct TlsCompatLogger;

impl log::Log for TlsCompatLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        LOGGER.with(|state| {
            if let Some(logger) = unsafe { state.logger.get().as_ref() } {
                logger.filter().filter(metadata.level().into())
            } else {
                false
            }
        })
    }

    fn log(&self, record: &log::Record) {
        LOGGER.with(|state| {
            if let Some(logger) = unsafe { state.logger.get().as_ref() } {
                logger.log(
                    record.level().into(),
                    *record.args(),
                    record.module_path_static().unwrap_or("unknown"),
                );
            }
        })
    }

    fn flush(&self) {}
}

pub fn with_logger<R>(logger: &super::Logger, fun: impl FnOnce() -> R) -> R {
    LOGGER.with(|state| {
        if !state.installed.get() {
            _ = log::set_logger(&TlsCompatLogger);
            state.installed.set(true);
        }

        state.logger.set(logger);
        let result = fun();
        state.logger.set(std::ptr::null());
        result
    })
}
