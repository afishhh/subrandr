struct BlitContext {
    pos: vec2<f32>,
    size: vec2<f32>,
    color: vec4<f32>,
}

@group(0) @binding(0) var blit_sampler: sampler;
@group(0) @binding(1) var blit_texture: texture_2d<f32>;
@group(0) @binding(2) var<uniform> ctx: BlitContext;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) coord: vec2<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) index: u32,
) -> VertexOutput {
    let vertices = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0),
        vec2<f32>(1.0, 0.0),
    );

    var out: VertexOutput;
    out.coord = vertices[index];
    out.position = vec4<f32>(
        ctx.pos + (out.coord * 2.0 - vec2f(1.0, 1.0)) * ctx.size + ctx.size,
        0.0, 1.0
    );
    return out;
}

@fragment
fn fs_main_mono_to_bgra(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    let sample = textureSample(blit_texture, blit_sampler, in.coord).x;
    return ctx.color * sample;
}

@fragment
fn fs_main_bgra_to_bgra(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    var sample = textureSample(blit_texture, blit_sampler, in.coord);
    sample.w *= ctx.color.w;
    return sample;
}

@fragment
fn fs_main_mono_to_mono(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return textureSample(blit_texture, blit_sampler, in.coord);
}

@fragment
fn fs_main_xxxa_to_mono(
    in: VertexOutput
) -> @location(0) vec4<f32> {
    return vec4f(
        textureSample(blit_texture, blit_sampler, in.coord).a,
        0.0,
        0.0,
        1.0
    );
}
