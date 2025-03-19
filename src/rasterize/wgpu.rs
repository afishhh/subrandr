use std::mem::MaybeUninit;

use wgpu::{include_wgsl, util::DeviceExt, vertex_attr_array};

use crate::{
    color::BGRA8,
    math::{Point2, Vec2},
};

use super::sw::gaussian_sigma_to_box_radius;

pub struct GpuRasterizer {
    device: wgpu::Device,
    queue: wgpu::Queue,

    stroke_bind_group_layout: wgpu::BindGroupLayout,
    stroke_pipeline: wgpu::RenderPipeline,

    blitter: Blitter,

    blur_bind_group_layout: wgpu::BindGroupLayout,
    blur_pipeline: wgpu::ComputePipeline,
    blur_state: Option<BlurState>,
}

struct Blitter {
    nearest_sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_color_to_bgra8u: wgpu::RenderPipeline,
    pipeline_mono_to_bgra8u: wgpu::RenderPipeline,
    pipeline_mono_to_mono32f: wgpu::RenderPipeline,
    pipeline_alpha_to_mono32f: wgpu::RenderPipeline,
}

impl Blitter {
    pub fn new(device: &wgpu::Device) -> Self {
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
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
                    buffers: &[],
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

            pipeline_mono_to_bgra8u: pipeline_for_fragment_with_name(
                "mono8 blit pipeline",
                "fs_main_mono8",
                wgpu::TextureFormat::Bgra8Unorm,
                true,
            ),
            pipeline_color_to_bgra8u: pipeline_for_fragment_with_name(
                "bgra8 blit pipeline",
                "fs_main_bgra8",
                wgpu::TextureFormat::Bgra8Unorm,
                true,
            ),
            pipeline_mono_to_mono32f: pipeline_for_fragment_with_name(
                "mono to mono blit pipeline",
                "fs_main_mono_to_mono",
                wgpu::TextureFormat::R32Float,
                false,
            ),
            pipeline_alpha_to_mono32f: pipeline_for_fragment_with_name(
                "alpha to mono blit pipeline",
                "fs_main_alpha_to_mono",
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
    front_texture: wgpu::Texture,
    radius: u32,
}

#[derive(Debug)]
pub struct GpuRenderTargetHandle {
    tex: wgpu::Texture,
    pass: wgpu::RenderPass<'static>,
    encoder: wgpu::CommandEncoder,
}

impl GpuRasterizer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let stroke_module = device.create_shader_module(include_wgsl!("./wgpu/stroke.wgsl"));
        let stroke_bind_group_layout =
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

        let blur_module = device.create_shader_module(include_wgsl!("./wgpu/blur.wgsl"));
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

        Self {
            stroke_pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shape stroke pipeline"),
                layout: Some(
                    &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[&stroke_bind_group_layout],
                        push_constant_ranges: &[],
                    }),
                ),
                vertex: wgpu::VertexState {
                    module: &stroke_module,
                    entry_point: Some("polygon_stroke_vert"),
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
                    topology: wgpu::PrimitiveTopology::LineStrip,
                    strip_index_format: None,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &stroke_module,
                    entry_point: Some("polygon_stroke_frag"),
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
            }),
            stroke_bind_group_layout,

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
        }
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
                        format: Some(wgpu::TextureFormat::Bgra8Unorm),
                        dimension: Some(wgpu::TextureViewDimension::D2),
                        usage: Some(wgpu::TextureUsages::RENDER_ATTACHMENT),
                        aspect: wgpu::TextureAspect::All,
                        ..Default::default()
                    }),
                    resolve_target: None,
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

        super::RenderTarget {
            width: texture.width(),
            height: texture.height(),
            handle: super::RenderTargetHandle::Gpu(GpuRenderTargetHandle {
                tex: texture,
                pass,
                encoder,
            }),
        }
    }

    fn pass_from_target<'a>(
        handle: &'a mut super::RenderTargetHandle,
    ) -> &'a mut wgpu::RenderPass<'static> {
        match handle {
            super::RenderTargetHandle::Gpu(GpuRenderTargetHandle { pass, .. }) => pass,
            handle => panic!("Unexpected render target passed to gpu rasterizer: {handle:?}"),
        }
    }

    fn texture_from_texture<'a>(&self, texture: &'a super::Texture) -> &'a wgpu::Texture {
        match &texture.handle {
            super::TextureDataHandle::Gpu(texture) => texture,
            handle => panic!("Unexpected texture handle passed to gpu rasterizer: {handle:?}"),
        }
    }
}

macro_rules! pass_from_target {
    ($self: ident, $target: expr) => {
        Self::pass_from_target(&mut $target.handle)
    };
}

impl GpuRasterizer {
    fn stroke_polyline_or_polygon(
        &mut self,
        target: &mut super::RenderTarget<'_>,
        offset: Vec2,
        vertices: &[Point2],
        closed: bool,
        color: BGRA8,
    ) {
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
                        data.len() * size_of::<Point2>(),
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

        let pass = pass_from_target!(self, target);
        pass.set_pipeline(&self.stroke_pipeline);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.set_bind_group(
            0,
            Some(&self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("line stroke bind group"),
                layout: &self.stroke_bind_group_layout,
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
        pass.draw(0..data.len() as u32, 0..1);
    }
}

#[inline]
fn target_transform_point_for(target: &super::RenderTarget, p: Point2) -> Point2 {
    target_transform_point(target.width, target.height, p)
}

#[inline]
fn target_transform_point(width: u32, height: u32, p: Point2) -> Point2 {
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

    pub fn write<T: bytemuck::Pod>(&mut self, data: &T) {
        let bytes = bytemuck::bytes_of(data);
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

impl super::Rasterizer for GpuRasterizer {
    fn downcast_gpu(&mut self) -> Option<&mut GpuRasterizer> {
        Some(self)
    }

    fn copy_or_move_into_texture(
        &mut self,
        width: u32,
        height: u32,
        data: super::sw::CpuTextureData,
    ) -> super::Texture {
        let wgpu_format = match data.format() {
            crate::rasterize::TextureFormat::Bgra => wgpu::TextureFormat::Bgra8Unorm,
            crate::rasterize::TextureFormat::Mono => wgpu::TextureFormat::R8Unorm,
        };

        super::Texture {
            width,
            height,
            format: data.format(),
            handle: super::TextureDataHandle::Gpu(self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
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
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[wgpu_format],
                },
                wgpu::util::TextureDataOrder::default(),
                data.bytes(),
            )),
        }
    }

    fn submit_render(&mut self, target: super::RenderTarget) {
        match target.handle {
            super::RenderTargetHandle::Gpu(handle) => {
                drop(handle.pass);
                self.queue.submit([handle.encoder.finish()]);
            }
            handle => panic!("Unexpected render target passed to gpu rasterizer: {handle:?}"),
        }
    }

    fn blit(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        texture: &super::Texture,
        color: BGRA8,
    ) {
        self.blitter.do_blit(
            &self.device,
            Self::pass_from_target(&mut target.handle),
            super::TextureFormat::Bgra,
            target.width,
            target.height,
            dx,
            dy,
            self.texture_from_texture(texture),
            texture.format,
            color,
        );
    }

    fn line(
        &mut self,
        target: &mut super::RenderTarget,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(
            target,
            Vec2::ZERO,
            &[Point2::new(x0, y0), Point2::new(x1, y1)],
            false,
            color,
        );
    }

    fn horizontal_line(
        &mut self,
        target: &mut super::RenderTarget,
        y: f32,
        x0: f32,
        x1: f32,
        color: crate::color::BGRA8,
    ) {
        self.line(target, x0, y, x1, y, color);
    }

    fn stroke_polyline(
        &mut self,
        target: &mut super::RenderTarget,
        offset: Vec2,
        vertices: &[crate::math::Point2],
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(target, offset, vertices, false, color);
    }

    fn stroke_polygon(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[crate::math::Point2],
        color: crate::color::BGRA8,
    ) {
        self.stroke_polyline_or_polygon(target, Vec2::ZERO, vertices, true, color);
    }

    fn fill_triangle(
        &mut self,
        target: &mut super::RenderTarget,
        vertices: &[crate::math::Point2; 3],
        color: crate::color::BGRA8,
    ) {
        // println!("TOOD: fill_triangle")
    }

    fn polygon_reset(&mut self, offset: crate::math::Vec2) {
        // println!("TOOD: polygon_reset")
    }

    fn polygon_add_polyline(&mut self, vertices: &[crate::math::Point2], winding: bool) {
        // println!("TOOD: polygon_add_polyline")
    }

    fn polygon_fill(&mut self, target: &mut super::RenderTarget, color: crate::color::BGRA8) {
        // println!("TOOD: polygon_fill")
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
                        ops: wgpu::Operations::default(),
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime(),
            encoder,
            front_texture: texture,
            radius,
        })
    }

    fn blur_buffer_blit(&mut self, dx: i32, dy: i32, texture: &super::Texture) {
        let tex = &self.texture_from_texture(texture);
        let state = self
            .blur_state
            .as_mut()
            .expect("GpuRasterizer::blur_buffer_blit called without an active blur pass");
        self.blitter.do_blit(
            &self.device,
            &mut state.blit_pass,
            super::TextureFormat::Mono,
            state.front_texture.width(),
            state.front_texture.height(),
            dx + (2 * state.radius) as i32,
            dy + (2 * state.radius) as i32,
            tex,
            texture.format,
            BGRA8::WHITE,
        );
    }

    fn blur_execute(&mut self, target: &mut super::RenderTarget, dx: i32, dy: i32, color: [u8; 3]) {
        let mut state = self
            .blur_state
            .take()
            .expect("GpuRasterizer::blur_buffer_blit called without an active blur pass");

        drop(state.blit_pass);

        self.do_blur(&mut state.encoder, &state.front_texture, state.radius);

        let pass = Self::pass_from_target(&mut target.handle);

        self.queue.submit([state.encoder.finish()]);

        self.blitter.do_blit(
            &self.device,
            pass,
            super::TextureFormat::Bgra,
            target.width,
            target.height,
            dx - (2 * state.radius) as i32,
            dy - (2 * state.radius) as i32,
            &state.front_texture,
            super::TextureFormat::Mono,
            BGRA8::new(color[2], color[1], color[0], 255),
        );
    }
}

impl GpuRasterizer {
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
                builder.write(&[0u32, 1]);

                // radius: u32
                builder.write(&radius);

                // padding
                builder.write(&0u32);

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
                builder.write(&[1u32, 0]);

                // radius: u32
                builder.write(&radius);

                // padding
                builder.write(&0u32);

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
        pass.dispatch_workgroups(front_texture.height() << 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, &back_texture, front_texture);
        pass.dispatch_workgroups(front_texture.height() << 6, 1, 1);
        set_pass_params(&mut pass, &horizontal_params, front_texture, &back_texture);
        pass.dispatch_workgroups(front_texture.height() << 6, 1, 1);

        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups(front_texture.width() << 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, front_texture, &back_texture);
        pass.dispatch_workgroups(front_texture.width() << 6, 1, 1);
        set_pass_params(&mut pass, &vertical_params, &back_texture, front_texture);
        pass.dispatch_workgroups(front_texture.width() << 6, 1, 1);
    }
}

impl Blitter {
    fn do_blit(
        &self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass,
        pass_format: super::TextureFormat,
        twidth: u32,
        theight: u32,
        dx: i32,
        dy: i32,
        texture: &wgpu::Texture,
        source_format: super::TextureFormat,
        color: BGRA8,
    ) {
        let data = {
            let mut builder = StructBuilder::<32>::new();

            builder.write(&target_transform_point(
                twidth,
                theight,
                Point2::new(dx as f32, dy as f32),
            ));

            builder.write(&Point2::new(
                texture.width() as f32 / twidth as f32,
                -(texture.height() as f32 / theight as f32),
            ));

            builder.write(&[
                color.r as f32 / 255.0,
                color.g as f32 / 255.0,
                color.b as f32 / 255.0,
                color.a as f32 / 255.0,
            ]);

            builder.finish()
        };

        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &data,
            usage: wgpu::BufferUsages::UNIFORM,
        });

        match (pass_format, source_format) {
            (super::TextureFormat::Bgra, super::TextureFormat::Bgra) => {
                pass.set_pipeline(&self.pipeline_color_to_bgra8u);
            }
            (super::TextureFormat::Bgra, super::TextureFormat::Mono) => {
                pass.set_pipeline(&self.pipeline_mono_to_bgra8u);
            }
            (super::TextureFormat::Mono, super::TextureFormat::Bgra) => {
                pass.set_pipeline(&self.pipeline_alpha_to_mono32f);
            }
            (super::TextureFormat::Mono, super::TextureFormat::Mono) => {
                pass.set_pipeline(&self.pipeline_mono_to_mono32f);
            }
        }

        pass.set_bind_group(
            0,
            Some(&device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&texture.create_view(
                            &wgpu::TextureViewDescriptor {
                                usage: Some(wgpu::TextureUsages::TEXTURE_BINDING),
                                ..Default::default()
                            },
                        )),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: None,
                        }),
                    },
                ],
            })),
            &[],
        );
        pass.draw(0..4, 0..1);
    }
}
