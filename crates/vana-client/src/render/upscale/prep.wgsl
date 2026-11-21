// The upscaler's inputs from the finished scene (render/upscale/mod.rs), one thread per render pixel: linear colour,
// motion vectors from the camera (previous minus current, render pixels, jitter removed), the reactive and the
// composition masks from the colour snapshots.
struct U {
    // Jittered clip -> world (the depth was drawn jittered).
    inv_vp: mat4x4<f32>,
    // World -> current and previous clip without the jitter.
    vp: mat4x4<f32>,
    prev_vp: mat4x4<f32>,
    // Render size, then (reactive scale, composition scale).
    size: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var scene: texture_2d<f32>;
@group(0) @binding(2) var depth: texture_depth_2d;
// Snapshots: after the opaque scenery, after the blended scenery, before the effects.
@group(0) @binding(3) var snap_opaque: texture_2d<f32>;
@group(0) @binding(4) var snap_blend: texture_2d<f32>;
@group(0) @binding(5) var snap_pre_fx: texture_2d<f32>;
@group(0) @binding(6) var out_color: texture_storage_2d<rgba16float, write>;
@group(0) @binding(7) var out_motion: texture_storage_2d<rg16float, write>;
@group(0) @binding(8) var out_reactive: texture_storage_2d<r8unorm, write>;
@group(0) @binding(9) var out_composition: texture_storage_2d<r8unorm, write>;
// The moving actors' motion (render/mod.rs draw_motion), taken where alpha is 1.
@group(0) @binding(10) var object_motion: texture_2d<f32>;

fn diff(a: vec3<f32>, b: vec3<f32>) -> f32 {
    let d = abs(a - b);
    return max(d.r, max(d.g, d.b));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(u.size.xy);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let p = vec2<i32>(id.xy);
    let c = textureLoad(scene, p, 0).rgb;
    textureStore(out_color, p, vec4<f32>(pow(c, vec3<f32>(2.2)), 1.0));

    let d = textureLoad(depth, p, 0);
    let uv = (vec2<f32>(id.xy) + 0.5) / u.size.xy;
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let wp = u.inv_vp * vec4<f32>(ndc, d, 1.0);
    let world = wp.xyz / wp.w;
    let cur = u.vp * vec4<f32>(world, 1.0);
    let prev = u.prev_vp * vec4<f32>(world, 1.0);
    var mv = vec2<f32>(0.0);
    if (cur.w > 1e-6 && prev.w > 1e-6) {
        let dn = prev.xy / prev.w - cur.xy / cur.w;
        mv = vec2<f32>(dn.x * 0.5, -dn.y * 0.5) * u.size.xy;
    }
    let om = textureLoad(object_motion, p, 0);
    if (om.a > 0.5) {
        mv = om.rg;
    }
    textureStore(out_motion, p, vec4<f32>(mv, 0.0, 0.0));

    let reactive = diff(textureLoad(snap_pre_fx, p, 0).rgb, c) * u.size.z;
    textureStore(out_reactive, p, vec4<f32>(clamp(reactive, 0.0, 1.0), 0.0, 0.0, 0.0));
    let composition = diff(textureLoad(snap_opaque, p, 0).rgb, textureLoad(snap_blend, p, 0).rgb) * u.size.w;
    textureStore(out_composition, p, vec4<f32>(clamp(composition, 0.0, 1.0), 0.0, 0.0, 0.0));
}
