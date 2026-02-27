use std::ffi::{c_char, CStr};

use crate::{capi::library::CLibrary, config::Config};

pub(super) struct CConfig {
    cfg: Config,
}

#[unsafe(no_mangle)]
extern "C" fn sbr_config_new(lib: &CLibrary) -> *mut CConfig {
    let cfg = lib.get_or_init_config(&lib.root_logger.new_ctx()).clone();
    Box::into_raw(Box::new(CConfig { cfg }))
}

#[unsafe(no_mangle)]
extern "C" fn sbr_config_set_str(
    cfg: *mut CConfig,
    name: *const c_char,
    value: *const c_char,
    value_len: usize,
) -> u32 {
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        super::fill_last_error(super::CError::from_dyn_error(util::AnyError::from(
            "option does not exist",
        )));
        // non-UTF-8 options can't exist
        return 1;
    };
    let Ok(value) =
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(value as *const u8, value_len) })
    else {
        super::fill_last_error(super::CError::new(
            crate::capi::ErrorKind::Other,
            "value is not valid UTF-8",
        ));
        // non-UTF-8 values can't be valid (currently)
        return 2;
    };

    match unsafe { (*cfg).cfg.set_str(name, value) } {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(err) => {
            super::fill_last_error(super::CError::from_dyn_error(err));
            2
        }
    }
}

#[unsafe(no_mangle)]
extern "C" fn sbr_config_destroy(cfg: *mut CConfig) {
    _ = unsafe { Box::from_raw(cfg) };
}
