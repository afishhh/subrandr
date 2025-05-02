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

#[cfg(feature = "wgpu")]
mod not_public {
    pub struct WebRasterizer {
        instance: wgpu::Instance,
        rasterizer: crate::rasterize::wgpu::Rasterizer,
    }

    use wasm_bindgen::prelude::*;
    use wgpu::web_sys;

    #[wasm_bindgen]
    pub async unsafe fn sbr_wasm_web_rasterizer_create(
    ) -> Result<*mut WebRasterizer, wasm_bindgen::JsError> {
        let instance = wgpu::util::new_instance_with_webgpu_detection(
            &wgpu::InstanceDescriptor::from_env_or_default(),
        )
        .await;

        let Some(adapter) = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::from_env()
                    .unwrap_or(wgpu::PowerPreference::LowPower),
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
        else {
            return Err(wasm_bindgen::JsError::new("No adapter found"));
        };

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default(), None)
            .await?;

        Ok(Box::into_raw(Box::new(WebRasterizer {
            instance,
            rasterizer: {
                let mut r = crate::rasterize::wgpu::Rasterizer::new(device, queue);
                r.set_adapter_info(adapter.get_info());
                r
            },
        })))
    }

    #[wasm_bindgen]
    pub unsafe fn sbr_wasm_renderer_render_with(
        renderer: *mut crate::Renderer,
        ctx: *const crate::SubtitleContext,
        subs: *const crate::Subtitles,
        t: u32,
        rasterizer: *mut WebRasterizer,
        any_canvas: JsValue,
        width: u32,
        height: u32,
    ) -> Result<(), JsError> {
        let WebRasterizer {
            instance,
            rasterizer,
        } = &mut *rasterizer;

        let surface =
            instance.create_surface(match any_canvas.dyn_into::<web_sys::HtmlCanvasElement>() {
                Ok(canvas) => wgpu::SurfaceTarget::Canvas(canvas),
                Err(any_canvas) => match any_canvas.dyn_into::<web_sys::OffscreenCanvas>() {
                    Ok(offscreen_canvas) => wgpu::SurfaceTarget::OffscreenCanvas(offscreen_canvas),
                    Err(value) => {
                        return Err(JsError::new(&format!(
                            "Value of non-canvas type passed as canvas argument: {:?}",
                            value
                        )))
                    }
                },
            })?;

        let device = rasterizer.device();
        surface.configure(
            device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: wgpu::TextureFormat::Bgra8Unorm,
                width,
                height,
                present_mode: wgpu::PresentMode::AutoVsync,
                desired_maximum_frame_latency: 2,
                alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
                view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
            },
        );

        let texture = surface.get_current_texture()?;

        (*renderer).render_to_wgpu(
            rasterizer,
            rasterizer.target_from_texture(texture.texture.clone()),
            &*ctx,
            t,
            unsafe { &*subs },
        )?;

        texture.present();

        Ok(())
    }
}
