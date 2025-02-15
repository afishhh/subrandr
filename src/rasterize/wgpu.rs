use std::mem::MaybeUninit;

use wgpu::{include_wgsl, util::DeviceExt, vertex_attr_array, RenderPass, Texture};

use crate::{
    color::BGRA8,
    math::{Point2, Vec2},
};

pub struct GpuRasterizer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    encoder: Option<wgpu::CommandEncoder>,

    current_render_pass: Option<(RenderPass<'static>, Texture)>,

    stroke_bind_group_layout: wgpu::BindGroupLayout,
    stroke_pipeline: wgpu::RenderPipeline,

    blit_sampler: wgpu::Sampler,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_pipeline_bgra8: wgpu::RenderPipeline,
    blit_pipeline_mono8: wgpu::RenderPipeline,
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

        let blit_module = device.create_shader_module(include_wgsl!("./wgpu/blit.wgsl"));
        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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

        let blit_pipeline_for_fragment_with_name =
            |fragment_entry_point: &'static str| -> wgpu::RenderPipeline {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: Some(
                        &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                            label: None,
                            bind_group_layouts: &[&blit_bind_group_layout],
                            push_constant_ranges: &[],
                        }),
                    ),
                    vertex: wgpu::VertexState {
                        module: &blit_module,
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
                        module: &blit_module,
                        entry_point: Some(fragment_entry_point),
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
            stroke_pipeline: device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: None,
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

            blit_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: None,
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Nearest,
                min_filter: wgpu::FilterMode::Nearest,
                ..Default::default()
            }),
            blit_pipeline_mono8: blit_pipeline_for_fragment_with_name("fs_main_mono8"),
            blit_pipeline_bgra8: blit_pipeline_for_fragment_with_name("fs_main_bgra8"),
            blit_bind_group_layout,

            encoder: None,
            current_render_pass: None,

            device,
            queue,
        }
    }

    pub fn target_from_texture(&self, texture: wgpu::Texture) -> super::RenderTarget<'static> {
        super::RenderTarget {
            width: texture.width(),
            height: texture.height(),
            handle: super::RenderTargetHandle::Gpu(texture),
        }
    }

    fn unwrap_encoder(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder
            .as_mut()
            .expect("GpuRasterizer rendering method called while not in a frame")
    }

    fn texture_from_target<'a>(target: &'a super::RenderTarget) -> &'a wgpu::Texture {
        match &target.handle {
            super::RenderTargetHandle::Gpu(texture) => texture,
            handle => panic!("Unexpected render target passed to gpu rasterizer: {handle:?}"),
        }
    }

    fn pass_from_target<'a>(
        slot: &'a mut Option<(RenderPass<'static>, Texture)>,
        target: &super::RenderTarget,
    ) -> &'a mut wgpu::RenderPass<'static> {
        let texture = Self::texture_from_target(target);

        match slot {
            Some((pass, current_texture)) if current_texture == texture => pass,
            Some((_, current_texture)) => panic!(
                "Tried to render to {texture:?} current render pass is for {current_texture:?}"
            ),
            _ => panic!("Tried to render to {texture:?} without a render pass"),
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
        Self::pass_from_target(&mut $self.current_render_pass, $target)
    };
}

impl GpuRasterizer {
    fn stroke_polyline_or_polygon(
        &mut self,
        target: &super::RenderTarget<'_>,
        offset: Vec2,
        vertices: &[Point2],
        closed: bool,
        color: BGRA8,
    ) {
        let pass = pass_from_target!(self, target);
        pass.set_pipeline(&self.stroke_pipeline);

        let data = {
            let mut result = Vec::with_capacity(vertices.len() + closed as usize);
            result.extend(
                vertices
                    .iter()
                    .map(|&point| target_transform_point(target, point + offset)),
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
fn target_transform_point(target: &super::RenderTarget, p: Point2) -> Point2 {
    Point2::new(
        (p.x / target.width as f32) * 2.0 - 1.0,
        -(p.y / target.height as f32) * 2.0 + 1.0,
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

    fn copy_into_texture(
        &mut self,
        width: u32,
        height: u32,
        format: super::TextureFormat,
        data: &[u8],
    ) -> super::Texture {
        let wgpu_format = match format {
            crate::rasterize::TextureFormat::Bgra8 => wgpu::TextureFormat::Bgra8Unorm,
            crate::rasterize::TextureFormat::Mono8 => wgpu::TextureFormat::R8Unorm,
        };

        super::Texture {
            width,
            height,
            format,
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
                data,
            )),
        }
    }

    fn begin_frame(&mut self) {
        self.encoder = Some(
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None }),
        )
    }

    fn end_frame(&mut self) {
        self.queue.submit([self.encoder.take().unwrap().finish()]);
    }

    fn begin_render_pass(&mut self, target: &mut super::RenderTarget) {
        let texture = Self::texture_from_target(target);

        if self.current_render_pass.is_some() {
            panic!("Cannot begin render pass if already in a render pass")
        }

        self.current_render_pass = Some((
            self.unwrap_encoder()
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
                .forget_lifetime(),
            texture.clone(),
        ));
    }

    fn end_render_pass(&mut self) {
        self.current_render_pass = None;
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
        println!("TOOD: fill_triangle")
    }

    fn polygon_reset(&mut self, offset: crate::math::Vec2) {
        println!("TOOD: polygon_reset")
    }

    fn polygon_add_polyline(&mut self, vertices: &[crate::math::Point2], winding: bool) {
        println!("TOOD: polygon_add_polyline")
    }

    fn polygon_fill(&mut self, target: &mut super::RenderTarget, color: crate::color::BGRA8) {
        println!("TOOD: polygon_fill")
    }

    fn blit(
        &mut self,
        target: &mut super::RenderTarget,
        dx: i32,
        dy: i32,
        source: &super::Texture,
        color: BGRA8,
    ) {
        let texture = self.texture_from_texture(source);
        let pass = pass_from_target!(self, target);

        let data = {
            let mut builder = StructBuilder::<32>::new();

            builder.write(&target_transform_point(
                target,
                Point2::new(dx as f32, dy as f32),
            ));

            builder.write(&Point2::new(
                source.width as f32 / target.width as f32,
                -(source.height as f32 / target.height as f32),
            ));

            builder.write(&[
                color.r as f32 / 255.0,
                color.g as f32 / 255.0,
                color.b as f32 / 255.0,
                color.a as f32 / 255.0,
            ]);

            builder.finish()
        };

        let buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &data,
                usage: wgpu::BufferUsages::UNIFORM,
            });

        match source.format() {
            super::TextureFormat::Bgra8 => {
                pass.set_pipeline(&self.blit_pipeline_bgra8);
            }
            super::TextureFormat::Mono8 => {
                pass.set_pipeline(&self.blit_pipeline_mono8);
            }
        }

        pass.set_bind_group(
            0,
            Some(&self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.blit_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
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
