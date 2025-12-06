// TODO: This is terrible (so many pipelines!!)

@group(0) @binding(0) var blit_sampler: sampler;
@group(0) @binding(1) var blit_texture: texture_2d<f32>;

struct VertexInput {
    @location(0) src_vtx: vec2<f32>,
    @location(1) dst_vtx: vec2<f32>,
    @location(2) color: vec4<f32>
}

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) src_coord: vec2<f32>,
    @interpolate(flat) @location(1) color: vec4<f32>
}

@vertex
fn vs_main(
    input: VertexInput,
    @builtin(vertex_index) index: u32,
) -> VertexOutput {
    var out: VertexOutput;
    out.src_coord = input.src_vtx;
    out.position = vec4<f32>(input.dst_vtx, 0.0, 1.0);
    out.color = input.color;
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
    return sample * in.color.w;
}

@fragment
fn fs_main_xxxa_to_bgra(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    var sample = textureSample(blit_texture, blit_sampler, in.src_coord);
    return in.color * sample.w;
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
