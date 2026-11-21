// The upscaler's debug views (config `upscale_debug`): an input shown in place of the scene, written linear at
// render size (the scaling pass encodes gamma).
struct U {
    // Render size, mode (1 motion, 2 depth, 3 reactive, 4 composition), near.
    size: vec4<f32>,
    // Far, the motion scale (pixels to full colour), the depth view's range (units).
    far: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var motion: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var reactive: texture_2d<f32>;
@group(0) @binding(4) var composition: texture_2d<f32>;
@group(0) @binding(5) var out: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= u32(u.size.x) || id.y >= u32(u.size.y)) { return; }
    let p = vec2<i32>(id.xy);
    let mode = u32(u.size.z);
    var c = vec3<f32>(0.0);
    if (mode == 1u) {
        // Direction as hue around grey, length as saturation.
        let mv = textureLoad(motion, p, 0).xy * u.far.y;
        c = clamp(vec3<f32>(0.5 + mv.x, 0.5 + mv.y, 0.5 - 0.5 * (mv.x + mv.y)), vec3<f32>(0.0), vec3<f32>(1.0));
    } else if (mode == 2u) {
        // View distance on a log scale, near white, far black.
        let d = textureLoad(depth, p, 0);
        let n = u.size.w;
        let f = u.far.x;
        let z = n * f / (f - d * (f - n));
        c = vec3<f32>(1.0 - clamp(log2(z / n) / log2(u.far.z / n), 0.0, 1.0));
    } else if (mode == 3u) {
        c = vec3<f32>(textureLoad(reactive, p, 0).r, 0.0, 0.0);
    } else {
        c = vec3<f32>(0.0, textureLoad(composition, p, 0).r, 0.0);
    }
    textureStore(out, p, vec4<f32>(pow(c, vec3<f32>(2.2)), 1.0));
}
