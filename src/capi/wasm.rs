use std::{alloc::Layout, sync::Arc};

use crate::{
    color::BGRA8,
    math::I16Dot16,
    text::{Face, FaceInfo, FontAxisValues, WEIGHT_AXIS},
    Renderer, Subrandr,
};

#[no_mangle]
pub unsafe extern "C" fn sbr_wasm_copy_convert_to_rgba(
    dst: *mut u32,
    src: *mut BGRA8,
    width: usize,
    height: usize,
) {
    let length = width * height;
    for i in 0..length {
        unsafe {
            let value = src.add(i).read();
            dst.add(i).write(value.to_rgba32().to_be());
        }
    }
}

#[no_mangle]
pub extern "C" fn sbr_wasm_alloc(len: usize) -> *mut u8 {
    unsafe { std::alloc::alloc(Layout::array::<u8>(len).unwrap()) }
}

#[no_mangle]
pub unsafe extern "C" fn sbr_wasm_dealloc(ptr: *mut u8, len: usize) {
    unsafe { std::alloc::dealloc(ptr, Layout::array::<u8>(len).unwrap()) }
}

#[no_mangle]
pub extern "C" fn sbr_wasm_create_uninit_arc(data_len: usize) -> *const u8 {
    Arc::into_raw(Arc::<[u8]>::new_uninit_slice(data_len)) as *const u8
}

#[no_mangle]
pub unsafe extern "C" fn sbr_wasm_destroy_arc(ptr: *const u8, len: usize) {
    unsafe {
        drop(Arc::from_raw(std::ptr::slice_from_raw_parts(ptr, len)));
    }
}

#[no_mangle]
pub unsafe extern "C" fn sbr_wasm_library_create_font(
    _sbr: *mut Subrandr,
    data_ptr: *const u8,
    data_len: usize,
) -> *mut Face {
    let data = {
        let data = std::ptr::slice_from_raw_parts(data_ptr, data_len);
        Arc::increment_strong_count(data);
        Arc::from_raw(data)
    };

    Box::into_raw(Box::new(ctry!(Face::load_from_bytes(data, 0))))
}

#[no_mangle]
pub unsafe extern "C" fn sbr_wasm_renderer_add_font(
    renderer: *mut Renderer,
    name_ptr: *const u8,
    name_len: usize,
    weight0: i32,
    weight1: i32,
    italic: bool,
    font: *mut Face,
) {
    let name = std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len)).unwrap();

    let renderer = unsafe { &mut *renderer };
    renderer.fonts.add_extra(FaceInfo {
        family: name.into(),
        width: FontAxisValues::Fixed(I16Dot16::new(100)),
        weight: if weight0 == weight1 {
            if weight0 == -1 {
                (*font).axis(WEIGHT_AXIS).map_or_else(
                    || FontAxisValues::Fixed((*font).weight()),
                    |axis| FontAxisValues::Range(axis.minimum, axis.maximum),
                )
            } else {
                FontAxisValues::Fixed(I16Dot16::new(weight0))
            }
        } else {
            FontAxisValues::Range(I16Dot16::new(weight0), I16Dot16::new(weight1))
        },
        italic,
        source: crate::text::FontSource::Memory((*font).clone()),
    });
}
