struct ComputeContext {
    // Cross axis, orthogonal to the one we compute the moving average on.
    cross_axis: vec2<u32>,
    radius: u32,
}

@group(0) @binding(0) var back_texture: texture_2d<f32>;
@group(0) @binding(1) var<uniform> cctx: ComputeContext;
@group(0) @binding(2) var out_texture: texture_storage_2d<r32float, write>;

@compute
@workgroup_size(64)
fn cs_main(
    @builtin(global_invocation_id) id: vec3<u32>,
) {
    let cross = id.x;

    // Main axis on which compute the moving average.
    let main_axis = vec2u(1, 1) - cctx.cross_axis;
    let step = main_axis;

    let size = textureDimensions(out_texture);
    let end = dot(size, main_axis);
    let start = cctx.cross_axis * cross;
    let radiusStep = step * cctx.radius;
    let iextent = 1.0 / f32(2 * cctx.radius + 1);
    var sum = 0.0;

    if(cross >= dot(size, cctx.cross_axis)) {
        return;
    }

    var current = start;
    for (var i: u32 = 0; i < cctx.radius; i += 1) {
        sum += textureLoad(back_texture, current, 0).x;
    }

    current = start;
    var x: u32 = 0;
    for (; x < cctx.radius; x += 1) {
        let right = textureLoad(back_texture, current + radiusStep, 0).x;
        sum += right;
        textureStore(out_texture, current, vec4f(sum * iextent, 0.0, 0.0, 1.0));
        current += step;
    }

    for (; x < end - cctx.radius; x += 1) {
        let left = textureLoad(back_texture, current - radiusStep, 0).x;
        let right = textureLoad(back_texture, current + radiusStep, 0).x;
        sum += right;
        textureStore(out_texture, current, vec4f(sum * iextent, 0.0, 0.0, 1.0));
        sum -= left;
        current += step;
    }

    for (; x < end; x += 1) {
        let left = textureLoad(back_texture, current - radiusStep, 0).x;
        textureStore(out_texture, current, vec4f(sum * iextent, 0.0, 0.0, 1.0));
        sum -= left;
        current += step;
    }
}
