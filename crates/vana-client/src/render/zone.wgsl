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
    // x: the area slot of 'ev10' while a sub-area is shown (0 none).
    area_info: vec4<f32>,
};

// An area environment's lighting (sun.w = 1 when it has one); fog.xy = start, end.
struct Area {
    sun: vec4<f32>,
    sun_color: vec4<f32>,
    ambient: vec4<f32>,
    fog_color: vec4<f32>,
    fog: vec4<f32>,
    actor_sun: vec4<f32>,
    actor_color: vec4<f32>,
    actor_ambient: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
// Point lights (two vec4s each: position, range; colour rgb / 128 (0 when unlit), intensity I), from the effects
// (classes2.md 3.2; D3D8 POINT, Att2 = 1 / I, or 1 when I <= 0).
@group(0) @binding(1) var<storage, read> plights: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> areas: array<Area>;

// A door part (scheduler ops 0x0d / 0x0c): rotation and translation from, to, (start tick, ticks); u.area_info.y is now.
struct Part {
    rot_from: vec4<f32>,
    rot_to: vec4<f32>,
    rot_time: vec4<f32>,
    pos_from: vec4<f32>,
    pos_to: vec4<f32>,
    pos_time: vec4<f32>,
};
@group(0) @binding(3) var<storage, read> parts: array<Part>;

fn part_k(time: vec4<f32>) -> f32 {
    return select(clamp((u.area_info.y - time.x) / time.y, 0.0, 1.0), 1.0, time.y <= 0.0);
}

// FUN_100acef0: the part's world matrix = base * T(t) * Rx(a.x) * Ry(a.y) * Rz(a.z) (column vectors).
fn part_matrix(slot: u32) -> mat4x4<f32> {
    let p = parts[slot];
    let a = mix(p.rot_from.xyz, p.rot_to.xyz, part_k(p.rot_time));
    let t = mix(p.pos_from.xyz, p.pos_to.xyz, part_k(p.pos_time));
    let cx = cos(a.x); let sx = sin(a.x);
    let cy = cos(a.y); let sy = sin(a.y);
    let cz = cos(a.z); let sz = sin(a.z);
    let rx = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, cx, sx, 0.0), vec4<f32>(0.0, -sx, cx, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let ry = mat4x4<f32>(vec4<f32>(cy, 0.0, -sy, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(sy, 0.0, cy, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let rz = mat4x4<f32>(vec4<f32>(cz, sz, 0.0, 0.0), vec4<f32>(-sz, cz, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
    let tr = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(t, 1.0));
    return tr * rx * ry * rz;
}
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
    // Bit 0: the model's 'ev10' flag (MMB +0x10 bit 28).
    @location(9) flags: u32,
    // x: the placement's area slot (0 = the zone's).
    @location(10) area: vec4<u32>,
    // Level of detail: drawn while x < d^2 <= y from the eye to the placement's origin; y 0 = always.
    @location(11) band: vec4<f32>,
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
    // Actors: the floor lamp, per vertex. Scenery: 0 (its point lights are per pixel, from `lights`).
    @location(6) point: vec3<f32>,
    // The placement's four light slots (+1, 0 = none) for the per-pixel point lights.
    @location(7) @interpolate(flat) lights: vec4<u32>,
    // The area slot it is lit by (0 = the zone's); bit 15: the 'ev10' path, which keeps the zone's fog range.
    @location(8) @interpolate(flat) area: u32,
    // Actors: the model colour's RGB (actor +0x660, 0x80 = 1.0), the material colour of the ambient and diffuse terms.
    @location(9) @interpolate(flat) tint: vec3<f32>,
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

// Retail's D3D8 POINT lights (C x I x max(N.L, 0) / d^2, cut at the range) ran per vertex, which draws triangles
// of light on coarse walls. Ours, per pixel: the same term with the distance floored at 1 and a smooth window over
// the range instead of the hard cut (not retail).
fn point_lights(lights: vec4<u32>, p: vec3<f32>, n: vec3<f32>) -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var k = 0u; k < 4u; k = k + 1u) {
        let s = lights[k];
        if (s == 0u) { continue; }
        let a = plights[2u * (s - 1u)];
        let c = plights[2u * (s - 1u) + 1u];
        let l = a.xyz - p;
        let d2 = dot(l, l);
        let r2 = a.w * a.w;
        if (d2 >= r2) { continue; }
        let q = d2 / r2;
        let win = (1.0 - q * q) * (1.0 - q * q);
        sum = sum + c.rgb * c.w * max(dot(n, l * inverseSqrt(max(d2, 1e-6))), 0.0) * win / max(d2, 1.0);
    }
    return sum;
}

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    var model = mat4x4<f32>(v.m0, v.m1, v.m2, v.m3);
    let part = u32(v.band.z);
    if (part != 0u) {
        model = model * part_matrix(part);
    }
    let world = model * vec4<f32>(v.pos, 1.0);
    var out: VertexOut;
    out.clip = u.view_proj * world;
    out.color = v.color;
    out.uv = v.uv;
    out.world = world.xyz;
    out.normal = normalize((model * vec4<f32>(v.normal, 0.0)).xyz);
    out.depth = out.clip.w;
    out.flash = vec2<f32>(0.0, 1.0);
    out.tint = vec3<f32>(1.0);
    out.lights = v.lights;
    let ev10 = u32(u.area_info.x);
    out.area = select(v.area.x, ev10 | 0x8000u, (v.flags & 1u) != 0u && ev10 != 0u);
    // FUN_1017c9c0's LOD pick: an instance outside its band collapses to nothing.
    let d = u.eye.xyz - v.m3.xyz;
    let d2 = dot(d, d);
    if (v.band.y > 0.0 && !(d2 > v.band.x && d2 <= v.band.y)) {
        out.clip = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    }
    return out;
}

// Skinned actors (ZoneRenderer::pose_actor): model matrix, (alpha, flash ambient, flash fog, lamp lights), (area slot,
// model colour r, g, b), then (quat, translation) per bone.
@group(2) @binding(0) var<storage, read> pose: array<vec4<f32>>;

struct SkinIn {
    @location(0) p0: vec3<f32>,
    @location(1) n0: vec3<f32>,
    @location(2) p1: vec3<f32>,
    @location(3) n1: vec3<f32>,
    @location(4) bones: vec2<u32>,
    @location(5) w: vec2<f32>,
    @location(6) uv: vec2<f32>,
    @location(7) mat: vec2<f32>,
};

fn qrot(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// As character::skin: p = sum(r * pos + t * w), n = normalize_or_zero(sum(r * normal)); pos and normal come pre-weighted.
@vertex
fn vs_skinned(v: SkinIn) -> VertexOut {
    return skin(v);
}

fn skin(v: SkinIn) -> VertexOut {
    let a = 6u + 2u * v.bones.x;
    let b = 6u + 2u * v.bones.y;
    let p = qrot(pose[a], v.p0) + pose[a + 1u].xyz * v.w.x + qrot(pose[b], v.p1) + pose[b + 1u].xyz * v.w.y;
    let n = qrot(pose[a], v.n0) + qrot(pose[b], v.n1);
    let len = length(n);
    let nn = select(vec3<f32>(0.0), n / len, len > 1e-12);
    let model = mat4x4<f32>(pose[0], pose[1], pose[2], pose[3]);
    let world = model * vec4<f32>(p, 1.0);
    var out: VertexOut;
    out.clip = u.view_proj * world;
    // As the CPU path's Unorm8 colour: red the material light factor (0x80 = 1.0), green its cloth flag; alpha
    // 0x80 = opaque.
    out.color = vec4<f32>(round(v.mat.x * 128.0) / 255.0, v.mat.y, 128.0 / 255.0, round(pose[4].x * 128.0) / 255.0);
    out.uv = v.uv;
    out.world = world.xyz;
    out.normal = normalize((model * vec4<f32>(nn, 0.0)).xyz);
    out.depth = out.clip.w;
    out.flash = pose[4].yz;
    out.point = actor_lamp(bitcast<u32>(pose[4].w), model[3].xyz, out.normal);
    out.area = u32(pose[5].x);
    out.tint = pose[5].yzw;
    return out;
}

// The actor shine overlay (scheduler ops 0x40..0x47): U / V scroll, TFACTOR (RGBA 0..1).
struct ShineU {
    uv: vec4<f32>,
    tf: vec4<f32>,
};
@group(3) @binding(0) var<uniform> shine: ShineU;

// FUN_1002b6e0's second pass: texture coordinates from the camera-space normal through the inverse view rotation
// (the world normal), x 0.5 + 0.5, plus the scroll.
@vertex
fn vs_skinned_shine(v: SkinIn) -> VertexOut {
    var out = skin(v);
    out.uv = vec2<f32>(0.5 * out.normal.x + 0.5 + shine.uv.x, 0.5 * out.normal.y + 0.5 + shine.uv.y);
    return out;
}

// Stages MODULATE2X(TEXTURE, DIFFUSE white), MODULATE2X(CURRENT, TFACTOR), added SRCALPHA / ONE. Texture alpha comes
// doubled from the upload. Fog fades it out *(inf: the base pass's fog state)*.
@fragment
fn fs_actor_shine(in: VertexOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let rgb = clamp(clamp(2.0 * t.rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 2.0 * shine.tf.rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let a = clamp(clamp(t.a, 0.0, 1.0) * 2.0 * shine.tf.a, 0.0, 1.0);
    let dist = distance(in.world, u.eye.xyz);
    let fog = clamp((dist - u.fog.x) / (u.fog.y - u.fog.x), 0.0, 1.0);
    // D3D8 fogs the pass's colour toward the fog colour before the add (the base pass's fog state stays on).
    let o = mix(rgb, u.fog_color.rgb, fog);
    return vec4<f32>(select(o, pow(o, vec3<f32>(1.0 / 2.2)), u.fog.z > 0.5), a);
}

// The actor refraction pass (scheduler ops 0x48 / 0x49): a = (L, r, k, 0), p = (P00, P11, 0, 0).
struct RefrU {
    a: vec4<f32>,
    p: vec4<f32>,
};
@group(3) @binding(0) var<uniform> refr: RefrU;

// D3DTS_TEXTURE0 = PROJ x T(0.5, -0.5 scale, 0.5 offset) + (L P11, L P22) on _41 / _42, CAMERASPACEPOSITION, COUNT4 |
// PROJECTED: the pixel's screen uv shifted by (L P11, L P22) / w (right and down).
@vertex
fn vs_skinned_refract(v: SkinIn) -> VertexOut {
    var out = skin(v);
    let c = out.clip;
    out.uv = vec2<f32>(0.5 * c.x + 0.5 * c.w + refr.a.x * refr.p.x, -0.5 * c.y + 0.5 * c.w + refr.a.x * refr.p.y);
    out.depth = c.w;
    return out;
}

// Stage 0 colour = the copy, alpha MODULATE2X(copy, TFACTOR r); stage 1 colour MODULATE2X(CURRENT, 0x80), alpha
// MODULATE4X(CURRENT, r); times the draw alpha k (retail composites a translucent actor at k); fogged as the base pass.
@fragment
fn fs_actor_refract(in: VertexOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, in.uv / in.depth);
    let enc = u.fog.z > 0.5;
    let lin = select(c.rgb, pow(c.rgb, vec3<f32>(2.2)), enc);
    let rgb = min(lin * 2.0 * (128.0 / 255.0), vec3<f32>(1.0));
    let a0 = min(2.0 * c.a * refr.a.y, 1.0);
    let a = min(4.0 * a0 * refr.a.y, 1.0) * refr.a.z;
    let dist = distance(in.world, u.eye.xyz);
    let fog = clamp((dist - u.fog.x) / (u.fog.y - u.fog.x), 0.0, 1.0);
    let o = mix(rgb, u.fog_color.rgb, fog);
    return vec4<f32>(select(o, pow(o, vec3<f32>(1.0 / 2.2)), enc), a);
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
    let lit = texel.rgb * (u.light.w + (1.0 - u.light.w) * diffuse) * in.tint;
    return vec4<f32>(shade(in, vec4<f32>(lit, 1.0)).rgb, alpha);
}

// Placements named '_': D3DRS_ALPHAFUNC GREATER, ALPHAREF 0x60 on alpha = tex.a x vertex.a x 4 (MODULATE4X; ours with
// the texture alpha already doubled).
@fragment
fn fs_alpha_test(in: VertexOut) -> @location(0) vec4<f32> {
    let color = textureSample(tex, samp, in.uv) * in.color * 2.0;
    if (color.a <= 96.0 / 255.0) {
        discard;
    }
    if (u.sun.w > 0.5) {
        return lit_zone(in, textureSample(tex, samp, in.uv));
    }
    return shade(in, color);
}

// MMB models with bit 31: blended by that alpha (SRCALPHA / INVSRCALPHA), alpha-tested only on '_' placements.
fn blended(in: VertexOut) -> vec4<f32> {
    let t = textureSample(tex, samp, in.uv);
    let a = clamp(t.a * in.color.a * 2.0, 0.0, 1.0);
    if (u.sun.w > 0.5) {
        return vec4<f32>(lit_zone(in, t).rgb, a);
    }
    return vec4<f32>(shade(in, t * in.color * 2.0).rgb, a);
}

@fragment
fn fs_blend(in: VertexOut) -> @location(0) vec4<f32> {
    return blended(in);
}

@fragment
fn fs_blend_test(in: VertexOut) -> @location(0) vec4<f32> {
    let c = blended(in);
    if (c.a <= 96.0 / 255.0) {
        discard;
    }
    return c;
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

// lit = clamp(vc * (ambient + sun * max(N.-L, 0))); out = tex * lit * 2. The light, ambient and fog are the zone's,
// or the area's (FUN_10183da0: FUN_10188ca0 lights, FUN_10188e00 ambient, FUN_10189120 fog).
fn lit_zone(in: VertexOut, t: vec4<f32>) -> vec4<f32> {
    var sun = u.sun.xyz;
    var sun_color = u.sun_color.rgb;
    var ambient = u.ambient.rgb;
    var fog_color = u.fog_color.rgb;
    var fog = u.fog.xy;
    let slot = in.area & 0x7fffu;
    if (slot != 0u && areas[slot].sun.w > 0.5) {
        let a = areas[slot];
        sun = a.sun.xyz;
        sun_color = a.sun_color.rgb;
        ambient = a.ambient.rgb;
        fog_color = a.fog_color.rgb;
        fog = select(a.fog.xy, u.fog.xy, (in.area & 0x8000u) != 0u);
    }
    let n = normalize(in.normal);
    let ndl = max(dot(n, -sun), 0.0);
    let lit = clamp(in.color.rgb * (ambient + sun_color * ndl + point_lights(in.lights, in.world, n)), vec3<f32>(0.0), vec3<f32>(1.0));
    let rgb = clamp(to_gamma(t.rgb) * lit * 2.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let f = clamp((fog.y - in.depth) / (fog.y - fog.x), 0.0, 1.0);
    let c = mix(fog_color, rgb, f);
    return vec4<f32>(select(pow(c, vec3<f32>(2.2)), c, u.fog.z > 0.5), 1.0);
}

// Actors: no vertex colour; material diffuse 0.5, ambient 0x80 / 128 = 1 (DAT_10346380, FUN_1002a8c0). Lit by the
// zone, or by the area they stand in (the actor update FUN_1018b9a0 and draw FUN_1018b9e0 / FUN_1018ba20 with vtable
// +0x188's area).
fn lit_actor(in: VertexOut, t: vec4<f32>) -> vec4<f32> {
    var sun = u.actor_sun.xyz;
    var color = u.actor_color.rgb;
    var ambient = u.actor_ambient.rgb;
    var fog_color = u.fog_color.rgb;
    var fog = u.fog.xy;
    if (in.area != 0u && areas[in.area].sun.w > 0.5) {
        let a = areas[in.area];
        sun = a.actor_sun.xyz;
        color = a.actor_color.rgb;
        ambient = a.actor_ambient.rgb;
        fog_color = a.fog_color.rgb;
        fog = a.fog.xy;
    }
    let ndl = max(dot(normalize(in.normal), -sun), 0.0);
    // D3DRS_AMBIENT = ftol(ambient x 128) (FUN_10181de0); the target flash mixes those bytes toward 220 and scales
    // the fog colour (FUN_100c94a0).
    let amb = floor(mix(floor(ambient * 128.0), vec3<f32>(220.0), in.flash.x)) / 255.0;
    // The material's light factor f (red, 0x80 = 1.0): body meshes scale the ambient bytes (clamped) and the lights by it
    // (FUN_1002cdf0), cloth ones the lights and add emissive max(f - 1.5, 0) (FUN_100216a0).
    let mf = in.color.r * 255.0 / 128.0;
    let cloth = in.color.g > 0.5;
    let ga = select(min(floor(amb * 255.0 * mf) / 255.0, vec3<f32>(1.0)), amb, cloth);
    let em = select(0.0, max(mf - 1.5, 0.0), cloth);
    let lit = clamp((ga + 0.5 * mf * (color * ndl + in.point)) * in.tint + em, vec3<f32>(0.0), vec3<f32>(1.0));
    let rgb = clamp(to_gamma(t.rgb) * lit * 2.0, vec3<f32>(0.0), vec3<f32>(1.0));
    let f = clamp((fog.y - in.depth) / (fog.y - fog.x), 0.0, 1.0);
    let c = mix(fog_color * in.flash.y, rgb, f);
    return vec4<f32>(select(pow(c, vec3<f32>(2.2)), c, u.fog.z > 0.5), 1.0);
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
