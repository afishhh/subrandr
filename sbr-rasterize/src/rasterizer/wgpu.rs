use std::{collections::HashMap, mem::MaybeUninit};

use util::{
    cast_bytes,
    math::{Point2, Point2f, Rect2, Rect2f, Vec2},
};
use wgpu::{include_wgsl, vertex_attr_array};

use crate::{
    color::BGRA8,
    scene::{self, FilledRect, SceneNode, Vec2S},
    sw::blur::gaussian_sigma_to_box_radius,
};

use super::PixelFormat;

mod packer;
use packer::{PackedTexture, TexturePacker};

const INITIAL_UNIFORM_CHUNK_CAPACITY: u32 = 8192;

pub struct Rasterizer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: Option<wgpu::AdapterInfo>,

    fill_bind_group_layout: wgpu::BindGroupLayout,
    fill_pipeline: wgpu::RenderPipeline,

    packers: Packers,
    blitter: Blitter,

    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::ComputePipeline,
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
    blit_bind_group_cache: HashMap<wgpu::Texture, wgpu::BindGroup>,
    pipeline_bgra_to_bgra: wgpu::RenderPipeline,
    pipeline_mono_to_bgra: wgpu::RenderPipeline,
    pipeline_xxxa_to_bgra: wgpu::RenderPipeline,
    pipeline_mono_to_mono: wgpu::RenderPipeline,
    pipeline_xxxa_to_mono: wgpu::RenderPipeline,
}

#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct BlitVertex {
    src_vtx: Point2f,
    dst_vtx: Point2f,
    color: [f32; 4],
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
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: size_of::<BlitVertex>() as wgpu::BufferAddress,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &vertex_attr_array![
                            0 => Float32x2,
                            1 => Float32x2,
                            2 => Float32x4,
                        ],
                    }],
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
            pipeline_bgra_to_bgra: pipeline_for_fragment_with_name(
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
            pipeline_xxxa_to_bgra: pipeline_for_fragment_with_name(
                "xxxa to bgra blit pipeline",
                "fs_main_xxxa_to_bgra",
                wgpu::TextureFormat::Bgra8Unorm,
                true,
            ),
            bind_group_layout,
            blit_bind_group_cache: HashMap::new(),
        }
    }

    fn clear_bind_group_cache(&mut self) {
        self.blit_bind_group_cache.clear();
    }

    fn blit(
        &mut self,
        device: &wgpu::Device,
        uniform_belt: &mut UniformBelt,
        render_pass: &mut wgpu::RenderPass,
        pass_format: PixelFormat,
        tsize: Vec2<u32>,
        dx: i32,
        dy: i32,
        unwrapped: &UnwrappedTexture,
        color: BGRA8,
        extract_alpha: bool,
    ) {
        let pipeline = match (pass_format, unwrapped.format) {
            (PixelFormat::Bgra, PixelFormat::Bgra) if extract_alpha => &self.pipeline_xxxa_to_bgra,
            (PixelFormat::Bgra, PixelFormat::Bgra) => &self.pipeline_bgra_to_bgra,
            (PixelFormat::Bgra, PixelFormat::Mono) => &self.pipeline_mono_to_bgra,
            (PixelFormat::Mono, PixelFormat::Bgra) => &self.pipeline_xxxa_to_mono,
            (PixelFormat::Mono, PixelFormat::Mono) => &self.pipeline_mono_to_mono,
        };

        let texture_size_f32 = Vec2::new(
            unwrapped.texture.width() as f32,
            unwrapped.texture.height() as f32,
        );

        let src_rect = Rect2f::from_min_size(
            Point2::new(
                unwrapped.position.x as f32 / texture_size_f32.x,
                unwrapped.position.y as f32 / texture_size_f32.y,
            ),
            Vec2::new(
                unwrapped.size.x as f32 / texture_size_f32.x,
                unwrapped.size.y as f32 / texture_size_f32.y,
            ),
        );
        let dst_rect = target_transform_rect(
            tsize,
            Rect2::from_min_size(
                Point2::new(dx as f32, dy as f32),
                Vec2::new(unwrapped.size.x as f32, unwrapped.size.y as f32),
            ),
        );

        let src_strip = rect_to_triangle_strip(&src_rect);
        let dst_strip = rect_to_triangle_strip(&dst_rect);
        let color_f32 = [
            color.r as f32 / 255.0,
            color.g as f32 / 255.0,
            color.b as f32 / 255.0,
            color.a as f32 / 255.0,
        ];
        let vertices: [BlitVertex; 4] = std::array::from_fn(|i| BlitVertex {
            src_vtx: src_strip[i],
            dst_vtx: dst_strip[i],
            color: color_f32,
        });

        render_pass.set_pipeline(pipeline);

        render_pass.set_bind_group(
            0,
            Some(
                &*self
                    .blit_bind_group_cache
                    .entry(unwrapped.texture.clone())
                    .or_insert_with(|| {
                        device.create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &self.bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::TextureView(
                                        &unwrapped.texture.create_view(
                                            &wgpu::TextureViewDescriptor {
                                                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                                                ..Default::default()
                                            },
                                        ),
                                    ),
                                },
                            ],
                        })
                    }),
            ),
            &[],
        );

        let vertex_buffer = uniform_belt.write_slice(
            unsafe { cast_bytes(std::slice::from_ref(&vertices)) },
            false,
        );
        render_pass.set_vertex_buffer(0, vertex_buffer);

        render_pass.draw(0..4, 0..1);
    }
}

#[derive(Debug)]
pub(super) struct RenderTarget(wgpu::Texture);

impl RenderTarget {
    pub fn size(&self) -> Vec2<u32> {
        Vec2::new(self.0.width(), self.0.height())
    }

    pub fn width(&self) -> u32 {
        self.0.width()
    }

    pub fn height(&self) -> u32 {
        self.0.height()
    }
}

#[derive(Debug, Clone)]
pub(super) enum Texture {
    // TODO: Get rid of the need for this
    //       i.e. per-glyph blur
    Full(wgpu::Texture),
    Packed(PackedTexture, PixelFormat),
    Empty(PixelFormat),
}

impl Texture {
    pub fn memory_footprint(&self) -> usize {
        match self {
            Texture::Full(texture) => texture
                .format()
                .theoretical_memory_footprint(texture.size())
                as usize,
            Texture::Packed(texture, format) => {
                let size = texture.size();
                format.width() as usize * size.x as usize * size.y as usize
            }
            Texture::Empty(_) => 0,
        }
    }

    pub fn width(&self) -> u32 {
        match self {
            Texture::Full(texture) => texture.width(),
            Texture::Packed(packed, _) => packed.size().x,
            Texture::Empty(_) => 0,
        }
    }

    pub fn height(&self) -> u32 {
        match self {
            Texture::Full(texture) => texture.height(),
            Texture::Packed(packed, _) => packed.size().y,
            Texture::Empty(_) => 0,
        }
    }

    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Texture::Full(texture) => match texture.format() {
                wgpu::TextureFormat::Bgra8Unorm => PixelFormat::Bgra,
                wgpu::TextureFormat::R8Unorm => PixelFormat::Mono,
                wgpu::TextureFormat::R32Float => PixelFormat::Mono,
                _ => unreachable!(),
            },
            &Texture::Packed(_, pixel_format) => pixel_format,
            &Texture::Empty(pixel_format) => pixel_format,
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
        super::TextureInner::Wgpu(Texture::Empty(_)) => None,
        super::TextureInner::Wgpu(Texture::Full(texture)) => {
            Some(UnwrappedTexture::from_texture_region(
                texture,
                Point2::ZERO,
                Vec2::new(texture.width(), texture.height()),
            ))
        }
        &super::TextureInner::Wgpu(Texture::Packed(ref packed, format)) => {
            let (texture, position, size) = packed.get_texture_region(packers.for_format(format));
            Some(UnwrappedTexture::from_texture_region(
                texture, position, size,
            ))
        }
        target => panic!(
            "Incompatible texture {:?} passed to wgpu rasterizer",
            target.variant_name()
        ),
    }
}

fn unwrap_wgpu_render_target<'a>(target: &'a mut super::RenderTarget<'_>) -> &'a mut RenderTarget {
    match &mut target.0 {
        super::RenderTargetInner::Wgpu(target) => target,
        target => panic!(
            "Incompatible render target {:?} passed to wgpu rasterizer",
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
            fill_pipeline: make_stroke_or_fill_pipeline(
                wgpu::PrimitiveTopology::TriangleStrip,
                "triangle fill pipeline",
            ),
            fill_bind_group_layout,

            packers: Packers {
                mono: TexturePacker::new(device.clone(), wgpu::TextureFormat::R8Unorm),
                bgra: TexturePacker::new(device.clone(), wgpu::TextureFormat::Bgra8Unorm),
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
        super::RenderTarget(super::RenderTargetInner::Wgpu(Box::new(RenderTarget(
            texture,
        ))))
    }

    pub fn begin_frame<'f>(&'f mut self) -> FrameRasterizer<'f> {
        let render_command_encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut secondary_command_encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.packers.mono.defragment(&mut secondary_command_encoder);
        self.packers.bgra.defragment(&mut secondary_command_encoder);

        FrameRasterizer {
            uniform_belt: UniformBelt::new(self.device.clone(), INITIAL_UNIFORM_CHUNK_CAPACITY),
            render_command_encoder,
            secondary_command_encoder,
            parent: self,
        }
    }
}

struct UniformBelt {
    device: wgpu::Device,
    full_buffers: Vec<wgpu::Buffer>,
    active_buffer: wgpu::Buffer,
    current_offset: u32,
    min_uniform_alignment: u32,
    inserted_slices: HashMap<Box<[u8]>, (u8, u32)>,
}

impl UniformBelt {
    pub fn new(device: wgpu::Device, capacity: u32) -> Self {
        Self {
            full_buffers: Vec::new(),
            active_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("uniform buffer"),
                size: u64::from(capacity),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: true,
            }),
            current_offset: 0,
            min_uniform_alignment: device.limits().min_uniform_buffer_offset_alignment,
            inserted_slices: HashMap::new(),
            device,
        }
    }

    fn alloc_address(&mut self, size: u32, aligned: bool) -> u32 {
        let current_offset = self.current_offset;
        let aligned_offset = if aligned {
            current_offset.next_multiple_of(self.min_uniform_alignment)
        } else {
            current_offset
        };
        let next_offset = aligned_offset + size;
        if u64::from(next_offset) > self.active_buffer.size() {
            self.active_buffer.unmap();
            let new_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("uniform buffer"),
                size: (self.active_buffer.size() * 2).max(2 * u64::from(size)),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::UNIFORM,
                mapped_at_creation: true,
            });
            let old_buffer = std::mem::replace(&mut self.active_buffer, new_buffer);
            self.full_buffers.push(old_buffer);
        }
        self.current_offset = next_offset;

        aligned_offset
    }

    pub fn write_slice(&mut self, data: &[u8], aligned: bool) -> wgpu::BufferSlice<'_> {
        if let Some(&(buf, offset)) = self.inserted_slices.get(data) {
            let buffer = if usize::from(buf) == self.full_buffers.len() {
                &self.active_buffer
            } else {
                &self.full_buffers[usize::from(buf)]
            };
            return buffer.slice(u64::from(offset)..u64::from(offset) + data.len() as u64);
        }

        let address = self.alloc_address(data.len() as u32, aligned);
        let buffer_slice = self
            .active_buffer
            .slice(u64::from(address)..u64::from(address) + data.len() as u64);
        buffer_slice.get_mapped_range_mut().copy_from_slice(data);
        self.inserted_slices
            .insert(data.into(), (self.full_buffers.len() as u8, address));
        buffer_slice
    }

    pub fn finish(self) {
        self.active_buffer.unmap();
    }
}

pub struct FrameRasterizer<'f> {
    parent: &'f mut Rasterizer,
    uniform_belt: UniformBelt,
    render_command_encoder: wgpu::CommandEncoder,
    secondary_command_encoder: wgpu::CommandEncoder,
}

struct TargetRenderPass<'p> {
    target: &'p mut RenderTarget,
    render_pass: wgpu::RenderPass<'p>,
}

impl FrameRasterizer<'_> {
    pub fn end_frame(self) {
        self.parent.blitter.clear_bind_group_cache();
        self.uniform_belt.finish();
        self.parent.queue.submit([
            self.secondary_command_encoder.finish(),
            self.render_command_encoder.finish(),
        ]);
    }

    fn fill_triangles<const N: usize>(
        &mut self,
        pass: &mut TargetRenderPass,
        vertices: &[Point2f; N],
        color: BGRA8,
    ) {
        let data = vertices.map(|point| target_transform_point_for(pass.target, point));

        pass.render_pass.set_vertex_buffer(
            0,
            self.uniform_belt.write_slice(
                unsafe {
                    std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), size_of_val(&data))
                },
                false,
            ),
        );

        let uniform_data = [
            // color
            color.r as f32 / 256.0,
            color.g as f32 / 256.0,
            color.b as f32 / 256.0,
            color.a as f32 / 256.0,
        ];

        let uniform_slice = self.uniform_belt.write_slice(
            unsafe {
                std::slice::from_raw_parts(
                    uniform_data.as_ptr().cast::<u8>(),
                    size_of_val(&uniform_data),
                )
            },
            true,
        );

        pass.render_pass.set_pipeline(&self.parent.fill_pipeline);
        pass.render_pass.set_bind_group(
            0,
            Some(
                &self
                    .parent
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("triangle fill bind group"),
                        layout: &self.parent.fill_bind_group_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: uniform_slice.buffer(),
                                offset: uniform_slice.offset(),
                                size: Some(uniform_slice.size()),
                            }),
                        }],
                    }),
            ),
            &[],
        );
        pass.render_pass.draw(0..vertices.len() as u32, 0..1);
    }

    fn fill_rect(&mut self, pass: &mut TargetRenderPass, rect: Rect2f, color: BGRA8) {
        // TODO: Anti aliased rectangle drawing
        self.fill_triangles(
            pass,
            &[
                rect.min,
                Point2f::new(rect.max.x, rect.min.y),
                Point2f::new(rect.min.x, rect.max.y),
                rect.max,
            ],
            color,
        );
    }

    fn draw_bitmap(&mut self, pass: &mut TargetRenderPass, bitmap: &scene::Bitmap) {
        let Some(unwrapped) = unwrap_wgpu_texture(&bitmap.texture, &self.parent.packers) else {
            return;
        };

        self.parent.blitter.blit(
            &self.parent.device,
            &mut self.uniform_belt,
            &mut pass.render_pass,
            PixelFormat::Bgra,
            pass.target.size(),
            bitmap.pos.x,
            bitmap.pos.y,
            &unwrapped,
            bitmap.color,
            match bitmap.filter {
                Some(scene::BitmapFilter::ExtractAlpha) => true,
                None => false,
            },
        );
    }
}

#[inline]
fn target_transform_point_for(target: &RenderTarget, p: Point2f) -> Point2f {
    target_transform_point(target.size(), p)
}

#[inline]
fn target_transform_point(size: Vec2<u32>, p: Point2f) -> Point2f {
    Point2::new(
        (p.x / size.x as f32) * 2.0 - 1.0,
        -(p.y / size.y as f32) * 2.0 + 1.0,
    )
}

#[inline]
fn target_transform_rect(size: Vec2<u32>, p: Rect2f) -> Rect2f {
    Rect2f::new(
        target_transform_point(size, p.min),
        target_transform_point(size, p.max),
    )
}

#[inline]
fn rect_to_triangle_strip(rect: &Rect2f) -> [Point2f; 4] {
    [
        rect.min,
        Point2f::new(rect.max.x, rect.min.y),
        Point2f::new(rect.min.x, rect.max.y),
        rect.max,
    ]
}

impl FrameRasterizer<'_> {
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
            return super::Texture(super::TextureInner::Wgpu(Texture::Empty(format)));
        }

        let byte_stride = (width * u32::from(format.width()))
            .next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);

        let buffer = self.parent.device.create_buffer(&wgpu::BufferDescriptor {
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
            let packer = self.parent.packers.for_format_mut(format);

            Texture::Packed(
                packer.add_from_buffer(
                    &mut self.secondary_command_encoder,
                    &buffer,
                    byte_stride,
                    width,
                    height,
                ),
                format,
            )
        } else {
            let texture = self.parent.device.create_texture(&wgpu::TextureDescriptor {
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
            self.secondary_command_encoder.copy_buffer_to_texture(
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

            Texture::Full(texture)
        };

        super::Texture(super::TextureInner::Wgpu(inner))
    }
}

impl super::Rasterizer for FrameRasterizer<'_> {
    fn name(&self) -> &'static str {
        "wgpu"
    }

    fn write_debug_info(&self, writer: &mut dyn std::fmt::Write) -> std::fmt::Result {
        if let Some(info) = self.parent.adapter_info.as_ref() {
            writeln!(writer, "adapter: {} ({})", info.name, info.driver)?;
        }

        let packers = &self.parent.packers;
        for (name, packer) in [("mono", &packers.mono), ("bgra", &packers.bgra)] {
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

    fn blur_texture(&mut self, texture: &super::Texture, blur_sigma: f32) -> super::BlurOutput {
        let Some(unwrapped) = unwrap_wgpu_texture(texture, &self.parent.packers) else {
            return super::BlurOutput {
                padding: Vec2::ZERO,
                texture: super::Texture(super::TextureInner::Wgpu(Texture::Empty(
                    PixelFormat::Mono,
                ))),
            };
        };

        let radius = gaussian_sigma_to_box_radius(blur_sigma) as u32;
        let padding = 2 * radius;
        let twidth = texture.width() + 2 * 2 * radius;
        let theight = texture.height() + 2 * 2 * radius;
        let front_texture = self.parent.device.create_texture(&wgpu::TextureDescriptor {
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

        let mut blit_pass =
            self.secondary_command_encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("blur front buffer render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &front_texture.create_view(&wgpu::TextureViewDescriptor {
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
                });

        self.parent.blitter.blit(
            &self.parent.device,
            &mut self.uniform_belt,
            &mut blit_pass,
            PixelFormat::Mono,
            Vec2::new(twidth, theight),
            padding as i32,
            padding as i32,
            &unwrapped,
            BGRA8::WHITE,
            true,
        );

        drop(blit_pass);

        self.blur_texture_in_place(&front_texture, radius);

        super::BlurOutput {
            padding: Vec2::splat(padding),
            texture: super::Texture(super::TextureInner::Wgpu(Texture::Full(front_texture))),
        }
    }

    fn render_scene(
        &mut self,
        target: &mut super::RenderTarget,
        scene: &[SceneNode],
        user_data: &(dyn std::any::Any + 'static),
    ) -> Result<(), super::SceneRenderError> {
        let target = unwrap_wgpu_render_target(target);
        let mut pass = TargetRenderPass {
            render_pass: self
                .render_command_encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.0.create_view(&wgpu::TextureViewDescriptor {
                            label: None,
                            format: Some(target.0.format()),
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
                .forget_lifetime(),
            target,
        };

        self.render_scene_at(Vec2::ZERO, &mut pass, scene, user_data)
    }
}

impl FrameRasterizer<'_> {
    fn blur_texture_in_place(&mut self, front_texture: &wgpu::Texture, radius: u32) {
        let back_texture = self.parent.device.create_texture(&wgpu::TextureDescriptor {
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

        let mut pass =
            self.secondary_command_encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("blur compute pass"),
                    timestamp_writes: None,
                });
        pass.set_pipeline(&self.parent.blur_pipeline);

        let set_pass_params = |pass: &mut wgpu::ComputePass,
                               params: &wgpu::BufferSlice,
                               front: &wgpu::Texture,
                               back: &wgpu::Texture| {
            pass.set_bind_group(
                0,
                Some(
                    &self
                        .parent
                        .device
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &self.parent.blur_bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(
                                        &front.create_view(&wgpu::TextureViewDescriptor {
                                            usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                                            ..Default::default()
                                        }),
                                    ),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                        buffer: params.buffer(),
                                        offset: params.offset(),
                                        size: Some(params.size()),
                                    }),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 2,
                                    resource: wgpu::BindingResource::TextureView(
                                        &back.create_view(&wgpu::TextureViewDescriptor {
                                            usage: Some(wgpu::TextureUsages::STORAGE_BINDING),
                                            ..Default::default()
                                        }),
                                    ),
                                },
                            ],
                        }),
                ),
                &[],
            );
        };

        let horizontal_params = {
            #[rustfmt::skip]
            let data = [
                // cross_axis: vec2<u32>
                0, 1,
                // radius: u32
                radius,
                // padding
                0,
            ];

            self.uniform_belt
                .write_slice(unsafe { cast_bytes(&data) }, true)
        };
        set_pass_params(&mut pass, &horizontal_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.height() + 0x3F) >> 6, 1, 1);

        let vertical_params = {
            #[rustfmt::skip]
            let data = [
                // cross_axis: vec2<u32>
                1, 0,

                // radius: u32
                radius,

                // padding
                0,
            ];

            self.uniform_belt
                .write_slice(unsafe { cast_bytes(&data) }, true)
        };
        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, front_texture, &back_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups((front_texture.width() + 0x3F) >> 6, 1, 1);
    }

    fn render_scene_at(
        &mut self,
        offset: Vec2S,
        pass: &mut TargetRenderPass,
        scene: &[SceneNode],
        user_data: &(dyn std::any::Any + 'static),
    ) -> Result<(), super::SceneRenderError> {
        for node in scene {
            match node {
                SceneNode::DeferredBitmaps(deferred_bitmaps) => {
                    let bitmaps = (deferred_bitmaps.to_bitmaps)(self, user_data)
                        .map_err(super::SceneRenderErrorInner::ToBitmaps)?;
                    for bitmap in bitmaps {
                        self.draw_bitmap(pass, &bitmap);
                    }
                }
                SceneNode::Bitmap(bitmap) => {
                    self.draw_bitmap(pass, bitmap);
                }
                SceneNode::StrokedPolyline(polyline) => {
                    let bitmap = polyline.to_bitmap(offset.to_point(), self);
                    self.draw_bitmap(pass, &bitmap);
                }
                &SceneNode::FilledRect(FilledRect { rect, color }) => {
                    self.fill_rect(pass, Rect2::to_float(rect), color);
                }
                SceneNode::Subscene(subscene) => {
                    self.render_scene_at(
                        offset + subscene.pos.to_vec(),
                        pass,
                        &subscene.scene,
                        user_data,
                    )?;
                }
            }
        }

        Ok(())
    }
}
