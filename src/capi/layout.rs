use std::{ffi::c_char, mem::ManuallyDrop};

use crate::{
    capi::library::CLibrary,
    style::{ComputedStyle, ComputedStyleInner},
};

#[unsafe(no_mangle)]
extern "C" fn sbr_computed_style_default(lctx: *const CLayoutContext) -> *const ComputedStyleInner {
    assert!(!lctx.is_null());

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
unsafe extern "C" fn sbr_computed_style_compute_from_str(
    lctx: *mut CLayoutContext,
    declarations: *const c_char,
    declarations_len: usize,
    parent: *const ComputedStyleInner,
) -> *const ComputedStyleInner {
    let lib = &*(*lctx).library;

    let source = ctry!(std::str::from_utf8(std::slice::from_raw_parts(
        declarations.cast::<u8>(),
        declarations_len,
    )));
    let parent = ManuallyDrop::new(ComputedStyle::from_raw(parent));

    let buffer = ctry!(crate::csssyn::buffer::TokenBuffer::from_source(source));
    let declarations = crate::csssyn::value::parse_declaration_list(buffer.start()).collect();

    ComputedStyle::into_raw(crate::style::from_declarations(
        lib.root_logger.new_ctx(),
        declarations,
        &parent,
    ))
}

struct CLayoutContext {
    library: *const CLibrary,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_context_create(lib: *const CLibrary) -> *mut CLayoutContext {
    Box::into_raw(Box::new(CLayoutContext { library: lib }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_layout_context_destroy(lctx: *mut CLayoutContext) {
    drop(unsafe { Box::from_raw(lctx) });
}
