use std::ffi::{c_char, c_int, CStr};

use crate::{
    capi::library::CLibrary,
    config::{Config, SetStrError},
};

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
) -> c_int {
    let Ok(name) = unsafe { CStr::from_ptr(name) }.to_str() else {
        cthrow!(OptionNotFound, "option does not exist");
        // non-UTF-8 options can't exist
    };
    let Ok(value) = unsafe { CStr::from_ptr(value) }.to_str() else {
        cthrow!(Other, "value is not valid UTF-8");
        // non-UTF-8 values can't be valid
    };

    match unsafe { (*cfg).cfg.set_str(name, value) } {
        Ok(()) => 0,
        Err(SetStrError::NotFound) => cthrow!(OptionNotFound, "option does not exist"),
        Err(SetStrError::InvalidValue(err)) => cthrow!(super::CError::from_dyn_error(err)),
    }
}

#[unsafe(no_mangle)]
extern "C" fn sbr_config_destroy(cfg: *mut CConfig) {
    _ = unsafe { Box::from_raw(cfg) };
}
