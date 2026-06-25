use std::{ffi::c_char, mem::ManuallyDrop};

use crate::{
    capi::library::CLibrary,
    style::{ComputedStyle, ComputedStyleInner},
};

#[unsafe(no_mangle)]
extern "C" fn sbr_computed_style_default(_lib: &CLibrary) -> *const ComputedStyleInner {
    ComputedStyle::DEFAULT.into_raw()
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_ref(style: *const ComputedStyleInner) {
    let style = ManuallyDrop::new(unsafe { ComputedStyle::from_raw(style) });
    std::mem::forget(ComputedStyle::clone(&style));
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_unref(style: *const ComputedStyleInner) {
    drop(unsafe { ComputedStyle::from_raw(style) });
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_computed_style_set(
    _lib: &CLibrary,
    style: *mut *const ComputedStyleInner,
    name: *const c_char,
    name_len: usize,
    value: *const c_char,
    value_len: usize,
) {
    let prev = ManuallyDrop::new(unsafe { ComputedStyle::from_raw(style.read()) });
    // do stuff
    todo!();
    style.write(prev.into_raw());
}

struct CLayoutContext {
    // font db and stuff
    pass: Option<CLayoutPass>,
}

struct CLayoutPass {}
