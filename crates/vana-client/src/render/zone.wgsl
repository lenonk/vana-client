struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz: direction towards the light, w: ambient level
    light: vec4<f32>,
    fog_color: vec4<f32>,
    // x: fog start, y: fog end
    fog: vec4<f32>,
    eye: vec4<f32>,
    inv_view_proj: mat4x4<f32>,
    // Zone lighting (dat/weather.rs); sun.w = 1 when enabled, xyz = direction the light travels.
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    actor_sun: vec4<f32>,
    actor_color: vec4<f32>,
    actor_ambient: vec4<f32>,
    sky_info: vec4<f32>,
    clear: vec4<f32>,
    sky_colors: array<vec4<f32>, 8>,
    sky_stops: array<vec4<f32>, 2>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
// Point lights (two vec4s each: position, range; colour rgb / 128 (0 when unlit), intensity I), from the effects
// (classes2.md 3.2; D3D8 POINT, Att2 = 1 / I, or 1 when I <= 0).
@group(0) @binding(1) var<storage, read> plights: array<vec4<f32>>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) m0: vec4<f32>,
    @location(5) m1: vec4<f32>,
    @location(6) m2: vec4<f32>,
    @location(7) m3: vec4<f32>,
    // The placement's four lights (table slot + 1, 0 = none).
    @location(8) lights: vec4<u32>,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) world: vec3<f32>,
    @location(3) normal: vec3<f32>,
    @location(4) depth: f32,
    // Target flash (FUN_100c94a0): ambient lerp toward 220 / 255, fog colour scale.
    @location(5) flash: vec2<f32>,
    // Point lights, per vertex (D3D8 POINT: C x I x max(N.L, 0) / d^2, none past the range).
    @location(6) point: vec3<f32>,
};

// FUN_100c94a0: of the floor's lights, the strongest w = 1 / max(d^2 Att2, 1e-4) at the actor's position within
// its range (an unlit one competes too), as a directional light from the lamp: C x w x max(N.L, 0).
fn actor_lamp(packed: u32, pos: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var best = 0.0;
    var col = vec3<f32>(0.0);
    var dir = vec3<f32>(0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        let s = (packed >> (8u * k)) & 0xffu;
        if (s == 0u) { continue; }
        let a = plights[2u * (s - 1u)];
        let c = plights[2u * (s - 1u) + 1u];
        let l = a.xyz - pos;
        let d2 = dot(l, l);
        if (sqrt(d2) > a.w) { continue; }
        let att2 = select(1.0, 1.0 / c.w, c.w > 0.0);
        let wt = 1.0 / max(d2 * att2, 1e-4);
        if (wt > best) {
            best = wt;
            col = c.rgb * wt;
            dir = l * inverseSqrt(max(d2, 1e-12));
        }
    }
    return col * max(dot(n, dir), 0.0);
}

fn point_lights(lights: vec4<u32>, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        let s = lights[k];
        if (s == 0u) { continue; }
        let a = plights[2u * (s - 1u)];
        let c = plights[2u * (s - 1u) + 1u];
        let l = a.xyz - p;
        let d2 = max(dot(l, l), 1e-6);
        if (d2 > a.w * a.w) { continue; }
        sum = sum + c.rgb * c.w * max(dot(n, l * inverseSqrt(d2)), 0.0) / d2;
    }
    return sum;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let world = model * vec4<f32>(v.pos, 1.0);
    var out: VertexOut;
    out.clip = u.view_proj * world;
    out.color = v.color;
    out.uv = v.uv;
    out.world = world.xyz;
    out.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    out.depth = out.clip.w;
    out.flash = vec2<f32>(0.0, 1.0);
    out.point = point_lights(v.lights, world.xyz, out.normal);
    return out;
}

// Skinned actors (ZoneRenderer::pose_actor): model matrix, (alpha, flash ambient, flash fog, 0), then (quat,
// translation) per bone.
@group(2) @binding(0) var<storage, read> pose: array<vec4<f32>>;

struct SkinIn {
    @location(0) p0: vec3<f32>,
    @location(1) n0: vec3<f32>,
    @location(2) p1: vec3<f32>,
    @location(3) n1: vec3<f32>,
    @location(4) bones: vec2<u32>,
    @location(5) w: vec2<f32>,
    @location(6) uv: vec2<f32>,
};

fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// As character::skin: p = sum(r * pos + t * w), n = normalize_or_zero(sum(r * normal)); pos and normal come pre-weighted.
@vertex
fn vs_skinned(v: SkinIn) -> VertexOut {
    let a = 5u + 2u * v.bones.x;
    let b = 5u + 2u * v.bones.y;
    let p = qrot(pose[a], v.p0) + pose[a + 1u].xyz * v.w.x + qrot(pose[b], v.p1) + pose[b + 1u].xyz * v.w.y;
    let n = qrot(pose[a], v.n0) + qrot(pose[b], v.n1);
    let len = length(n);
    let nn = select(vec3<f32>(0.0), n / len, len > 1e-12);
    let model = mat4x4<f32>(pose[0], pose[1], pose[2], pose[3]);
    let world = model * vec4<f32>(p, 1.0);
    var out: VertexOut;
    out.clip = u.view_proj * world;
    // The CPU path's Unorm8 grey 128; alpha 0x80 = opaque.
    out.color = vec4<f32>(vec3<f32>(128.0 / 255.0), round(pose[4].x * 128.0) / 255.0);
    out.uv = v.uv;
    out.world = world.xyz;
    out.normal = normalize((model * vec4<f32>(nn, 0.0)).xyz);
    out.depth = out.clip.w;
    out.flash = pose[4].yz;
    out.point = actor_lamp(bitcast<u32>(pose[4].w), model[3].xyz, out.normal);
    return out;
}

// FFXI vertex colours are half intensity (0x80 = 1.0).
fn shade(in: VertexOut, color: vec4<f32>) -> vec4<f32> {
    let dist = distance(in.world, u.eye.xyz);
    let fog = clamp((dist - u.fog.x) / (u.fog.y - u.fog.x), 0.0, 1.0);
    let rgb = mix(color.rgb, u.fog_color.rgb, fog);
    // fog.z = 1 when the target is not sRGB and gamma has to be applied here.
    return vec4<f32>(select(rgb, pow(rgb, vec3<f32>(1.0 / 2.2)), u.fog.z > 0.5), 1.0);
}

// Most scenery is opaque; its texture alpha carries something other than coverage.
@fragment
fn fs_opaque(in: VertexOut) -> @location(0) vec4<f32> {
    if (u.sun.w > 0.5) {
        return lit_zone(in, textureSample(tex, samp, in.uv));
    }
    return shade(in, textureSample(tex, samp, in.uv) * in.color * 2.0);
}

// Characters carry no baked lighting: shade by the normal. Placeholder until the D3D8 light setup is known.
@fragment
fn fs_actor(in: VertexOut) -> @location(0) vec4<f32> {
    let texel = textureSample(tex, samp, in.uv);
    if (texel.a < 0.3) {
        discard;
    }
    // Vertex alpha carries the model colour alpha (0x80 = 1.0).
    let alpha = clamp(in.color.a * 2.0, 0.0, 1.0);
    if (u.sun.w > 0.5) {
        return vec4<f32>(lit_actor(in, texel).rgb, alpha);
    }
    let diffuse = max(dot(normalize(in.normal), normalize(-u.light.xyz)), 0.0);
    let lit = texel.rgb * (u.light.w + (1.0 - u.light.w) * diffuse);
    return vec4<f32>(shade(in, vec4<f32>(lit, 1.0)).rgb, alpha);
}

@fragment
fn fs_alpha_test(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(tex, samp, in.uv) * in.color * 2.0;
    if (color.a < 0.3) {
        discard;
    }
    if (u.sun.w > 0.5) {
        return lit_zone(in, textureSample(tex, samp, in.uv));
    }
    return shade(in, color);
}

// ---- D3D8 fixed-function zone lighting (dat/weather.rs; FUN_10174610, FUN_10008d60 defaults) ----
// Retail: COLORVERTEX on, DIFFUSE/AMBIENT material source = vertex colour, stage 0 COLOROP MODULATE2X,
// ALPHAOP MODULATE4X, table fog LINEAR on view depth. Maths in gamma space as on D3D8.

fn to_gamma(c: vec3<f32>) -> vec3<f32> {
    return pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.2));
}

fn d3d_out(in: VertexOut, rgb: vec3<f32>) -> vec4<f32> {
    return d3d_out_fog(in, rgb, u.fog_color.rgb);
}

fn d3d_out_fog(in: VertexOut, rgb: vec3<f32>, fog_color: vec3<f32>) -> vec4<f32> {
    let f = clamp((u.fog.y - in.depth) / (u.fog.y - u.fog.x), 0.0, 1.0);
    let c = mix(fog_color, rgb, f);
    return vec4<f32>(select(pow(c, vec3<f32>(2.2)), c, u.fog.z > 0.5), 1.0);
}

// lit = clamp(vc * (ambient + sun * max(N.-L, 0))); out = tex * lit * 2
fn lit_zone(in: VertexOut, t: vec4<f32>) -> vec4<f32> {
    let ndl = max(dot(normalize(in.normal), -u.sun.xyz), 0.0);
    let lit = clamp(in.color.rgb * (u.ambient.rgb + u.sun_color.rgb * ndl + in.point), vec3<f32>(0.0), vec3<f32>(1.0));
    return d3d_out(in, clamp(to_gamma(t.rgb) * lit * 2.0, vec3<f32>(0.0), vec3<f32>(1.0)));
}

// Actors: no vertex colour; material taken as 0x80 grey (inferred).
fn lit_actor(in: VertexOut, t: vec4<f32>) -> vec4<f32> {
    let ndl = max(dot(normalize(in.normal), -u.actor_sun.xyz), 0.0);
    // The target flash pulls the ambient toward 220 / 255 and scales the fog colour (FUN_100c94a0).
    let amb = mix(u.actor_ambient.rgb, vec3<f32>(220.0 / 255.0), in.flash.x);
    let lit = clamp(0.5 * (amb + u.actor_color.rgb * ndl + in.point), vec3<f32>(0.0), vec3<f32>(1.0));
    return d3d_out_fog(in, clamp(to_gamma(t.rgb) * lit * 2.0, vec3<f32>(0.0), vec3<f32>(1.0)), u.fog_color.rgb * in.flash.y);
}

struct SkyOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_sky(@builtin(vertex_index) i: u32) -> SkyOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - 1.0;
    var out: SkyOut;
    out.clip = vec4<f32>(p, 0.0, 1.0);
    out.ndc = p;
    return out;
}

fn sky_stop(k: u32) -> f32 {
    return u.sky_stops[k / 4u][k % 4u];
}

// Sky dome (FUN_1003cd20 / FUN_1003c800): 8 rings, elevation = stop * 90 degrees, linear between rings.
@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4<f32> {
    let far = u.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let ray = normalize(far.xyz / far.w - u.eye.xyz);
    let e = asin(clamp(-ray.y, -1.0, 1.0)) / 1.5707963;
    var c = u.clear.rgb;
    if (u.sky_info.x > 0.5) {
        c = u.sky_colors[0].rgb;
        for (var k = 0u; k < 7u; k = k + 1u) {
            let a = sky_stop(k);
            let b = sky_stop(k + 1u);
            if (e >= a && b > a) {
                c = mix(u.sky_colors[k].rgb, u.sky_colors[k + 1u].rgb, clamp((e - a) / (b - a), 0.0, 1.0));
            }
        }
    }
    return vec4<f32>(select(pow(c, vec3<f32>(2.2)), c, u.fog.z > 0.5), 1.0);
}
