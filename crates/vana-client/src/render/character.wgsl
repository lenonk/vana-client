struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz: direction towards the light, w: ambient level
    light: vec4<f32>,
    // x: 1 when the target is not sRGB
    misc: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = u.view_proj * vec4<f32>(v.pos, 1.0);
    out.uv = v.uv;
    out.normal = v.normal;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(tex, samp, in.uv);
    if (color.a < 0.5) {
        discard;
    }
    let n = normalize(in.normal);
    let diffuse = select(1.0, abs(dot(n, normalize(u.light.xyz))), length(in.normal) > 0.01);
    let rgb = color.rgb * (u.light.w + (1.0 - u.light.w) * diffuse);
    return vec4<f32>(select(rgb, pow(rgb, vec3<f32>(1.0 / 2.2)), u.misc.x > 0.5), 1.0);
}
