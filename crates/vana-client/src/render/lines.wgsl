// Layout mode's overlay (FUN_101cf110): world-space lines in their vertex colour, no depth test.
struct Uniforms {
    view_proj: mat4x4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_line(@location(0) pos: vec3<f32>, @location(1) color: vec4<f32>) -> Out {
    var o: Out;
    o.pos = u.view_proj * vec4<f32>(pos, 1.0);
    o.color = color;
    return o;
}

@fragment
fn fs_line(i: Out) -> @location(0) vec4<f32> {
    return i.color;
}
