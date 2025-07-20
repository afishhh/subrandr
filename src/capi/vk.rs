use std::{
    ffi::{c_char, c_int, CStr},
    mem::MaybeUninit,
};

use crate::{
    rasterize::{wgpu::Rasterizer as WgpuRasterizer, RenderTarget},
    Subrandr,
};

#[derive(Clone)]
struct CEntry {
    raw_entry: ash::Entry,
    version: u32,
    has_nv_optimus: bool,
}

struct CInstance {
    wgpu: wgpu::Instance,
}

impl CInstance {
    fn as_hal(&self) -> &wgpu::hal::vulkan::Instance {
        unsafe { self.wgpu.as_hal::<wgpu::hal::vulkan::Api>().unwrap() }
    }
}

struct CAdapter {
    instance: wgpu::Instance,
    wgpu: wgpu::Adapter,
}

struct CPhysicalDeviceFeatures(
    wgpu::hal::vulkan::PhysicalDeviceFeatures,
    Vec<*const c_char>,
);

struct CDevice {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_entry_create(
    _sbr: *mut Subrandr,
    get_instance_proc_addr: ash::vk::PFN_vkGetInstanceProcAddr,
) -> *mut CEntry {
    let static_fn = ash::StaticFn {
        get_instance_proc_addr,
    };
    let entry = ash::Entry::from_static_fn(static_fn.clone());
    let version = ctry!(entry.try_enumerate_instance_version()).unwrap_or(ash::vk::API_VERSION_1_0);

    let has_nv_optimus = ctry!(entry.enumerate_instance_layer_properties())
        .iter()
        .any(|layer| layer.layer_name_as_c_str() == Ok(c"VK_LAYER_NV_optimus"));

    Box::into_raw(Box::new(CEntry {
        raw_entry: entry,
        version,
        has_nv_optimus,
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_entry_destroy(instance: *mut CEntry) {
    _ = Box::from_raw(instance);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_entry_desired_extensions(
    instance: &CEntry,
    _flags: u64,
    out_extensions: *mut *const *const c_char,
    out_num_extensions: *mut usize,
) -> c_int {
    let extensions = ctry!(wgpu::hal::vulkan::Instance::desired_extensions(
        &instance.raw_entry,
        instance.version,
        wgpu::InstanceFlags::default(),
    ));

    // Maybe someday CStr will be a thin pointer...
    let ptrs: Vec<*const c_char> = extensions.iter().map(|x| x.as_ptr()).collect();

    out_extensions.write(ptrs.as_ptr());
    out_num_extensions.write(ptrs.len());

    std::mem::forget(ptrs);

    0
}

#[repr(C)]
struct InstanceParams {
    flags: u64,
    extensions: *const *const c_char,
    num_extensions: usize,
    android_sdk_version: u32,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_instance_create(
    entry: &CEntry,
    raw_instance: ash::vk::Instance,
    params: &InstanceParams,
) -> *mut CInstance {
    let cstr_extensions = std::slice::from_raw_parts(params.extensions, params.num_extensions)
        .iter()
        .map(|&ptr| CStr::from_ptr(ptr))
        .collect();

    let instance = ash::Instance::load(entry.raw_entry.static_fn(), raw_instance);

    let hal_instance = ctry!(wgpu::hal::vulkan::Instance::from_raw(
        entry.raw_entry.clone(),
        instance.clone(),
        entry.version,
        params.android_sdk_version,
        None,
        cstr_extensions,
        wgpu::InstanceFlags::default(),
        wgpu::MemoryBudgetThresholds::default(),
        entry.has_nv_optimus,
        Some(Box::new(|| {})),
    ));

    Box::into_raw(Box::new(CInstance {
        wgpu: wgpu::Instance::from_hal::<wgpu::hal::vulkan::Api>(hal_instance),
    }))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_instance_destroy(entry: *mut CEntry) {
    _ = Box::from_raw(entry);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_adapter_create(
    instance: &CInstance,
    physical_device: ash::vk::PhysicalDevice,
) -> *mut CAdapter {
    let hal = instance.as_hal();

    match hal.expose_adapter(physical_device) {
        Some(adapter) => Box::into_raw(Box::new(CAdapter {
            instance: instance.wgpu.clone(),
            wgpu: instance.wgpu.create_adapter_from_hal(adapter),
        })),
        None => {
            super::clear_last_error();
            std::ptr::null_mut()
        }
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_adapter_required_physical_device_features(
    adapter: &'static CAdapter,
    _flags: u64,
) -> *mut CPhysicalDeviceFeatures {
    let hal_adapter = adapter.wgpu.as_hal::<wgpu::hal::vulkan::Api>().unwrap();
    let exts = hal_adapter.required_device_extensions(wgpu::Features::default());
    let feat = hal_adapter.physical_device_features(&exts, wgpu::Features::empty());
    Box::into_raw(Box::new(CPhysicalDeviceFeatures(
        feat,
        exts.iter().map(|s| s.as_ptr()).collect(),
    )))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_adapter_destroy(adapter: *mut CAdapter) {
    _ = Box::from_raw(adapter);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_physical_device_features_required_extensions(
    features: &'static mut CPhysicalDeviceFeatures,
    out_extensions: *mut *const *const c_char,
    out_num_extensions: *mut usize,
) {
    out_extensions.write(features.1.as_ptr());
    out_num_extensions.write(features.1.len());
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_physical_device_features_add_to_device_create(
    features: &'static mut CPhysicalDeviceFeatures,
    device_open: &mut ash::vk::DeviceCreateInfo,
) {
    *device_open = features.0.add_to_device_create(*device_open);
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_physical_device_features_destroy(
    adapter: *mut CPhysicalDeviceFeatures,
) {
    _ = Box::from_raw(adapter);
}

#[repr(C)]
struct DeviceParams {
    flags: u64,
    enabled_extensions: *const *const c_char,
    num_enabled_extensions: usize,
    family_index: u32,
    queue_index: u32,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_device_from_raw(
    adapter: &CAdapter,
    raw_device: ash::vk::Device,
    params: &DeviceParams,
) -> *mut CDevice {
    let cstr_extensions: Vec<_> =
        std::slice::from_raw_parts(params.enabled_extensions, params.num_enabled_extensions)
            .iter()
            .map(|&ptr| CStr::from_ptr(ptr))
            .collect();

    let features = wgpu::Features::default();
    let memory_hints = wgpu::MemoryHints::default();

    let hal_device = {
        let hal_adapter = adapter.wgpu.as_hal::<wgpu::hal::vulkan::Api>().unwrap();
        ctry!(hal_adapter.device_from_raw(
            ash::Device::load(
                adapter
                    .instance
                    .as_hal::<wgpu::hal::vulkan::Api>()
                    .unwrap()
                    .shared_instance()
                    .raw_instance()
                    .fp_v1_0(),
                raw_device,
            ),
            Some(Box::new(|| {})),
            &cstr_extensions,
            features,
            &memory_hints,
            params.family_index,
            params.queue_index,
        ))
    };

    let (device, queue) = ctry!(adapter.wgpu.create_device_from_hal(
        hal_device,
        &wgpu::DeviceDescriptor {
            label: None,
            required_features: features,
            required_limits: wgpu::Limits::defaults(),
            memory_hints,
            trace: wgpu::Trace::Off,
        },
    ));

    let device = CDevice {
        device,
        queue,
        adapter_info: adapter.wgpu.get_info(),
    };

    Box::into_raw(Box::new(device))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_device_destroy(device: *mut CDevice) {
    _ = Box::from_raw(device);
}

#[repr(C)]
struct CVkRasterizer {
    erased_ptr: super::ErasedRasterizerPtr,
    rasterizer: WgpuRasterizer,
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_rasterizer_create(cdevice: &CDevice) -> *mut CVkRasterizer {
    let storage =
        Box::into_raw(Box::new(MaybeUninit::<CVkRasterizer>::uninit())).cast::<CVkRasterizer>();

    (&raw mut (*storage).rasterizer).write({
        let mut rasterizer = WgpuRasterizer::new(cdevice.device.clone(), cdevice.queue.clone());
        rasterizer.set_adapter_info(cdevice.adapter_info.clone());
        rasterizer
    });
    (*storage).erased_ptr.0 = &raw mut (*storage).rasterizer;

    storage
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_rasterizer_create_render_target(
    rasterizer: *mut CVkRasterizer,
    image: ash::vk::Image,
    extent: &ash::vk::Extent2D,
) -> *mut RenderTarget<'_> {
    let hal_descriptor = wgpu::hal::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUses::COLOR_TARGET,
        view_formats: vec![wgpu::TextureFormat::Bgra8Unorm],
        // FIXME: What do these mean???
        memory_flags: wgpu::hal::MemoryFlags::empty(),
    };
    let descriptor = wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[wgpu::TextureFormat::Bgra8Unorm],
    };

    let texture =
        wgpu::hal::vulkan::Device::texture_from_raw(image, &hal_descriptor, Some(Box::new(|| {})));
    Box::into_raw(Box::new(
        (*rasterizer).rasterizer.target_from_texture(
            (*rasterizer)
                .rasterizer
                .device()
                .create_texture_from_hal::<wgpu::hal::vulkan::Api>(texture, &descriptor),
        ),
    ))
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_rasterizer_submit(
    rasterizer: *mut CVkRasterizer,
    target: *mut RenderTarget,
) -> c_int {
    (*rasterizer)
        .rasterizer
        .submit_render(*Box::from_raw(target));
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn sbr_vk_rasterizer_destroy(rasterizer: *mut CVkRasterizer) {
    _ = Box::from_raw(rasterizer);
}
