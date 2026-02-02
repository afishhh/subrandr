use std::{cell::Cell, ffi::c_void};

use crate::DebugFlags;

pub struct CLibrary {
    pub(super) root_logger: log::RootLogger,
    pub(super) did_log_version: Cell<bool>,
    pub(super) debug_flags: DebugFlags,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_init() -> *mut CLibrary {
    Box::into_raw(Box::new(CLibrary {
        root_logger: log::RootLogger::new(),
        did_log_version: Cell::new(false),
        debug_flags: DebugFlags::from_env(),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_fini(lib: *mut CLibrary) {
    drop(Box::from_raw(lib));
}

const fn const_parse_u32(value: &str) -> u32 {
    match u32::from_str_radix(value, 10) {
        Ok(result) => result,
        Err(_) => panic!("const value is not an integer"),
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_version(major: *mut u32, minor: *mut u32, patch: *mut u32) {
    major.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_MAJOR")) });
    minor.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_MINOR")) });
    patch.write(const { const_parse_u32(env!("CARGO_PKG_VERSION_PATCH")) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_library_set_log_callback(
    lib: &mut CLibrary,
    callback: log::CLogCallback,
    user_data: *const c_void,
) {
    lib.root_logger
        .set_message_callback(log::MessageCallback::C {
            callback,
            user_data,
        });
}
