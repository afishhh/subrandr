@group(0) @binding(0) var blit_sampler: sampler;
@group(0) @binding(1) var blit_texture: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) src_coord: vec2<f32>,
    @interpolate(flat) @location(1) color: vec4<f32>
}

struct VertexInput {
    @location(0) vertex: vec2<f32>,
    @builtin(vertex_index) index: u32,
}

struct InstanceInput {
    @location(1) pos: vec2<f32>,
    @location(2) size: vec2<f32>,
    @location(3) color: vec4<f32>
}

@vertex
fn vs_main(
    vertex: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let vertices = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
    );

    var out: VertexOutput;
    out.src_coord = vertex.vertex;
    out.position = vec4<f32>(
        instance.pos + (vertices[vertex.index] * 2.0 - vec2f(1.0, 1.0)) * instance.size + instance.size,
        0.0, 1.0
    );
    out.color = instance.color;
    return out;
}

@fragment
fn fs_main_mono_to_bgra(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let sample = textureSample(blit_texture, blit_sampler, in.src_coord).x;
    return in.color * sample;
}

@fragment
fn fs_main_bgra_to_bgra(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    var sample = textureSample(blit_texture, blit_sampler, in.src_coord);
    sample.w *= in.color.w;
    return sample;
}

@fragment
fn fs_main_mono_to_mono(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return textureSample(blit_texture, blit_sampler, in.src_coord);
}

@fragment
fn fs_main_xxxa_to_mono(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return vec4f(
        textureSample(blit_texture, blit_sampler, in.src_coord).a,
        0.0,
        0.0,
        1.0
    );
}
