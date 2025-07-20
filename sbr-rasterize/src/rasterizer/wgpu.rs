use std::mem::MaybeUninit;

use util::math::{Point2, Point2f, Rect2f, Vec2, Vec2f};
use wgpu::{include_wgsl, util::DeviceExt, vertex_attr_array};

use crate::color::BGRA8;

use super::{sw::blur::gaussian_sigma_to_box_radius, PixelFormat};

mod packer;
use packer::{PackedTexture, TexturePacker};

pub struct Rasterizer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: Option<wgpu::AdapterInfo>,

    stroke_fill_bind_group_layout: wgpu::BindGroupLayout,
    stroke_pipeline: wgpu::RenderPipeline,
    fill_pipeline: wgpu::RenderPipeline,

    packers: Packers,
    blitter: Blitter,

    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::ComputePipeline,
    blur_state: Option<BlurState>,
}

struct Packers {
    mono: TexturePacker,
    bgra: TexturePacker,
}

impl Packers {
    fn for_format(&self, pixel_format: PixelFormat) -> &TexturePacker {
        match pixel_format {
            PixelFormat::Mono => &self.mono,
            PixelFormat::Bgra => &self.bgra,
        }
    }

    fn for_format_mut(&mut self, pixel_format: PixelFormat) -> &mut TexturePacker {
        match pixel_format {
            PixelFormat::Mono => &mut self.mono,
            PixelFormat::Bgra => &mut self.bgra,
        }
    }
}

struct Blitter {
    nearest_sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_color_to_bgra: wgpu::RenderPipeline,
    pipeline_mono_to_bgra: wgpu::RenderPipeline,
    pipeline_mono_to_mono: wgpu::RenderPipeline,
    pipeline_xxxa_to_mono: wgpu::RenderPipeline,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct BlitInstanceInput {
    src_pos: Point2f,
    src_uv_size: Vec2f,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct BlitInstanceOutput {
    dst_pos: Point2f,
    size: Vec2f,
    color: [f32; 4],
}

#[derive(Debug)]
struct BlitBatch {
    pipeline: wgpu::RenderPipeline,
    texture: wgpu::Texture,
    // These are split in two because of the vertex buffer stride limit which
    // is 32 bytes.
    inputs: Vec<BlitInstanceInput>,
    outputs: Vec<BlitInstanceOutput>,
}

impl Blitter {
    fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(include_wgsl!("./wgpu/blit.wgsl"));
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    // TODO: Offload text rendering billinear interpolation to the GPU
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_for_fragment_with_name = |label: &'static str,
                                               fragment_entry_point: &'static str,
                                               target_format: wgpu::TextureFormat,
                                               blend: bool|
         -> wgpu::RenderPipeline {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(
                    &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[&bind_group_layout],
                        push_constant_ranges: &[],
                    }),
                ),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[
                        wgpu::VertexBufferLayout {
                            array_stride: size_of::<BlitInstanceInput>() as wgpu::BufferAddress,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &vertex_attr_array![
                                0 => Float32x2,
                                1 => Float32x2,
                            ],
                        },
                        wgpu::VertexBufferLayout {
                            array_stride: size_of::<BlitInstanceOutput>() as wgpu::BufferAddress,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &vertex_attr_array![
                                2 => Float32x2,
                                3 => Float32x2,
                                4 => Float32x4
                            ],
                        },
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    strip_index_format: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment_entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: blend.then_some(wgpu::BlendState {
                            color: wgpu::BlendComponent::OVER,
                            alpha: wgpu::BlendComponent::OVER,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview: None,
                cache: None,
            })
        };

        Self {
            nearest_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),

            pipeline_mono_to_bgra: pipeline_for_fragment_with_name(
                "mono to mono blit pipeline",
                "fs_main_mono_to_bgra",
                wgpu::TextureFormat::Bgra8Unorm,
                true,
            ),
            pipeline_color_to_bgra: pipeline_for_fragment_with_name(
                "bgra to bgra blit pipeline",
                "fs_main_bgra_to_bgra",
                wgpu::TextureFormat::Bgra8Unorm,
                true,
            ),
            pipeline_mono_to_mono: pipeline_for_fragment_with_name(
                "mono to mono blit pipeline",
                "fs_main_mono_to_mono",
                wgpu::TextureFormat::R32Float,
                false,
            ),
            pipeline_xxxa_to_mono: pipeline_for_fragment_with_name(
                "xxxa to mono blit pipeline",
                "fs_main_xxxa_to_mono",
                wgpu::TextureFormat::R32Float,
                false,
            ),
            bind_group_layout,
        }
    }
}

struct BlurState {
    encoder: wgpu::CommandEncoder,
    blit_pass: wgpu::RenderPass<'static>,
    blit_batch: Option<BlitBatch>,
    front_texture: wgpu::Texture,
    radius: u32,
}

#[derive(Debug)]
pub(super) struct RenderTargetImpl {
    pub tex: wgpu::Texture,
    pass: wgpu::RenderPass<'static>,
    encoder: wgpu::CommandEncoder,
    blit_batch: Option<BlitBatch>,
}

/// Describes what an unwrapped render target will be used for and is used to
/// determine what kinds of buffered should be flushed before releasing the
/// underlying render target to subsequent drawing code.
///
/// For example a [`RenderTargetUse::Blit`] will not flush the blit batch because
/// the render target will be used for blitting and the usage site must be aware of
/// the batching and integrate with it. But a [`RenderTargetUse::Other`] will flush
/// any pending blits so that drawing a rectangle on top of a blitted batch will have
/// the expected draw order.
#[derive(Debug, Clone, Copy)]
enum RenderTargetUse {
    /// Will not flush the current blit batch
    Blit,
    /// Will flush all pending buffered operations
    Other,
}

impl Rasterizer {
    fn flush_render_target_for_use(&self, target: &mut RenderTargetImpl, use_: RenderTargetUse) {
        if !matches!(use_, RenderTargetUse::Blit) {
            if let Some(batch) = target.blit_batch.take() {
                batch.execute(&self.device, &mut target.pass, &self.blitter);
            }
        }
    }

    fn unwrap_render_target<'a>(
        &self,
        target: &'a mut super::RenderTarget<'_>,
        use_: RenderTargetUse,
    ) -> Option<&'a mut RenderTargetImpl> {
        match &mut target.0 {
            super::RenderTargetInner::Wgpu(target) => {
                self.flush_render_target_for_use(target, use_);
                Some(target)
            }
            super::RenderTargetInner::WgpuEmpty => None,
            target => panic!(
                "Incompatible render target {:?} passed to wgpu rasterizer (expected: wgpu)",
                target.variant_name()
            ),
        }
    }

    fn unwrap_render_target_owned(
        &self,
        target: super::RenderTarget<'_>,
        use_: RenderTargetUse,
    ) -> Option<Box<RenderTargetImpl>> {
        match target.0 {
            super::RenderTargetInner::Wgpu(mut target) => {
                self.flush_render_target_for_use(&mut target, use_);
                Some(target)
            }
            super::RenderTargetInner::WgpuEmpty => None,
            target => panic!(
                "Incompatible render target {:?} passed to wgpu rasterizer (expected: wgpu)",
                target.variant_name()
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum TextureImpl {
    // TODO: Get rid of the need for this
    //       i.e. per-glyph blur
    Full(wgpu::Texture),
    Packed(PackedTexture, PixelFormat),
    Empty,
}
impl TextureImpl {
    pub fn width(&self) -> u32 {
        match self {
            TextureImpl::Full(texture) => texture.width(),
            TextureImpl::Packed(packed, _) => packed.size().x,
            TextureImpl::Empty => 0,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            TextureImpl::Full(texture) => texture.height(),
            TextureImpl::Packed(packed, _) => packed.size().y,
            TextureImpl::Empty => 0,
        }
    }
}

struct UnwrappedTexture<'a> {
    texture: &'a wgpu::Texture,
    position: Point2<u32>,
    size: Vec2<u32>,
    format: PixelFormat,
}

impl<'a> UnwrappedTexture<'a> {
    fn from_texture_region(
        texture: &'a wgpu::Texture,
        position: Point2<u32>,
        size: Vec2<u32>,
    ) -> Self {
        Self {
            texture,
            position,
            size,
            format: match texture.format() {
                wgpu::TextureFormat::Bgra8Unorm => PixelFormat::Bgra,
                wgpu::TextureFormat::R8Unorm => PixelFormat::Mono,
                wgpu::TextureFormat::R32Float => PixelFormat::Mono,
                format => {
                    panic!("Texture with unexpected format {format:?} passed to wgpu rasterizer")
                }
            },
        }
    }
}

fn unwrap_wgpu_texture<'a>(
    texture: &'a super::Texture,
    packers: &'a Packers,
) -> Option<UnwrappedTexture<'a>> {
    match &texture.0 {
        super::TextureInner::Wgpu(TextureImpl::Empty) => None,
        super::TextureInner::Wgpu(TextureImpl::Full(texture)) => {
            Some(UnwrappedTexture::from_texture_region(
                texture,
                Point2::ZERO,
                Vec2::new(texture.width(), texture.height()),
            ))
        }
        &super::TextureInner::Wgpu(TextureImpl::Packed(ref packed, format)) => {
            let (texture, position, size) = packed.get_texture_region(packers.for_format(format));
            Some(UnwrappedTexture::from_texture_region(
                texture, position, size,
            ))
        }
        target => panic!(
            "Incompatible texture {:?} passed to software rasterizer",
            target.variant_name()
        ),
    }
}

impl Rasterizer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let fill_module = device.create_shader_module(include_wgsl!("./wgpu/fill.wgsl"));
        let fill_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    // color
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });

        let blur_module = unsafe {
            device.create_shader_module_trusted(
                include_wgsl!("./wgpu/blur.wgsl"),
                wgpu::ShaderRuntimeChecks::unchecked(),
            )
        };
        let blur_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format: wgpu::TextureFormat::R32Float,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                ],
            });

        let make_stroke_or_fill_pipeline =
            |topology: wgpu::PrimitiveTopology, name: &'static str| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(name),
                    layout: Some(
                        &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[&fill_bind_group_layout],
                            push_constant_ranges: &[],
                        }),
                    ),
                    vertex: wgpu::VertexState {
                        module: &fill_module,
                        entry_point: Some("vs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        buffers: &[
                            // positions
                            wgpu::VertexBufferLayout {
                                array_stride: 8,
                                step_mode: wgpu::VertexStepMode::Vertex,
                                attributes: &vertex_attr_array![
                                    0 => Float32x2,
                                ],
                            },
                        ],
                    },
                    primitive: wgpu::PrimitiveState {
                        topology,
                        strip_index_format: None,
                        polygon_mode: wgpu::PolygonMode::Fill,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &fill_module,
                        entry_point: Some("fs_main"),
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Bgra8Unorm,
                            blend: Some(wgpu::BlendState {
                                color: wgpu::BlendComponent::OVER,
                                alpha: wgpu::BlendComponent::OVER,
                            }),
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview: None,
                    cache: None,
                })
            };

        Self {
            stroke_pipeline: make_stroke_or_fill_pipeline(
                wgpu::PrimitiveTopology::LineStrip,
                "shape stroke pipeline",
            ),
            fill_pipeline: make_stroke_or_fill_pipeline(
                wgpu::PrimitiveTopology::TriangleStrip,
                "triangle fill pipeline",
            ),
            stroke_fill_bind_group_layout: fill_bind_group_layout,

            packers: Packers {
                mono: TexturePacker::new(
                    device.clone(),
                    queue.clone(),
                    wgpu::TextureFormat::R8Unorm,
                ),
                bgra: TexturePacker::new(
                    device.clone(),
                    queue.clone(),
                    wgpu::TextureFormat::Bgra8Unorm,
                ),
            },
            blitter: Blitter::new(&device),

            blur_pipeline: device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("gaussian blur pipeline"),
                layout: Some(
                    &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[&blur_bind_group_layout],
                        push_constant_ranges: &[],
                    }),
                ),
                module: &blur_module,
                entry_point: Some("cs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            }),
            blur_bind_group_layout,

            blur_state: None,

            device,
            queue,
            adapter_info: None,
        }
    }

    pub fn set_adapter_info(&mut self, info: wgpu::AdapterInfo) {
        self.adapter_info = Some(info);
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn target_from_texture(&self, texture: wgpu::Texture) -> super::RenderTarget<'static> {
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        let pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture.create_view(&wgpu::TextureViewDescriptor {
                        label: None,
                        format: Some(texture.format()),
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
                        aspect: wgpu::TextureAspect::All,
                        ..Default::default()
                    }),
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();

        super::RenderTarget(super::RenderTargetInner::Wgpu(Box::new(RenderTargetImpl {
            tex: texture,
            pass,
            encoder,
            blit_batch: None,
        })))
    }
}

impl Rasterizer {
    fn stroke_polyline_or_polygon(
        &mut self,
        target: &mut super::RenderTarget<'_>,
        offset: Vec2f,
        vertices: &[Point2f],
        closed: bool,
        color: BGRA8,
    ) {
        let Some(target) = self.unwrap_render_target(target, RenderTargetUse::Other) else {
            return;
        };

        let data = {
            let mut result = Vec::with_capacity(vertices.len() + closed as usize);
            result.extend(
                vertices
                    .iter()
                    .map(|&point| target_transform_point_for(target, point + offset)),
            );
            if closed {
                result.push(result[0]);
            }
            result
        };

        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("line draw buffer"),
                contents: unsafe {
                    std::slice::from_raw_parts(
                        data.as_ptr() as *const u8,
                        size_of_val(data.as_slice()),
                    )
                },
                usage: wgpu::BufferUsages::VERTEX,
            });

        let uniform0_data = [
            // color
            color.r as f32 / 256.0,
            color.g as f32 / 256.0,
            color.b as f32 / 256.0,
            color.a as f32 / 256.0,
        ];

        target.pass.set_pipeline(&self.stroke_pipeline);
        target.pass.set_vertex_buffer(0, buffer.slice(..));
        target.pass.set_bind_group(
            0,
            Some(&self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("line stroke bind group"),
                layout: &self.stroke_fill_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.device.create_buffer_init(
                            &wgpu::util::BufferInitDescriptor {
                                label: None,
                                contents: unsafe {
                                    std::slice::from_raw_parts(
                                        uniform0_data.as_ptr() as *const u8,
                                        16,
                                    )
                                },
                                usage: wgpu::BufferUsages::UNIFORM,
                            },
                        ),
                        offset: 0,
                        size: None,
                    }),
                }],
            })),
            &[],
        );
        target.pass.draw(0..data.len() as u32, 0..1);
    }

    fn fill_triangles<const N: usize>(
        &mut self,
        target: &mut super::RenderTarget<'_>,
        vertices: &[Point2f; N],
        color: BGRA8,
    ) {
        let Some(target) = self.unwrap_render_target(target, RenderTargetUse::Other) else {
            return;
        };

        let data = vertices.map(|point| target_transform_point_for(target, point));

        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("triangle fill buffer"),
                contents: unsafe {
                    std::slice::from_raw_parts(
                        data.as_ptr().cast::<u8>(),
                        std::mem::size_of_val(&data),
                    )
                },
                usage: wgpu::BufferUsages::VERTEX,
            });

        let uniform0_data = [
            // color
            color.r as f32 / 256.0,
            color.g as f32 / 256.0,
            color.b as f32 / 256.0,
            color.a as f32 / 256.0,
        ];

        target.pass.set_pipeline(&self.fill_pipeline);
        target.pass.set_vertex_buffer(0, buffer.slice(..));
        target.pass.set_bind_group(
            0,
            Some(&self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("triangle fill bind group"),
                layout: &self.stroke_fill_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.device.create_buffer_init(
                            &wgpu::util::BufferInitDescriptor {
                                label: None,
                                contents: unsafe {
                                    std::slice::from_raw_parts(
                                        uniform0_data.as_ptr() as *const u8,
                                        16,
                                    )
                                },
                                usage: wgpu::BufferUsages::UNIFORM,
                            },
                        ),
                        offset: 0,
                        size: None,
                    }),
                }],
            })),
            &[],
        );
        target.pass.draw(0..vertices.len() as u32, 0..1);
    }
}

#[inline]
fn target_transform_point_for(target: &RenderTargetImpl, p: Point2f) -> Point2f {
    target_transform_point(target.tex.width(), target.tex.height(), p)
}

#[inline]
fn target_transform_point(width: u32, height: u32, p: Point2f) -> Point2f {
    Point2::new(
        (p.x / width as f32) * 2.0 - 1.0,
        -(p.y / height as f32) * 2.0 + 1.0,
    )
}

struct StructBuilder<const SIZE: usize> {
    data: [MaybeUninit<u8>; SIZE],
    offset: usize,
}

impl<const SIZE: usize> StructBuilder<SIZE> {
    pub fn new() -> Self {
        Self {
            data: [MaybeUninit::uninit(); SIZE],
            offset: 0,
        }
    }

    // TODO: Either add bytemuck or wait for safe transmutes in std
    pub fn write_u32s(&mut self, data: &[u32]) {
        let bytes =
            unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 4) };
        unsafe {
            self.data[self.offset..self.offset + size_of_val(data)]
                .copy_from_slice(std::mem::transmute::<&[u8], &[MaybeUninit<u8>]>(bytes));
        }
        self.offset += size_of_val(data);
    }

    fn finish(self) -> [u8; SIZE] {
        assert_eq!(self.offset, SIZE);
        *unsafe { std::mem::transmute::<&[MaybeUninit<u8>; SIZE], &[u8; SIZE]>(&self.data) }
    }
}

impl Rasterizer {
    fn submit_render_impl(&mut self, target: RenderTargetImpl) -> wgpu::Texture {
        assert!(target.blit_batch.is_none());
        drop(target.pass);
        self.queue.submit([target.encoder.finish()]);
        target.tex
    }

    pub fn submit_render(&mut self, target: super::RenderTarget<'_>) {
        if let Some(target) = self.unwrap_render_target_owned(target, RenderTargetUse::Other) {
            self.submit_render_impl(*target);
        }
    }

    fn create_texture_mapped_impl(
        &mut self,
        width: u32,
        height: u32,
        format: super::PixelFormat,
        // FIXME: ugly box...
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
        pack: bool,
    ) -> super::Texture {
        if width == 0 || height == 0 {
            callback(&mut [], 0);
            return super::Texture(super::TextureInner::Wgpu(TextureImpl::Empty));
        }

        let byte_stride = (width * u32::from(format.width()))
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("memory mapped texture write buffer"),
            size: (u64::from(byte_stride) * u64::from(height))
                .next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT),
            usage: wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: true,
        });

        {
            let mut mapped = buffer.slice(..).get_mapped_range_mut();
            let slice = &mut *mapped;

            callback(unsafe { std::mem::transmute(slice) }, byte_stride as usize);
        }

        buffer.unmap();

        let wgpu_format = match format {
            PixelFormat::Bgra => wgpu::TextureFormat::Bgra8Unorm,
            PixelFormat::Mono => wgpu::TextureFormat::R8Unorm,
        };

        let inner = if pack {
            let packer = self.packers.for_format_mut(format);

            TextureImpl::Packed(
                packer.add_from_buffer(&buffer, byte_stride, width, height),
                format,
            )
        } else {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu_format,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[wgpu_format],
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("mapped buffer -> texture move encoder"),
                });
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(byte_stride),
                        rows_per_image: None,
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.queue.submit([encoder.finish()]);

            TextureImpl::Full(texture)
        };

        super::Texture(super::TextureInner::Wgpu(inner))
    }
}

impl super::Rasterizer for Rasterizer {
    fn name(&self) -> &'static str {
        "wgpu"
    }

    fn write_debug_info(&self, writer: &mut dyn std::fmt::Write) -> std::fmt::Result {
        if let Some(info) = self.adapter_info.as_ref() {
            writeln!(writer, "adapter: {} ({})", info.name, info.driver)?;
        }

        for (name, packer) in [("mono", &self.packers.mono), ("bgra", &self.packers.bgra)] {
            writeln!(writer, "{name} texture atlas stats:")?;
            packer.write_atlas_stats(writer)?;
        }

        Ok(())
    }

    unsafe fn create_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: super::PixelFormat,
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> super::Texture {
        self.create_texture_mapped_impl(width, height, format, callback, false)
    }

    unsafe fn create_packed_texture_mapped(
        &mut self,
        width: u32,
        height: u32,
        format: PixelFormat,
        callback: Box<dyn FnOnce(&mut [MaybeUninit<u8>], usize) + '_>,
    ) -> super::Texture {
        self.create_texture_mapped_impl(width, height, format, callback, true)
    }

    fn create_mono_texture_rendered(
        &mut self,
        width: u32,
        height: u32,
    ) -> super::RenderTarget<'static> {
        if width == 0 || height == 0 {
            return super::RenderTarget(super::RenderTargetInner::WgpuEmpty);
        }

        self.target_from_texture(self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("render texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::R32Float],
        }))
    }

    fn finalize_texture_render(&mut self, target: super::RenderTarget<'static>) -> super::Texture {
        let Some(target) = self.unwrap_render_target_owned(target, RenderTargetUse::Other) else {
            return super::Texture(super::TextureInner::Wgpu(TextureImpl::Empty));
        };

        super::Texture(super::TextureInner::Wgpu(TextureImpl::Full(
            self.submit_render_impl(*target),
        )))
    }

    fn blit(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
        color: BGRA8,
    ) {
        let Some(target) = self.unwrap_render_target(target, RenderTargetUse::Blit) else {
            return;
        };

        if let Some(unwrapped) = unwrap_wgpu_texture(texture, &self.packers) {
            self.blitter.do_blit(
                &self.device,
                &mut target.pass,
                &mut target.blit_batch,
                super::PixelFormat::Bgra,
                target.tex.width(),
                target.tex.height(),
                dx,
                dy,
                &unwrapped,
                color,
            );
        }
    }

    unsafe fn blit_to_mono_texture_unchecked(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
    ) {
        let Some(target) = self.unwrap_render_target(target, RenderTargetUse::Blit) else {
            return;
        };

        if let Some(unwrapped) = unwrap_wgpu_texture(texture, &self.packers) {
            self.blitter.do_blit(
                &self.device,
                &mut target.pass,
                &mut target.blit_batch,
                super::PixelFormat::Mono,
                target.tex.width(),
                target.tex.height(),
                dx,
                dy,
                &unwrapped,
                BGRA8::WHITE,
            );
        }
    }

    fn flush(&mut self, target: &mut super::RenderTarget) {
        _ = self.unwrap_render_target(target, RenderTargetUse::Other);

        self.packers.mono.defragment();
        self.packers.bgra.defragment();
    }

    fn line(
        &mut self,
        target: &mut super::RenderTarget,
        p0: Point2f,
        p1: Point2f,
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(target, Vec2::ZERO, &[p0, p1], false, color);
    }

    fn stroke_polyline(
        &mut self,
        target: &mut super::RenderTarget,
        offset: Vec2f,
        vertices: &[Point2f],
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(target, offset, vertices, false, color);
    }

    fn stroke_polygon(
        &mut self,
        target: &mut super::RenderTarget,
        offset: Vec2f,
        vertices: &[Point2f],
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(target, offset, vertices, true, color);
    }

    fn fill_triangle(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[Point2f; 3],
        color: crate::color::BGRA8,
    ) {
        self.fill_triangles(target, vertices, color);
    }

    fn fill_axis_aligned_rect(
        &mut self,
        target: &mut super::RenderTarget,
        rect: Rect2f,
        color: BGRA8,
    ) {
        // TODO: Anti aliased rectangle drawing
        self.fill_triangles(
            target,
            &[
                rect.min,
                Point2f::new(rect.max.x, rect.min.y),
                Point2f::new(rect.min.x, rect.max.y),
                rect.max,
            ],
            color,
        );
    }

    fn blur_prepare(&mut self, width: u32, height: u32, sigma: f32) {
        if self.blur_state.is_some() {
            panic!("GpuRasterizer::blur_prepare called while a blur is still in-progress")
        }

        let radius = gaussian_sigma_to_box_radius(sigma) as u32;
        let twidth = width + 2 * 2 * radius;
        let theight = height + 2 * 2 * radius;
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("blur command encoder"),
            });
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur initial front buffer"),
            size: wgpu::Extent3d {
                width: twidth,
                height: theight,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::R32Float],
        });

        self.blur_state = Some(BlurState {
            blit_pass: encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("blur front buffer render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &texture.create_view(&wgpu::TextureViewDescriptor {
                            label: Some("blur initial front buffer view"),
                            ..Default::default()
                        }),
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime(),
            blit_batch: None,
            encoder,
            front_texture: texture,
            radius,
        })
    }

    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &super::Texture) {
        let state = self
            .blur_state
            .as_mut()
            .expect("Rasterizer::blur_buffer_blit called without an active blur pass");

        if let Some(unwrapped) = unwrap_wgpu_texture(texture, &self.packers) {
            self.blitter.do_blit(
                &self.device,
                &mut state.blit_pass,
                &mut state.blit_batch,
                PixelFormat::Mono,
                state.front_texture.width(),
                state.front_texture.height(),
                dx + (2 * state.radius) as i32,
                dy + (2 * state.radius) as i32,
                &unwrapped,
                BGRA8::WHITE,
            );
        }
    }

    fn blur_padding(&mut self) -> Vec2f {
        let state = self
            .blur_state
            .as_ref()
            .expect("Rasterizer::blur_padding called without an active blur pass");
        let pad = (2 * state.radius) as f32;
        Vec2::new(pad, pad)
    }

    fn blur_to_mono_texture(&mut self) -> super::Texture {
        let mut state = self
            .blur_state
            .take()
            .expect("Rasterizer::blur_to_mono_texture called without an active blur pass");

        if let Some(batch) = state.blit_batch.take() {
            batch.execute(&self.device, &mut state.blit_pass, &self.blitter);
        }

        drop(state.blit_pass);

        self.do_blur(&mut state.encoder, &state.front_texture, state.radius);

        self.queue.submit([state.encoder.finish()]);

        super::Texture(super::TextureInner::Wgpu(TextureImpl::Full(
            state.front_texture,
        )))
    }
}

impl Rasterizer {
    fn do_blur(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        front_texture: &wgpu::Texture,
        radius: u32,
    ) {
        let horizontal_params = {
            let data = {
                let mut builder = StructBuilder::<16>::new();

                // cross_axis: vec2<u32>
                builder.write_u32s(&[0u32, 1]);

                // radius: u32
                builder.write_u32s(&[radius]);

                // padding
                builder.write_u32s(&[0u32]);

                builder.finish()
            };

            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: &data,
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        };

        let vertical_params = {
            let data = {
                let mut builder = StructBuilder::<16>::new();

                // main_axis: vec2<u32>
                builder.write_u32s(&[1u32, 0]);

                // radius: u32
                builder.write_u32s(&[radius]);

                // padding
                builder.write_u32s(&[0u32]);

                builder.finish()
            };

            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: &data,
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        };

        let back_texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("blur back buffer"),
            size: front_texture.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[wgpu::TextureFormat::R32Float],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("blur compute pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.blur_pipeline);

        let set_pass_params = |pass: &mut wgpu::ComputePass,
                               params: &wgpu::Buffer,
                               front: &wgpu::Texture,
                               back: &wgpu::Texture| {
            pass.set_bind_group(
                0,
                Some(&self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.blur_bind_group_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&front.create_view(
                                &wgpu::TextureViewDescriptor {
                                    usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                                    ..Default::default()
                                },
                            )),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: params,
                                offset: 0,
                                size: None,
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&back.create_view(
                                &wgpu::TextureViewDescriptor {
                                    usage: Some(wgpu::TextureUsages::STORAGE_BINDING),
                                    ..Default::default()
                                },
                            )),
                        },
                    ],
                })),
                &[],
            );
        };

        set_pass_params(&mut pass, &horizontal_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);

        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
    }
}

impl BlitBatch {
    fn execute(self, device: &wgpu::Device, pass: &mut wgpu::RenderPass, blitter: &Blitter) {
        let inputs_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: unsafe {
                std::slice::from_raw_parts(
                    self.inputs.as_ptr().cast::<u8>(),
                    size_of_val(self.inputs.as_slice()),
                )
            },
            usage: wgpu::BufferUsages::VERTEX,
        });

        let outputs_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: unsafe {
                std::slice::from_raw_parts(
                    self.outputs.as_ptr().cast::<u8>(),
                    size_of_val(self.outputs.as_slice()),
                )
            },
            usage: wgpu::BufferUsages::VERTEX,
        });

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(
            0,
            Some(&device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &blitter.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&blitter.nearest_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&self.texture.create_view(
                            &wgpu::TextureViewDescriptor {
                                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                                ..Default::default()
                            },
                        )),
                    },
                ],
            })),
            &[],
        );
        pass.set_vertex_buffer(0, inputs_buffer.slice(..));
        pass.set_vertex_buffer(1, outputs_buffer.slice(..));
        pass.draw(0..4, 0..self.outputs.len() as u32);
    }
}

impl Blitter {
    fn do_blit(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass,
        blit_batch: &mut Option<BlitBatch>,
        pass_format: PixelFormat,
        twidth: u32,
        theight: u32,
        dx: i32,
        dy: i32,
        unwrapped: &UnwrappedTexture,
        color: BGRA8,
    ) {
        let pipeline = match (pass_format, unwrapped.format) {
            (PixelFormat::Bgra, PixelFormat::Bgra) => &self.pipeline_color_to_bgra,
            (PixelFormat::Bgra, PixelFormat::Mono) => &self.pipeline_mono_to_bgra,
            (PixelFormat::Mono, PixelFormat::Bgra) => &self.pipeline_xxxa_to_mono,
            (PixelFormat::Mono, PixelFormat::Mono) => &self.pipeline_mono_to_mono,
        };

        if let Some(batch) = blit_batch
            .take_if(|batch| &batch.texture != unwrapped.texture || &batch.pipeline != pipeline)
        {
            batch.execute(device, pass, self);
        }

        let batch = blit_batch.get_or_insert_with(|| BlitBatch {
            pipeline: pipeline.clone(),
            texture: unwrapped.texture.clone(),
            inputs: Vec::new(),
            outputs: Vec::new(),
        });

        batch.outputs.push(BlitInstanceOutput {
            dst_pos: target_transform_point(twidth, theight, Point2::new(dx as f32, dy as f32)),
            size: Vec2::new(
                unwrapped.size.x as f32 / twidth as f32,
                -(unwrapped.size.y as f32 / theight as f32),
            ),
            color: [
                color.r as f32 / 255.0,
                color.g as f32 / 255.0,
                color.b as f32 / 255.0,
                color.a as f32 / 255.0,
            ],
        });

        let in_fsz = Vec2::new(
            unwrapped.texture.width() as f32,
            unwrapped.texture.height() as f32,
        );

        let uv_pos = Point2::new(
            unwrapped.position.x as f32 / in_fsz.x,
            unwrapped.position.y as f32 / in_fsz.y,
        );

        let uv_size = Vec2::new(
            unwrapped.size.x as f32 / in_fsz.x,
            unwrapped.size.y as f32 / in_fsz.y,
        );

        batch.inputs.push(BlitInstanceInput {
            src_pos: uv_pos,
            src_uv_size: uv_size,
        });
    }
}
