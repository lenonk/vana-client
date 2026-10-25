// Effect elements on the GPU (fx/elem.rs has the layout; constants are prepended from it). The compute pass is
// the element update stream FUN_100499d0 plus the transforms of FUN_10043f00 / FUN_100440d0 / FUN_10044f00
// (generators.md C.3, C.4, C.7); the render pass draws each element's mesh with its TFACTOR colour.

struct Params {
    // dt (ticks), seconds, day fraction, element count
    time: vec4<f32>,
    // DAT_10469678 +0xb50, +0xb90, +0xd10, +0xd50, +0xc10, and inverse(+0xb90) (FUN_100691d0)
    view: mat4x4<f32>,
    view_rot: mat4x4<f32>,
    d10: mat4x4<f32>,
    d50: mat4x4<f32>,
    c10: mat4x4<f32>,
    inv_view_rot: mat4x4<f32>,
    eye: vec4<f32>,
    // moon phase, weekday
    extra: vec4<f32>,
    // the zone's area light (DAT_106175ac, area_light.md)
    area: vec4<f32>,
};

@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> el: array<u32>;
@group(0) @binding(2) var<storage, read> ops: array<u32>;
@group(0) @binding(3) var<storage, read> keys: array<vec2<f32>>;
@group(0) @binding(4) var<storage, read> key_desc: array<vec2<u32>>;
@group(0) @binding(5) var<storage, read> slots: array<u32>;
@group(0) @binding(6) var<storage, read_write> outs: array<f32>;
@group(0) @binding(7) var<storage, read_write> back: array<u32>;
@group(0) @binding(8) var<storage, read> paths: array<vec4<f32>>;
// The zone's point-light table (render/mod.rs LIGHTS x two vec4s).
@group(0) @binding(9) var<storage, read_write> lights: array<vec4<f32>>;

// FUN_10062c80 (fx/spline.rs eval): path `at` (first vec4 + 1) at u.
fn path_at(at: u32, u: f32) -> vec3<f32> {
    let h = at - 1u;
    let n = bitcast<u32>(paths[h].x);
    var acc = 0.0;
    var j = n - 1u;
    var t = 1.0;
    for (var k = 0u; k < n; k = k + 1u) {
        let len = paths[h + 1u + 4u * k + 1u].w;
        acc = acc + len;
        if (u <= acc) { j = k; t = (u - (acc - len)) / len; break; }
    }
    let q = h + 1u + 4u * j;
    let t2 = t * t;
    let t3 = t2 * t;
    return paths[q].xyz * (2.0 * t3 - 3.0 * t2 + 1.0) + paths[q + 1u].xyz * (t3 - 2.0 * t2 + t) + paths[q + 2u].xyz * (3.0 * t2 - 2.0 * t3) + paths[q + 3u].xyz * (t3 - t2);
}

const PI: f32 = 3.141592;
const IDENT: mat4x4<f32> = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));

// Inverse of an affine matrix (rotation / scale / translation).
fn inverse4(m: mat4x4<f32>) -> mat4x4<f32> {
    let a = m[0].xyz;
    let b = m[1].xyz;
    let c = m[2].xyz;
    let r0 = cross(b, c);
    let r1 = cross(c, a);
    let r2 = cross(a, b);
    let det = dot(a, r0);
    let inv_t = mat3x3<f32>(r0, r1, r2) * (1.0 / det);
    let inv = transpose(inv_t);
    let t = -(inv * m[3].xyz);
    return mat4x4<f32>(vec4<f32>(inv[0], 0.0), vec4<f32>(inv[1], 0.0), vec4<f32>(inv[2], 0.0), vec4<f32>(t, 1.0));
}

fn ef(b: u32, i: u32) -> f32 { return bitcast<f32>(el[b + i]); }
fn set_ef(b: u32, i: u32, v: f32) { el[b + i] = bitcast<u32>(v); }
fn ev3(b: u32, i: u32) -> vec3<f32> { return vec3<f32>(ef(b, i), ef(b, i + 1u), ef(b, i + 2u)); }
fn set_ev3(b: u32, i: u32, v: vec3<f32>) { set_ef(b, i, v.x); set_ef(b, i + 1u, v.y); set_ef(b, i + 2u, v.z); }
fn sl(b: u32, s: u32) -> u32 { return el[b + E_SLOTS + min(s, E_SLOT_COUNT - 1u)]; }
fn slf(b: u32, s: u32) -> f32 { return bitcast<f32>(sl(b, s)); }
fn set_sl(b: u32, s: u32, v: u32) { if (s < E_SLOT_COUNT) { el[b + E_SLOTS + s] = v; } }
fn set_slf(b: u32, s: u32, v: f32) { set_sl(b, s, bitcast<u32>(v)); }
fn slv3(b: u32, s: u32) -> vec3<f32> { return vec3<f32>(slf(b, s), slf(b, s + 1u), slf(b, s + 2u)); }
fn set_slv3(b: u32, s: u32, v: vec3<f32>) { set_slf(b, s, v.x); set_slf(b, s + 1u, v.y); set_slf(b, s + 2u, v.z); }
fn opf(i: u32) -> f32 { return bitcast<f32>(ops[i]); }
fn opv3(i: u32) -> vec3<f32> { return vec3<f32>(opf(i), opf(i + 1u), opf(i + 2u)); }

// Colour byte k of the D3DCOLOR (0 B, 1 G, 2 R, 3 A).
fn cb(b: u32, k: u32) -> u32 { return (el[b + E_COLOR] >> (8u * k)) & 0xffu; }
fn set_cb(b: u32, k: u32, v: u32) {
    let sh = 8u * k;
    el[b + E_COLOR] = (el[b + E_COLOR] & ~(0xffu << sh)) | ((v & 0xffu) << sh);
}
// ftol of key * 255 into a byte (retail truncates and keeps the low byte).
fn to_byte(v: f32) -> u32 { return u32(i32(v * 255.0)) & 0xffu; }

// FUN_10053550 (linear, x clamped) / FUN_100537f0 (eased) over keyframe `id`.
fn key_sample(id: u32, x_in: f32, eased: bool) -> f32 {
    return key_ov(id, x_in, eased, false, 0.0);
}

// FUN_10053520 / FUN_10053740: the same with the first key's value replaced by v0 when `ov`.
fn key_ov(id: u32, x_in: f32, eased: bool, ov: bool, v0: f32) -> f32 {
    if (id == NONE || id >= arrayLength(&key_desc)) { return 0.0; }
    let d = key_desc[id];
    if (d.y == 0u) { return 0.0; }
    if (d.y == 1u) { return select(keys[d.x].y, v0, ov); }
    var x = x_in;
    if (!eased) { x = clamp(x, 0.0, 1.0); }
    var i = 0u;
    loop {
        if (i + 2u >= d.y || x <= keys[d.x + i + 1u].x) { break; }
        i = i + 1u;
    }
    var a = keys[d.x + i];
    if (ov && i == 0u) { a.y = v0; }
    let c = keys[d.x + i + 1u];
    if (c.x == a.x) { return c.y; }
    let f = (x - a.x) / (c.x - a.x);
    if (!eased) { return a.y + (c.y - a.y) * f; }
    if (f < 0.5) {
        let g = 2.0 * f * f;
        return c.y * g + a.y * (1.0 - g);
    }
    let g = 2.0 * (1.0 - f) * (1.0 - f);
    return a.y * g + c.y * (1.0 - g);
}

// The slot's keyframe at x with its first value replaced by S[s+2] (FUN_1004d530 / FUN_10053520).
fn key_t(b: u32, s: u32, x: f32) -> f32 {
    return key_ov(sl(b, s), x, (sl(b, s + 1u) & 0xfu) != 0u, true, slf(b, s + 2u));
}

// The same with the first value forced to 0 (ops 0x0f..0x11 / 0x6b..0x6d push a literal 0).
fn key_0(b: u32, s: u32, x: f32) -> f32 {
    return key_ov(sl(b, s), x, (sl(b, s + 1u) & 0xfu) != 0u, true, 0.0);
}

// No override (FUN_10053550 / FUN_100537f0 called directly).
fn key_n(b: u32, s: u32, x: f32) -> f32 {
    return key_sample(sl(b, s), x, (sl(b, s + 1u) & 0xfu) != 0u);
}

// The slot's keyframe at the day fraction, no override.
fn key_g(b: u32, s: u32) -> f32 {
    return key_n(b, s, p.time.z);
}

// Op 0x3f: the day fraction repeated n = (param >> 5) & 0x1ff times a day when set (case 0x1004c63f).
fn key_rep(b: u32, s: u32) -> f32 {
    let n = (sl(b, s + 1u) >> 5u) & 0x1ffu;
    var x = p.time.z;
    if (n != 0u) { x = fract(x * f32(n)); }
    return key_n(b, s, x);
}

fn fmod2(v: f32) -> f32 { return v - 2.0 * trunc(v / 2.0); }

fn wrap_pi(a: f32) -> f32 {
    if (a > PI) { return a - 2.0 * PI; }
    if (a < -PI) { return a + 2.0 * PI; }
    return a;
}

// FUN_1003d040: fmod(v, 2) * pi, wrapped into [-pi, pi].
fn half_turns(v: f32) -> f32 {
    var a = (v - 2.0 * trunc(v / 2.0)) * PI;
    if (a > PI) { a = a - 2.0 * PI; } else if (a < -PI) { a = a + 2.0 * PI; }
    return a;
}

fn rot_x(a: f32) -> mat3x3<f32> { let c = cos(a); let s = sin(a); return mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, c, s), vec3<f32>(0.0, -s, c)); }
fn rot_y(a: f32) -> mat3x3<f32> { let c = cos(a); let s = sin(a); return mat3x3<f32>(vec3<f32>(c, 0.0, -s), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(s, 0.0, c)); }
fn rot_z(a: f32) -> mat3x3<f32> { let c = cos(a); let s = sin(a); return mat3x3<f32>(vec3<f32>(c, s, 0.0), vec3<f32>(-s, c, 0.0), vec3<f32>(0.0, 0.0, 1.0)); }
// Math vtable +0x58 (fx/math.rs euler): Rz·Ry·Rx in row vectors, i.e. Z first, X last.
fn euler(r: vec3<f32>) -> mat3x3<f32> { return rot_x(r.x) * rot_y(r.y) * rot_z(r.z); }

// Colour += 8.8 fixed deltas per tick (FUN_1003d6c0), channels in R, G, B, A order.
fn color_step(b: u32, d: array<i32, 4>, dt: f32) {
    let order = array<u32, 4>(2u, 1u, 0u, 3u);
    for (var i = 0u; i < 4u; i = i + 1u) {
        let k = order[i];
        let c = i32(cb(b, k)) * 256 + i32(f32(d[i]) * dt);
        set_cb(b, k, u32(clamp(c >> 8u, 0, 255)));
    }
}

fn s16(v: u32, hi: bool) -> i32 {
    var x = v & 0xffffu;
    if (hi) { x = v >> 16u; }
    return (i32(x) << 16u) >> 16u;
}

fn slot_mat(s: u32) -> mat4x4<f32> {
    let o = s * SLOT_WORDS;
    return mat4x4<f32>(
        vec4<f32>(bitcast<f32>(slots[o]), bitcast<f32>(slots[o + 1u]), bitcast<f32>(slots[o + 2u]), bitcast<f32>(slots[o + 3u])),
        vec4<f32>(bitcast<f32>(slots[o + 4u]), bitcast<f32>(slots[o + 5u]), bitcast<f32>(slots[o + 6u]), bitcast<f32>(slots[o + 7u])),
        vec4<f32>(bitcast<f32>(slots[o + 8u]), bitcast<f32>(slots[o + 9u]), bitcast<f32>(slots[o + 10u]), bitcast<f32>(slots[o + 11u])),
        vec4<f32>(bitcast<f32>(slots[o + 12u]), bitcast<f32>(slots[o + 13u]), bitcast<f32>(slots[o + 14u]), bitcast<f32>(slots[o + 15u])));
}
fn slot_v3(s: u32, k: u32) -> vec3<f32> {
    let o = s * SLOT_WORDS + k;
    return vec3<f32>(bitcast<f32>(slots[o]), bitcast<f32>(slots[o + 1u]), bitcast<f32>(slots[o + 2u]));
}
fn slot_f(s: u32, k: u32) -> f32 { return bitcast<f32>(slots[s * SLOT_WORDS + k]); }

// FUN_10043e00: parent x (pos x scale A).
fn world_of(b: u32, parent: u32) -> vec3<f32> {
    if (parent == NONE) { return ev3(b, E_POS); }
    let wp = ev3(b, E_POS) * slot_v3(parent, S_SCALE_A);
    if (slots[parent * SLOT_WORDS + S_HAS_MATRIX] == 0u) { return wp; }
    return (slot_mat(parent) * vec4<f32>(wp, 1.0)).xyz;
}

@compute @workgroup_size(64)
fn update(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u32(p.time.w)) { return; }
    let b = i * E_WORDS;
    let ob = i * OUT_WORDS;
    let bb = i * BACK_WORDS;
    if ((el[b + E_STATE] & ST_ALIVE) == 0u) {
        outs[ob + 23u] = 0.0;
        back[bb + 1u] = 0u;
        return;
    }
    let dt = p.time.x;
    // FUN_10043ca0: life counts down unless the CPU set it this frame.
    if ((el[b + E_CTRL] & CTL_LIFE_SET) == 0u && ef(b, E_MAX_LIFE) != 0.0) {
        set_ef(b, E_LIFE, ef(b, E_LIFE) - dt);
        // Expired (the CPU deletes it this frame): never drawn at t >= 1.
        if (ef(b, E_LIFE) < 1.0) {
            el[b + E_CTRL] = 0u;
            outs[ob + 23u] = 0.0;
            return;
        }
    }
    el[b + E_CTRL] = 0u;
    set_ef(b, E_FADE, 1.0);
    let parent = el[b + E_PARENT];
    let old_pos = ev3(b, E_POS);
    var kill = 0u;
    var ip = el[b + E_STREAM];
    var guard = 0u;
    loop {
        guard = guard + 1u;
        if (guard > 256u || ip >= arrayLength(&ops)) { break; }
        let w0 = ops[ip];
        let code = w0 & 0xffu;
        let len = (w0 >> 8u) & 0x1fu;
        if (code == 0u || len == 0u) { break; }
        let s = (w0 >> 13u) & 0x3fu;
        let r = ip + 1u;
        let life = ef(b, E_LIFE);
        let maxl = ef(b, E_MAX_LIFE);
        let t = ef(b, E_T);
        switch code {
            case 0x01u: { set_ev3(b, E_POS, ev3(b, E_POS) + opv3(r) * dt); }
            case 0x02u: { set_ev3(b, E_POS, ev3(b, E_POS) + slv3(b, s) * dt); }
            // Follow the path: u = t, or key(t) (no override, fmod 1 above 1); pos += p(u) - S[s+3..5].
            case 0x34u: {
                let at = sl(b, s + 7u);
                if (at != 0u) {
                    var u = t;
                    if (sl(b, s) != NONE) {
                        u = key_n(b, s, t);
                        if (u > 1.0) { u = fract(u); }
                    }
                    let pp = path_at(at, u);
                    set_ev3(b, E_POS, ev3(b, E_POS) + pp - slv3(b, s + 3u));
                    set_slv3(b, s + 3u, pp);
                }
            }
            case 0x03u: { if ((el[b + E_STATE] & ST_BATCHED) == 0u) { set_slv3(b, s, slv3(b, s) + opv3(r) * dt); } }
            case 0x04u: { set_ev3(b, E_ROT, ev3(b, E_ROT) + opv3(r) * dt); }
            case 0x05u: { set_ev3(b, E_ROT, ev3(b, E_ROT) + slv3(b, s) * dt); }
            case 0x06u, 0x09u: { set_slv3(b, s, slv3(b, s) + opv3(r) * dt); }
            case 0x07u: { set_ev3(b, E_SCALE, ev3(b, E_SCALE) + opv3(r) * dt); }
            case 0x08u: { set_ev3(b, E_SCALE, ev3(b, E_SCALE) + slv3(b, s) * dt); }
            case 0x0au: {
                color_step(b, array<i32, 4>(s16(ops[r], false), s16(ops[r], true), s16(ops[r + 1u], false), s16(ops[r + 1u], true)), dt);
            }
            case 0x0bu: {
                color_step(b, array<i32, 4>(s16(sl(b, s), false), s16(sl(b, s), true), s16(sl(b, s + 1u), false), s16(sl(b, s + 1u), true)), dt);
            }
            case 0x0cu: {
                let sec = p.time.y;
                for (var k = 0u; k < 4u; k = k + 1u) {
                    let word = s + k / 2u;
                    let hi = (k % 2u) == 1u;
                    let cur = s16(sl(b, word), hi);
                    let d = s16(ops[r + k / 2u], hi);
                    let nv = u32(cur + i32(f32(d) * sec)) & 0xffffu;
                    let sh = select(0u, 16u, hi);
                    set_sl(b, word, (sl(b, word) & ~(0xffffu << sh)) | (nv << sh));
                }
            }
            case 0x0du: {
                if ((el[b + E_STATE] & ST_HAS_RES) != 0u) {
                    var cd = ef(b, E_FRAME_CD) - dt;
                    let period = ef(b, E_FRAME_PERIOD);
                    var frame = el[b + E_FRAME];
                    var n = 0u;
                    loop {
                        if (cd >= 0.0 || period == 0.0 || n > 64u) { break; }
                        cd = cd + period;
                        frame = frame + 1u;
                        n = n + 1u;
                    }
                    set_ef(b, E_FRAME_CD, cd);
                    el[b + E_FRAME] = frame;
                }
            }
            case 0x0eu: { set_ef(b, E_T, select(1.0 - life / maxl, 0.0, maxl == 0.0)); }
            case 0x0fu, 0x10u, 0x11u: { set_ef(b, E_POS + code - 0x0fu, slf(b, s + 2u) + key_0(b, s, t)); }
            case 0x12u, 0x13u, 0x14u: { set_ef(b, E_ROT + code - 0x12u, half_turns(key_t(b, s, t))); }
            case 0x15u, 0x16u, 0x17u: { set_ef(b, E_SCALE + code - 0x15u, key_t(b, s, t)); }
            case 0x18u: { set_cb(b, 2u, to_byte(key_t(b, s, t))); }
            case 0x19u: { set_cb(b, 1u, to_byte(key_t(b, s, t))); }
            case 0x1au: { set_cb(b, 0u, to_byte(key_t(b, s, t))); }
            case 0x1bu: { set_cb(b, 3u, to_byte(key_t(b, s, t))); }
            // 0x1c / 0x1d: U / V = fmod(key, 2); 0x54 / 0x55 accumulate key x dt (no override), fmod 2.
            case 0x1cu: { set_ef(b, E_UV, fmod2(key_t(b, s, t))); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_U; }
            case 0x1du: { set_ef(b, E_UV + 1u, fmod2(key_t(b, s, t))); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_V; }
            case 0x54u: { set_ef(b, E_UV, fmod2(ef(b, E_UV) + key_n(b, s, t) * dt)); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_U; }
            case 0x55u: { set_ef(b, E_UV + 1u, fmod2(ef(b, E_UV + 1u) + key_n(b, s, t) * dt)); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_V; }
            // D3b morph weights +0x194.. (class 0xf only): the slot keyframe at life t, or the day fraction with
            // parameter bit 4, repeated (param >> 5) & 0x1ff times.
            case 0x1eu, 0x1fu, 0x20u, 0x21u, 0x22u: {
                if (el[b + E_KIND] == DK_MORPH) {
                    let prm = sl(b, s + 1u);
                    var x = t;
                    if ((prm & 0x10u) != 0u) { x = p.time.z; }
                    let n = (prm >> 5u) & 0x1ffu;
                    if (n != 0u) { x = fract(x * f32(n)); }
                    set_ef(b, E_MORPH_W + code - 0x1eu, key_t(b, s, x));
                }
            }
            // The moon (FUN_10184dc0): frame = phase, rotation z = pi/2.
            case 0x45u: {
                if ((el[b + E_STATE] & ST_HAS_RES) != 0u) {
                    el[b + E_FRAME] = u32(p.extra.x);
                    set_ef(b, E_ROT + 2u, PI / 2.0);
                }
            }
            // 0x4e: colour bytes (R, G, B, A) from an 8-entry weekday table at op+8; 0x4f: the colour times a 12-entry
            // moon-phase table / 128 (FUN_1003d290). Both lerp from the previous entry in the day's first minute.
            case 0x4eu, 0x4fu: {
                let n = select(8u, 12u, code == 0x4fu);
                let idx = select(u32(p.extra.y), u32(p.extra.x), code == 0x4fu) % n;
                let prev = (idx + n - 1u) % n;
                let minute = p.time.z * 1440.0;
                let lerp = minute < 1.0;
                let cur = ops[r + 1u + idx];
                let old = ops[r + 1u + prev];
                let order = array<u32, 4>(2u, 1u, 0u, 3u);
                for (var q = 0u; q < 4u; q = q + 1u) {
                    let cv = f32((cur >> (8u * q)) & 0xffu);
                    let ov = f32((old >> (8u * q)) & 0xffu);
                    let v = select(cv, ov + (cv - ov) * minute, lerp);
                    let k = order[q];
                    if (code == 0x4eu) {
                        set_cb(b, k, u32(v));
                    } else {
                        set_cb(b, k, u32(max(f32(cb(b, k)) * v / 128.0, 0.0)));
                    }
                }
            }
            case 0x26u: {
                let m = euler(vec3<f32>(opf(r + 2u), opf(r + 1u), opf(r)));
                set_slv3(b, s, m * slv3(b, s));
            }
            case 0x27u: { set_ef(b, E_UV, ef(b, E_UV) + opf(r) * dt); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_U; }
            case 0x28u: { set_ef(b, E_UV + 1u, ef(b, E_UV + 1u) + opf(r) * dt); el[b + E_STATE] = el[b + E_STATE] | ST_SCROLL_V; }
            // FUN_100499d0 case 0x1004a0a2: d == 0 hides; below near kept, to far faded, else 0 (expire on bit 0).
            case 0x2eu: {
                let d = distance(p.eye.xyz, world_of(b, parent));
                let near = opf(r);
                let far = opf(r + 1u);
                if (d == 0.0) {
                    set_ef(b, E_FADE, 0.0);
                } else if (d >= near) {
                    if (d <= far) {
                        set_ef(b, E_FADE, ef(b, E_FADE) * (1.0 - (d - near) / (far - near)));
                    } else {
                        set_ef(b, E_FADE, 0.0);
                        if ((ops[r + 2u] & 1u) != 0u) { kill = 1u; }
                    }
                }
            }
            // Same case, 'H': 0 below rec1, in to rec2, kept to rec3, out to rec4, else 0 (expire on rec5 bit 0).
            case 0x48u: {
                let d = distance(p.eye.xyz, world_of(b, parent));
                if (d == 0.0) {
                    set_ef(b, E_FADE, 0.0);
                } else if (d >= opf(r) && d < opf(r + 1u)) {
                    set_ef(b, E_FADE, ef(b, E_FADE) * (1.0 - (d - opf(r + 1u)) / (opf(r) - opf(r + 1u))));
                } else if (d >= opf(r) && d >= opf(r + 2u) && d < opf(r + 3u)) {
                    set_ef(b, E_FADE, ef(b, E_FADE) * (1.0 - (d - opf(r + 2u)) / (opf(r + 3u) - opf(r + 2u))));
                } else if (d < opf(r) || d >= opf(r + 3u)) {
                    set_ef(b, E_FADE, 0.0);
                    if ((ops[r + 4u] & 1u) != 0u) { kill = 1u; }
                }
            }
            // Drag on the slot vector: S *= rec[1]^dt (__CIpow, d3a_particles.md 4.2).
            case 0x2cu: { set_slv3(b, s, slv3(b, s) * pow(opf(r), dt)); }
            case 0x2fu: { set_slv3(b, s, euler(ev3(b, E_ROT)) * slv3(b, s)); }
            // Point-light fields: 0x5b / 0x5c life keys (first value S[s+2]), 0x5d / 0x5e plain, 0x49 the day.
            case 0x5bu: { if (el[b + E_KIND] == DK_LIGHT) { set_ef(b, E_L_INT, key_t(b, s, t)); } }
            case 0x5cu: { if (el[b + E_KIND] == DK_LIGHT) { set_ef(b, E_L_RANGE, key_t(b, s, t)); } }
            case 0x5du: { if (el[b + E_KIND] == DK_LIGHT) { set_ef(b, E_L_ISCALE, key_n(b, s, t)); } }
            case 0x5eu: { if (el[b + E_KIND] == DK_LIGHT) { set_ef(b, E_L_RSCALE, key_n(b, s, t)); } }
            case 0x49u: { if (el[b + E_KIND] == DK_LIGHT) { set_ef(b, E_L_INT, key_g(b, s)); } }
            // Colour R, G, B = the slot's bytes 2 / 1 / 0 (op 0x91's base) x the zone light; ftol, wrapping.
            case 0x69u: {
                let sc = sl(b, s);
                set_cb(b, 2u, u32(max(f32((sc >> 16u) & 0xffu) * p.area.x, 0.0)) & 0xffu);
                set_cb(b, 1u, u32(max(f32((sc >> 8u) & 0xffu) * p.area.y, 0.0)) & 0xffu);
                set_cb(b, 0u, u32(max(f32(sc & 0xffu) * p.area.z, 0.0)) & 0xffu);
            }
            // vtable +0x34(key(t)): CMoDist +0x194 (0 without a keyframe).
            case 0x24u: { if (el[b + E_KIND] == DK_DIST || el[b + E_KIND] == DK_DIST_MODEL) { set_ef(b, E_DIST_OFF, select(0.0, key_t(b, s, t), sl(b, s) != NONE)); } }
            case 0x30u, 0x31u, 0x32u: {
                let k = el[b + E_OP2_SLOT];
                if (k != NONE) { set_slf(b, k + code - 0x30u, slf(b, s + 2u) + key_t(b, s, t)); }
            }
            case 0x3cu: { set_cb(b, 2u, to_byte(key_g(b, s))); }
            case 0x3du: { set_cb(b, 1u, to_byte(key_g(b, s))); }
            case 0x3eu: { set_cb(b, 0u, to_byte(key_g(b, s))); }
            case 0x3fu: { set_ef(b, E_FADE, ef(b, E_FADE) * key_rep(b, s)); }
            case 0x40u, 0x41u, 0x42u: { set_ef(b, E_SCALE + code - 0x40u, key_g(b, s)); }
            case 0x4au: { set_cb(b, 2u, to_byte(key_g(b, s))); }
            case 0x4bu: { set_cb(b, 1u, to_byte(key_g(b, s))); }
            case 0x4cu: { set_cb(b, 0u, to_byte(key_g(b, s))); }
            case 0x4du: { set_cb(b, 3u, to_byte(key_g(b, s))); }
            case 0x50u: { if (parent != NONE) { set_ef(b, E_FADE, ef(b, E_FADE) * slot_f(parent, S_WEATHER)); } }
            // FUN_10046c00 / FUN_1003d0d0: rot += wrap(fmod(key, 2) pi) dt, wrapped again.
            case 0x56u, 0x57u, 0x58u: { set_ef(b, E_ROT + code - 0x56u, wrap_pi(ef(b, E_ROT + code - 0x56u) + half_turns(key_n(b, s, t)) * dt)); }
            case 0x61u, 0x62u, 0x63u: { set_ef(b, E_ROT + code - 0x61u, wrap_pi(ef(b, E_ROT + code - 0x61u) + half_turns(key_g(b, s)) * dt)); }
            case 0x66u, 0x67u, 0x68u: { set_ef(b, E_ROT + code - 0x66u, half_turns(key_g(b, s))); }
            case 0x6bu, 0x6cu, 0x6du: { set_ef(b, E_POS + code - 0x6bu, slf(b, s + 2u) + key_0(b, s, p.time.z)); }
            default: {}
        }
        ip = ip + len;
    }

    // Transforms (model_draw.md 6). Local L = scale then rotation; orientation modes; world and view position.
    let flags = el[b + E_FLAGS];
    var a = vec3<f32>(1.0);
    var ks = vec3<f32>(1.0);
    var pm = IDENT;
    var has_p = false;
    if (parent != NONE) {
        a = slot_v3(parent, S_SCALE_A);
        if (slots[parent * SLOT_WORDS + S_SIZE_SCALED] != 0u) { ks = slot_v3(parent, S_SCALE_B); }
        if (slots[parent * SLOT_WORDS + S_HAS_MATRIX] != 0u) { pm = slot_mat(parent); has_p = true; }
    }
    let rv = ev3(b, E_ROT);
    var rot = euler(rv);
    if ((flags & 0x200u) != 0u) { rot = rot_z(rv.z) * rot_y(rv.y) * rot_x(rv.x); }
    let sc = ev3(b, E_SCALE) * ks;
    let ssc = mat3x3<f32>(vec3<f32>(sc.x, 0.0, 0.0), vec3<f32>(0.0, sc.y, 0.0), vec3<f32>(0.0, 0.0, sc.z));
    var l3 = rot * ssc;
    if ((flags & 2u) != 0u) { l3 = ssc * rot; }
    if ((el[b + E_STATE] & ST_GROUND) != 0u) {
        l3 = mat3x3<f32>(ev3(b, E_GROUND), ev3(b, E_GROUND + 3u), ev3(b, E_GROUND + 6u)) * l3;
    }
    let pos = ev3(b, E_POS);
    // FUN_10044350: orientation modes 1 / 2 face the motion, 3 faces the camera *(the bit-0 basis variant is not
    // built)*.
    let omode = (flags >> 6u) & 7u;
    if (omode >= 1u && omode <= 3u) {
        var dv = pos - old_pos;
        if (omode == 3u) {
            var eye_l = p.eye.xyz;
            if (has_p) { eye_l = (inverse4(pm) * vec4<f32>(eye_l, 1.0)).xyz / a; }
            dv = eye_l - pos;
        }
        if (dot(dv, dv) > 1e-12) {
            let yaw = -atan2(dv.z, dv.x);
            var o = rot_y(yaw);
            if (omode != 2u) { o = rot_y(yaw) * rot_z(atan2(dv.y, length(dv.xz))); }
            l3 = o * l3;
        }
    }
    var wpos = pos;
    if (has_p) { wpos = (pm * vec4<f32>(pos * a, 1.0)).xyz; }
    let bboard = (flags & 1u) != 0u && (flags & 0x1c0u) == 0u;
    let screen = (flags & 4u) != 0u && (flags & 8u) != 0u;
    var e: vec3<f32>;
    if ((flags & 0x20000u) != 0u) {
        var t = vec3<f32>(0.0);
        if (has_p) { t = pm[3].xyz; }
        e = (p.view * vec4<f32>(t, 1.0)).xyz + vec3<f32>(pos.x, select(pos.y, -pos.y, bboard), pos.z);
    } else if ((flags & 4u) != 0u) {
        if ((flags & 8u) != 0u) { e = pos; } else { e = (p.view_rot * vec4<f32>(pos, 0.0)).xyz; }
    } else {
        e = (p.view * vec4<f32>(wpos, 1.0)).xyz;
    }
    // FUN_10044e10: the base matrix by bits 13/14 (identity with bit 3).
    let sel = (flags >> 13u) & 3u;
    var xm = p.view_rot;
    if ((flags & 8u) != 0u) { xm = IDENT; } else if (sel == 1u) { xm = p.d10; } else if (sel == 2u) { xm = p.d50; } else if (sel == 3u) { xm = p.c10; }
    var mv: mat4x4<f32>;
    if (bboard || screen) {
        var m3: mat3x3<f32>;
        if (!screen) {
            m3 = rot_x(PI) * l3;
        } else {
            m3 = mat3x3<f32>(l3[0], -l3[1], l3[2]);
        }
        var t = e;
        var c0 = m3[0];
        var c1 = m3[1];
        if (screen && abs(t.z) >= 0.2) {
            t.z = -t.z;
            t.y = -t.y;
        } else if (abs(t.z) < 0.2 && t.z != 0.0) {
            // Near clamp, as coded (only _11 / _22 scale).
            let q = 0.2 / t.z;
            t.z = -0.2;
            c0.x = c0.x * q;
            c1.y = c1.y * q;
            t.x = t.x * q;
            t.y = -q * t.y;
        }
        mv = mat4x4<f32>(vec4<f32>(c0, 0.0), vec4<f32>(c1, 0.0), vec4<f32>(m3[2], 0.0), vec4<f32>(t, 1.0));
    } else if ((flags & 4u) != 0u) {
        mv = xm * mat4x4<f32>(vec4<f32>(l3[0], 0.0), vec4<f32>(l3[1], 0.0), vec4<f32>(l3[2], 0.0), vec4<f32>(pos, 1.0));
    } else if ((flags & 0x20000u) != 0u) {
        mv = mat4x4<f32>(vec4<f32>(l3[0], 0.0), vec4<f32>(l3[1], 0.0), vec4<f32>(l3[2], 0.0), vec4<f32>(e, 1.0));
    } else {
        var base = l3;
        if (sel == 0u && has_p) { base = mat3x3<f32>(pm[0].xyz, pm[1].xyz, pm[2].xyz) * l3; }
        let x3 = mat3x3<f32>(xm[0].xyz, xm[1].xyz, xm[2].xyz) * base;
        mv = mat4x4<f32>(vec4<f32>(x3[0], 0.0), vec4<f32>(x3[1], 0.0), vec4<f32>(x3[2], 0.0), vec4<f32>(e, 1.0));
    }
    // Sort key +0x12c (FUN_100440d0).
    let bias = ef(b, E_BIAS);
    var key = bias;
    if ((flags & 0x400000u) == 0u) {
        key = bias - e.z;
        if (screen && abs(bias) > 0.03) { key = (1.0 - bias) - e.z; }
        if ((flags & 0x8000000u) != 0u) { key = key + 400.0; }
    }
    // Projection depth bias (FUN_10045230 step 4).
    var pbias = 0.0;
    if (bias != 0.0 && (flags & 0x8400000u) == 0u) {
        let z = abs(mv[3].z);
        if (z <= 32.0) {
            let tt = 1.0 - z / 32.0;
            pbias = tt * tt * tt * 0.03 * bias;
        }
    }
    let fade = ef(b, E_FADE);
    let drawn = fade >= 0.01;
    let kind = el[b + E_KIND];
    // FUN_10055450 from the common draw: pos = +0x54 raw, range = +0x1a8 x +0x19c, I = max(0, +0x1a0) x +0x1a4 x
    // +0x138; FUN_10171640: colour rgb / 128 (black when I <= 0), attenuation 1 / (d^2 / I).
    if (drawn && kind == DK_LIGHT && el[b + E_L_SLOT] != NONE) {
        let ls = el[b + E_L_SLOT];
        let li = max(max(0.0, ef(b, E_L_INT)) * ef(b, E_L_ISCALE) * fade, 0.0);
        let rgb = vec3<f32>(f32(cb(b, 2u)), f32(cb(b, 1u)), f32(cb(b, 0u))) / 128.0;
        lights[2u * ls] = vec4<f32>(ev3(b, E_POS), ef(b, E_L_RSCALE) * ef(b, E_L_RANGE));
        lights[2u * ls + 1u] = vec4<f32>(select(rgb, vec3<f32>(0.0), li <= 0.0), li);
    }
    // D3a frame wrap at draw time (FUN_1003dad0 / FUN_1003e2f0): a reset, not a modulo.
    if (drawn && (kind == DK_SPRITE || kind == DK_BATCHED) && el[b + E_FRAME] >= el[b + E_FRAMES]) {
        el[b + E_FRAME] = 0u;
    }
    for (var k = 0u; k < 4u; k = k + 1u) {
        outs[ob + 4u * k] = mv[k].x;
        outs[ob + 4u * k + 1u] = mv[k].y;
        outs[ob + 4u * k + 2u] = mv[k].z;
        outs[ob + 4u * k + 3u] = mv[k].w;
    }
    // TFACTOR (FUN_1003d4f0): A x (fade x +0x134) only when that is below 1; modes 0x62 / 0x64 / 0x68 replace A.
    let kf = fade * ef(b, E_ALPHA);
    var alpha = cb(b, 3u);
    if (kf < 1.0) { alpha = u32(max(f32(alpha) * kf, 0.0)) & 0xffu; }
    let mode = el[b + E_BLEND];
    if ((mode & 0xffu) == 0x62u) { alpha = (mode >> 8u) & 0xffu; }
    if ((mode & 0xffu) == 0x64u || (mode & 0xffu) == 0x68u) { alpha = min(2u * ((mode >> 8u) & 0xffu), 255u); }
    outs[ob + 16u] = f32(cb(b, 2u)) / 255.0;
    outs[ob + 17u] = f32(cb(b, 1u)) / 255.0;
    outs[ob + 18u] = f32(cb(b, 0u)) / 255.0;
    outs[ob + 19u] = f32(alpha) / 255.0;
    let st = el[b + E_STATE];
    outs[ob + 20u] = select(0.0, ef(b, E_UV), (st & ST_SCROLL_U) != 0u);
    outs[ob + 21u] = select(0.0, ef(b, E_UV + 1u), (st & ST_SCROLL_V) != 0u);
    outs[ob + 22u] = f32(el[b + E_FRAME]);
    outs[ob + 23u] = select(0.0, 1.0, drawn);
    outs[ob + 24u] = pbias;
    // FUN_1003ea60: morph weights divided by their sum; none -> the largest of last frame's gets 1.0.
    if (kind == DK_MORPH) {
        let kk = min(el[b + E_FRAMES], 5u);
        var sum = 0.0;
        for (var q = 0u; q < kk; q = q + 1u) { sum = sum + ef(b, E_MORPH_W + q); }
        var best = 0u;
        for (var q = 1u; q < kk; q = q + 1u) {
            if (ef(b, E_MORPH_PREV + q) > ef(b, E_MORPH_PREV + best)) { best = q; }
        }
        for (var q = 0u; q < 5u; q = q + 1u) {
            var wq = 0.0;
            if (q < kk) {
                if (sum < 1.17549435e-38) { wq = select(0.0, 1.0, q == best); } else { wq = ef(b, E_MORPH_W + q) / sum; }
            }
            outs[ob + 32u + q] = wq;
            set_ef(b, E_MORPH_PREV + q, ef(b, E_MORPH_W + q));
        }
    }
    // Batched particles (d3a_particles.md 4.4 / 4.5): count, t, placement M1 = parent rotation x inverse(V_b90).
    if (kind == DK_BATCHED) {
        let nn = el[b + E_PART_N];
        var n = nn;
        if (nn >= 20u && ((el[b + E_GEN_BITS] >> 8u) & 1u) != 0u) { n = u32(f32(nn) * 0.5); }
        // 6 vertices a particle but 4n - 2 strip triangles from vertex 1: the first (2n + 1) / 3 show (as coded).
        let shown = select(0u, (2u * n + 1u) / 3u, n > 0u);
        outs[ob + 25u] = f32(shown);
        outs[ob + 26u] = ef(b, E_PART_T);
        outs[ob + 27u] = ef(b, E_DRAG);
        let g = ev3(b, E_GRAVITY);
        outs[ob + 28u] = g.x;
        outs[ob + 29u] = g.y;
        outs[ob + 30u] = g.z;
        var prot = IDENT;
        if (has_p) { prot = mat4x4<f32>(pm[0], pm[1], pm[2], vec4<f32>(0.0, 0.0, 0.0, 1.0)); }
        let m1 = p.inv_view_rot * prot;
        for (var k = 0u; k < 3u; k = k + 1u) {
            outs[ob + 32u + 4u * k] = m1[k].x;
            outs[ob + 33u + 4u * k] = m1[k].y;
            outs[ob + 34u + 4u * k] = m1[k].z;
        }
        if (drawn && n > 0u) { set_ef(b, E_PART_T, ef(b, E_PART_T) + dt); }
    }
    back[bb] = bitcast<u32>(key);
    back[bb + 1u] = kill | select(0u, B_DRAWN, drawn);
    back[bb + 2u] = bitcast<u32>(wpos.x);
    back[bb + 3u] = bitcast<u32>(wpos.y);
    back[bb + 4u] = bitcast<u32>(wpos.z);
    back[bb + 5u] = bitcast<u32>(rv.x);
    back[bb + 6u] = bitcast<u32>(rv.y);
    back[bb + 7u] = bitcast<u32>(rv.z);
    back[bb + 8u] = bitcast<u32>(sc.x);
    back[bb + 9u] = bitcast<u32>(sc.y);
    back[bb + 10u] = bitcast<u32>(sc.z);
    back[bb + 11u] = el[b + E_COLOR];
    back[bb + 12u] = el[b + E_SERIAL];
    let lp = ev3(b, E_POS);
    back[bb + 13u] = bitcast<u32>(lp.x);
    back[bb + 14u] = bitcast<u32>(lp.y);
    back[bb + 15u] = bitcast<u32>(lp.z);
}

// ---- Drawing ----

struct Draw {
    proj: mat4x4<f32>,
    fog_color: vec4<f32>,
    // fog start, end, linearize (1 = sRGB target: the gamma-space result is converted), 0
    fog: vec4<f32>,
    // near, far, scene target width, height
    clip: vec4<f32>,
    // "menu    dmgnum" glyphs 0..10: UV rect, then (width, height, 0, 0) in units
    glyphs: array<vec4<f32>, 22>,
};

@group(0) @binding(0) var<uniform> d: Draw;
@group(0) @binding(1) var<storage, read> douts: array<f32>;
@group(0) @binding(2) var<storage, read> dels: array<u32>;
@group(0) @binding(3) var<storage, read> parts: array<f32>;
@group(0) @binding(4) var<storage, read> sverts: array<u32>;
@group(0) @binding(5) var<storage, read> sframes: array<vec2<u32>>;
@group(0) @binding(6) var<storage, read> mdata: array<u32>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

// Instances are element x 512 + (particle, or the shading variant for meshes).
const PER_ELEM: u32 = 512u;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
    @location(3) uv: vec2<f32>,
};

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) tf: vec4<f32>,
    @location(3) depth: f32,
    // flags, blend mode, shading variant (0 D3m, 1 D3m untextured, 2 MMB, 3 MMB untextured, 4 D3a)
    @location(4) @interpolate(flat) mode: vec3<u32>,
};

fn out_mat(o: u32) -> mat4x4<f32> {
    return mat4x4<f32>(
        vec4<f32>(douts[o], douts[o + 1u], douts[o + 2u], douts[o + 3u]),
        vec4<f32>(douts[o + 4u], douts[o + 5u], douts[o + 6u], douts[o + 7u]),
        vec4<f32>(douts[o + 8u], douts[o + 9u], douts[o + 10u], douts[o + 11u]),
        vec4<f32>(douts[o + 12u], douts[o + 13u], douts[o + 14u], douts[o + 15u]));
}

// View position to clip space with the projection depth bias (near and far pulled by f) and sky meshes past the
// far plane kept at it.
fn to_clip(vp: vec3<f32>, f: f32) -> vec4<f32> {
    var c = d.proj * vec4<f32>(vp, 1.0);
    if (f != 0.0) {
        let n = d.clip.x - f;
        let fr = d.clip.y - f;
        c.z = vp.z * fr / (n - fr) + n * fr / (n - fr);
    }
    // D3DRS_ZBIAS 15 for the effect draw (FUN_10045950), as Wine's d3d8 maps it: -15 x 5e-6 in NDC depth
    // (inferred; D3D8 left the scale to the driver).
    c.z = min(c.z - 7.5e-5 * c.w, c.w * 0.999999);
    return c;
}

fn vout(e: u32, vp: vec3<f32>, color: vec4<f32>, uv: vec2<f32>, variant: u32) -> VOut {
    let o = e * OUT_WORDS;
    var out: VOut;
    out.clip = to_clip(vp, douts[o + 24u]);
    out.color = color;
    out.uv = uv + vec2<f32>(douts[o + 20u], douts[o + 21u]);
    out.tf = vec4<f32>(douts[o + 16u], douts[o + 17u], douts[o + 18u], douts[o + 19u]);
    out.depth = -vp.z;
    let eb = e * E_WORDS;
    out.mode = vec3<u32>(dels[eb + E_FLAGS], dels[eb + E_BLEND], variant);
    return out;
}

fn hidden() -> VOut {
    var out: VOut;
    out.clip = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    return out;
}

@vertex
fn vs_mesh(v: VIn, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let vp = (out_mat(o) * vec4<f32>(v.pos, 1.0)).xyz;
    return vout(e, vp, v.color, v.uv, inst % PER_ELEM);
}

fn svert(i: u32) -> VIn {
    let b = i * 9u;
    var v: VIn;
    v.pos = vec3<f32>(bitcast<f32>(sverts[b]), bitcast<f32>(sverts[b + 1u]), bitcast<f32>(sverts[b + 2u]));
    let c = sverts[b + 6u];
    v.color = vec4<f32>(f32(c & 0xffu), f32((c >> 8u) & 0xffu), f32((c >> 16u) & 0xffu), f32(c >> 24u)) / 255.0;
    v.uv = vec2<f32>(bitcast<f32>(sverts[b + 7u]), bitcast<f32>(sverts[b + 8u]));
    return v;
}

// D3a: the element's frame (CMoD3aElem), or for a batched element the frame's first quad at every particle
// (CMoVuAnmElem).
@vertex
fn vs_sprite(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let pi = inst % PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let eb = e * E_WORDS;
    let fr = sframes[dels[eb + E_SFRAMES] + u32(douts[o + 22u])];
    if (k >= fr.y * 6u) { return hidden(); }
    let v = svert(fr.x + k);
    let mv = out_mat(o);
    if (dels[eb + E_KIND] != DK_BATCHED) {
        return vout(e, (mv * vec4<f32>(v.pos, 1.0)).xyz, v.color, v.uv, 4u);
    }
    if (f32(pi) >= douts[o + 25u] || k >= 6u) { return hidden(); }
    let pb = (dels[eb + E_PART_BASE] + pi) * PART_WORDS;
    let p0 = vec3<f32>(parts[pb], parts[pb + 1u], parts[pb + 2u]);
    let dir = vec3<f32>(parts[pb + 3u], parts[pb + 4u], parts[pb + 5u]);
    let vel = vec3<f32>(parts[pb + 6u], parts[pb + 7u], parts[pb + 8u]);
    let t = douts[o + 26u];
    let tt = 0.5 * t * t;
    let g = vec3<f32>(douts[o + 28u], douts[o + 29u], douts[o + 30u]);
    let pp = p0 + vel * t + tt * g + (tt * douts[o + 27u]) * dir;
    let m1 = mat3x3<f32>(vec3<f32>(douts[o + 32u], douts[o + 33u], douts[o + 34u]), vec3<f32>(douts[o + 36u], douts[o + 37u], douts[o + 38u]), vec3<f32>(douts[o + 40u], douts[o + 41u], douts[o + 42u]));
    let r = mat3x3<f32>(mv[0].xyz, mv[1].xyz, mv[2].xyz);
    let vp = r * v.pos + m1 * pp + mv[3].xyz;
    return vout(e, vp, v.color, v.uv, 4u);
}

// Lens flare (FUN_1003ddb0): every group's quads in screen space along the line from the light (sx, sy) through the
// centre to (W - sx, H - sy); file units are W/16 x H/16 pixels; alpha x vis / 256. The group t is in normal.x.
@vertex
fn vs_flare(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    let vis = douts[o + 27u];
    if (douts[o + 23u] < 0.5 || vis <= 0.0) { return hidden(); }
    let eb = e * E_WORDS;
    let fr = sframes[dels[eb + E_SFRAMES]];
    if (k >= fr.y * 6u) { return hidden(); }
    let v = svert(fr.x + k);
    let t = bitcast<f32>(sverts[(fr.x + k) * 9u + 3u]);
    let w = d.clip.z;
    let h = d.clip.w;
    let sx = douts[o + 25u];
    let sy = douts[o + 26u];
    let x = v.pos.x * w / 16.0 + (w - 2.0 * sx) * t + sx;
    let y = v.pos.y * h / 16.0 + (h - 2.0 * sy) * t + sy;
    var out: VOut;
    out.clip = vec4<f32>(x / w * 2.0 - 1.0, 1.0 - y / h * 2.0, 0.5, 1.0);
    let a = f32(min(255u, (u32(round(v.color.a * 255.0)) * u32(vis)) >> 8u)) / 255.0;
    out.color = vec4<f32>(v.color.rgb, a);
    out.uv = v.uv;
    out.tf = vec4<f32>(douts[o + 16u], douts[o + 17u], douts[o + 18u], douts[o + 19u]);
    out.depth = 0.0;
    out.mode = vec3<u32>(dels[eb + E_FLAGS], dels[eb + E_BLEND], 5u);
    return out;
}

// The screen glare of a flare (FUN_10053920 step 7 -> FUN_100125c0): g = max(0, 1 - r 2/3) from the screen
// centre, f = g x 0.5 x (vis >> 3) / 128, the op 0x4b colour x f added to the whole view.
@vertex
fn vs_glare(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    let vis = douts[o + 27u];
    if (douts[o + 23u] < 0.5 || vis <= 0.0) { return hidden(); }
    let hw = floor(d.clip.z / 2.0);
    let hh = floor(d.clip.w / 2.0);
    let nx = (hw - douts[o + 25u]) / hw;
    let ny = (hh - douts[o + 26u]) / hh;
    let g = max(0.0, 1.0 - sqrt(nx * nx + ny * ny) * (2.0 / 3.0));
    let f = g * 0.5 * floor(vis / 8.0) / 128.0;
    var c = dels[e * E_WORDS + E_GLARE];
    if (c == 0u) { c = 0x80808080u; }
    let rgb = vec3<f32>(floor(f32((c >> 16u) & 0xffu) * f), floor(f32((c >> 8u) & 0xffu) * f), floor(f32(c & 0xffu) * f)) / 255.0;
    let p = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    var out: VOut;
    out.clip = vec4<f32>(p[min(k, 2u)], 0.5, 1.0);
    out.color = vec4<f32>(rgb, 1.0);
    out.uv = vec2<f32>(0.0);
    out.tf = vec4<f32>(1.0);
    out.depth = 0.0;
    out.mode = vec3<u32>(0x2000000u, 0x48u, 6u);
    return out;
}

// CMoDist (FUN_100418b0, pass 2): a fan of the cross (0,0,0), (-1,0,0), (0,-1,0), (1,0,0), (0,1,0) through +0xa0
// with its view translation shifted by (off, off), sampling the scene copy where the unshifted points project;
// the centre has the element colour (alpha x fade), the tips 0x00808080. Variant 9.
@vertex
fn vs_dist(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5 || k >= 12u) { return hidden(); }
    let cross = array<vec3<f32>, 5>(vec3<f32>(0.0), vec3<f32>(-1.0, 0.0, 0.0), vec3<f32>(0.0, -1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0));
    let tri = k / 3u;
    let c = k % 3u;
    var pi = 0u;
    if (c == 1u) { pi = tri + 1u; } else if (c == 2u) { pi = (tri + 1u) % 4u + 1u; }
    let mv = out_mat(o);
    let vp = (mv * vec4<f32>(cross[pi], 1.0)).xyz;
    let cl = to_clip(vp, douts[o + 24u]);
    let off = bitcast<f32>(dels[e * E_WORDS + E_DIST_OFF]);
    var out = vout(e, vp + vec3<f32>(off, off, 0.0), vec4<f32>(0.0), vec2<f32>(0.0), 9u);
    out.uv = vec2<f32>(cl.x / cl.w * 0.5 + 0.5, 0.5 - cl.y / cl.w * 0.5);
    if (pi == 0u) { out.color = out.tf; } else { out.color = vec4<f32>(vec3<f32>(128.0 / 255.0), 0.0); }
    return out;
}

// The scene copy's lookup at a view point (FUN_10042d00 / FUN_1006eb70: through the projection with |w|, the copy
// flipped back; top-left origin).
fn screen_uv(vp: vec3<f32>) -> vec2<f32> {
    let c = d.proj * vec4<f32>(vp, 1.0);
    var w = abs(c.w);
    if (w == 0.0) { w = 1.0; }
    return vec2<f32>(0.5 + 0.5 * c.x / w, 0.5 - 0.5 * c.y / w);
}

// CMoDistModelElem (FUN_10043320): the mesh drawn unshifted; each vertex samples the scene copy at the screen point of
// pos + (off, off, 0) in model space through the element matrix; the UV scroll slides the lookup (V flipped back).
// Variant 11.
@vertex
fn vs_distmodel(v: VIn, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let mv = out_mat(o);
    let off = bitcast<f32>(dels[e * E_WORDS + E_DIST_OFF]);
    let vp = (mv * vec4<f32>(v.pos, 1.0)).xyz;
    let su = (mv * vec4<f32>(v.pos + vec3<f32>(off, off, 0.0), 1.0)).xyz;
    var out = vout(e, vp, v.color, vec2<f32>(0.0), 11u);
    out.uv = screen_uv(su) + vec2<f32>(douts[o + 20u], -douts[o + 21u]);
    return out;
}

// CMoDistRingElem (FUN_10043720): a closed S-gon per ring pair (angle (q mod S) 2 pi / S), each vertex sampling the
// scene copy at its own screen point (no offset, no scroll); the ring colour per byte x the faded element colour >> 7,
// wrapping. Variant 12 (TSS 0x10353378, MIRROR).
@vertex
fn vs_distring(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let eb = e * E_WORDS;
    let sr = dels[eb + E_RING_SR];
    let segs = sr & 0xffu;
    let rings = (sr >> 8u) & 0xffu;
    if (segs == 0u || rings < 2u) { return hidden(); }
    let per = 6u * segs;
    let p = k / per;
    if (p + 1u >= rings) { return hidden(); }
    let j = (k % per) / 3u + k % 3u;
    let ang = f32((j / 2u) % segs) * 6.2831853 / f32(segs);
    let ring = p + j % 2u;
    let r = bitcast<f32>(dels[eb + E_RING_R + ring]);
    let vp = (out_mat(o) * vec4<f32>(sin(ang) * r, cos(ang) * r, 0.0, 1.0)).xyz;
    let c = dels[eb + E_RING_C + ring];
    let tf = vec4<u32>(round(vec4<f32>(douts[o + 16u], douts[o + 17u], douts[o + 18u], douts[o + 19u]) * 255.0));
    let cb = vec4<u32>((c >> 16u) & 0xffu, (c >> 8u) & 0xffu, c & 0xffu, c >> 24u);
    let dc = vec4<f32>(((cb * tf) >> vec4<u32>(7u)) & vec4<u32>(0xffu)) / 255.0;
    var out = vout(e, vp, dc, vec2<f32>(0.0), 12u);
    out.uv = screen_uv(vp);
    return out;
}

// RingProg (FUN_100555f0): per ring pair p a triangle strip of 2S + 2 vertices, as triangles (strip index t + c);
// strip pair q at angle 0 for q 0 and S, else (q - 1) 2 pi / S (the literal degenerate first pair); even strip
// vertices on ring p, odd on ring p + 1, at (sin a r, cos a r, 0) with that ring's colour. Colour diffuse x TFACTOR
// x 2, alpha x 4 (TSS 0x10348678, variant 3), TFACTOR the raw element colour (+0xf8, no fade). Untextured.
@vertex
fn vs_ring(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let eb = e * E_WORDS;
    let sr = dels[eb + E_RING_SR];
    let segs = sr & 0xffu;
    let rings = (sr >> 8u) & 0xffu;
    if (segs == 0u || rings < 2u) { return hidden(); }
    let per = 6u * segs;
    let p = k / per;
    if (p + 1u >= rings) { return hidden(); }
    let j = (k % per) / 3u + k % 3u;
    let q = j / 2u;
    var ang = 0.0;
    if (q != 0u && q != segs) { ang = f32(q - 1u) * 6.2831853 / f32(segs); }
    let ring = p + j % 2u;
    let r = bitcast<f32>(dels[eb + E_RING_R + ring]);
    let mv = out_mat(o);
    let vp = (mv * vec4<f32>(sin(ang) * r, cos(ang) * r, 0.0, 1.0)).xyz;
    let c = dels[eb + E_RING_C + ring];
    let dc = vec4<f32>(f32((c >> 16u) & 0xffu), f32((c >> 8u) & 0xffu), f32(c & 0xffu), f32(c >> 24u)) / 255.0;
    var out = vout(e, vp, dc, vec2<f32>(0.0), 3u);
    let t = dels[eb + E_COLOR];
    out.tf = vec4<f32>(f32((t >> 16u) & 0xffu), f32((t >> 8u) & 0xffu), f32(t & 0xffu), f32(t >> 24u)) / 255.0;
    return out;
}

// DamValueProg (FUN_10041af0 / FUN_10041c20): glyph k / 6 of the number as a screen quad. The element's view origin
// passes the name-plate test and scale (FUN_100830c0), is pulled 1 unit toward the camera and projected; glyphs
// are centred on it, each [x, x + w] x [-(h / 2), h / 2] units of (W, H) x scale x 0.00234375 px. Diffuse = the
// element colour, the message colour where that is grey 0x808080. Variant 10.
@vertex
fn vs_damv(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let eb = e * E_WORDS;
    let info = dels[eb + E_GLARE];
    let n = min(info >> 24u, 16u);
    let g = k / 6u;
    if (g >= n) { return hidden(); }
    let vo = out_mat(o)[3].xyz;
    let dist = -vo.z;
    if (dist < 1.0) { return hidden(); }
    let zi = i32(4096.0 * 65535.0 / (65535.0 - 0.1) * (1.0 - 0.1 / dist));
    if (zi > 4100) { return hidden(); }
    var scale = 1.0;
    if (zi >= 4020) { scale = f32(4100 - zi) / 80.0; }
    let hw = floor(d.clip.z / 2.0);
    let hh = floor(d.clip.w / 2.0);
    let s = vec2<f32>(d.clip.z, d.clip.w) * scale * 0.00234375;
    let c = to_clip(vo + vec3<f32>(0.0, 0.0, 1.0), douts[o + 24u]);
    let zn = c.z / c.w;
    if (zn < 0.0 || zn >= 1.0) { return hidden(); }
    let px = (c.x / c.w + 1.0) * hw - 0.5;
    let py = (1.0 - c.y / c.w) * hh - 0.5;
    var total = 0;
    var x0 = 0;
    var id = 0u;
    for (var q = 0u; q < n; q = q + 1u) {
        let gq = (dels[eb + E_MORPH_W + q / 8u] >> (4u * (q % 8u))) & 0xfu;
        let wq = i32(d.glyphs[2u * min(gq, 10u) + 1u].x);
        if (q < g) { x0 = x0 + wq; }
        if (q == g) { id = min(gq, 10u); }
        total = total + wq;
    }
    let gw = i32(d.glyphs[2u * id + 1u].x);
    let gh = i32(d.glyphs[2u * id + 1u].y);
    let x = -(total / 2) + x0;
    let corner = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u)[k % 6u];
    let right = (corner & 1u) != 0u;
    let bottom = corner >= 2u;
    let ux = f32(select(x, x + gw, right));
    let uy = f32(select(-(gh / 2), gh / 2, bottom));
    let sp = vec2<f32>(px, py) + vec2<f32>(ux, uy) * s;
    let r = d.glyphs[2u * id];
    var out = vout(e, vo, vec4<f32>(0.0), vec2<f32>(0.0), 10u);
    out.clip = vec4<f32>((sp.x + 0.5) / hw - 1.0, 1.0 - (sp.y + 0.5) / hh, zn, 1.0);
    out.uv = vec2<f32>(select(r.x, r.z, right), select(r.y, r.w, bottom));
    let tf = out.tf;
    var rgb = tf.rgb;
    if (all(round(tf.rgb * 255.0) == vec3<f32>(128.0))) {
        rgb = vec3<f32>(f32((info >> 16u) & 0xffu), f32((info >> 8u) & 0xffu), f32(info & 0xffu)) / 255.0;
    }
    out.color = vec4<f32>(rgb, tf.a);
    return out;
}

// D3b morph (FUN_1003e600): corner k of the element's mesh, its position blended from the key meshes by the
// normalised weights; colours come doubled from the loader. Variant 7 (8 with the alpha test).
@vertex
fn vs_morph(@builtin(vertex_index) k: u32, @builtin(instance_index) inst: u32) -> VOut {
    let e = inst / PER_ELEM;
    let o = e * OUT_WORDS;
    if (douts[o + 23u] < 0.5) { return hidden(); }
    let eb = e * E_WORDS;
    let base = dels[eb + E_SFRAMES];
    let kk = mdata[base];
    let nv = mdata[base + 1u];
    let nn = mdata[base + 2u];
    let nt = mdata[base + 3u];
    if (k >= nt * 3u) { return hidden(); }
    let c = base + 4u + k * 5u;
    let pi = mdata[c];
    let keys = base + 4u + nt * 15u;
    var pos = vec3<f32>(0.0);
    for (var q = 0u; q < min(kk, 5u); q = q + 1u) {
        let a = keys + (q * nv + pi) * 3u;
        pos = pos + douts[o + 32u + q] * vec3<f32>(bitcast<f32>(mdata[a]), bitcast<f32>(mdata[a + 1u]), bitcast<f32>(mdata[a + 2u]));
    }
    let col = mdata[c + 2u];
    let color = vec4<f32>(f32(col & 0xffu), f32((col >> 8u) & 0xffu), f32((col >> 16u) & 0xffu), f32(col >> 24u)) / 255.0;
    let uv = vec2<f32>(bitcast<f32>(mdata[c + 3u]), bitcast<f32>(mdata[c + 4u]));
    let atest = select(7u, 8u, (dels[eb + E_GLARE] & 1u) != 0u);
    let vp = (out_mat(o) * vec4<f32>(pos, 1.0)).xyz;
    return vout(e, vp, color, uv, atest);
}

fn sat(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn sat3(x: vec3<f32>) -> vec3<f32> { return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0)); }

// The texture stages of each path (model_draw.md 1.3 / 2, d3a_particles.md 2.2). Texture alpha comes doubled from
// the upload (0x80 = 1.0), so retail's 2 x Ta is `t.a` here.
@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    let flags = i.mode.x;
    let mode = i.mode.y & 0xffu;
    let variant = i.mode.z;
    let dd = i.color;
    var f = i.tf;
    var rgb: vec3<f32>;
    var a: f32;
    let vonly = (flags & 0x10000000u) != 0u;
    if ((variant <= 1u || variant == 11u) && mode == 0x44u && f.a > 126.0 / 255.0) { f.a = 1.0; }
    if (variant == 0u) {
        let t = textureSample(tex, samp, i.uv);
        rgb = sat3(sat3(2.0 * dd.rgb * t.rgb) * 2.0 * f.rgb);
        a = sat(sat(dd.a * t.a) * 4.0 * f.a);
        if (vonly) { a = sat(dd.a * 4.0 * f.a); }
        if (mode == 0x49u) { rgb = vec3<f32>(1.0); }
    } else if (variant == 1u) {
        rgb = sat3(2.0 * dd.rgb * f.rgb);
        a = sat(select(4.0, 2.0, mode == 0x44u) * dd.a * f.a);
    } else if (variant == 2u) {
        let t = textureSample(tex, samp, i.uv);
        if (mode == 0x64u || vonly) {
            rgb = sat3(sat3(t.rgb * dd.rgb) * f.rgb * 4.0);
            a = sat(4.0 * dd.a * f.a);
        } else {
            rgb = sat3(sat3(t.rgb * f.rgb) * dd.rgb * 4.0);
            a = sat(sat(t.a * f.a) * dd.a * 4.0);
        }
    } else if (variant == 3u) {
        rgb = sat3(2.0 * dd.rgb * f.rgb);
        a = sat(4.0 * dd.a * f.a);
    } else if (variant == 5u) {
        // Flare stages: colour diffuse x texture x 2 (TFACTOR rgb not applied), alpha x TFACTOR x 2.
        let t = textureSample(tex, samp, i.uv);
        rgb = sat3(2.0 * dd.rgb * t.rgb);
        a = sat(sat(dd.a * t.a) * f.a * 2.0);
    } else if (variant == 6u) {
        rgb = dd.rgb;
        a = 1.0;
    } else if (variant == 9u) {
        // TSS 0x10347f68: colour = texture x diffuse x 2, alpha = diffuse x TFACTOR (0x80, left by pass 1) x 4.
        var t = textureSample(tex, samp, i.uv).rgb;
        if (d.fog.z > 0.5) { t = pow(t, vec3<f32>(1.0 / 2.2)); }
        rgb = sat3(2.0 * dd.rgb * t);
        a = sat(dd.a * (128.0 / 255.0) * 4.0);
    } else if (variant == 11u) {
        // DistModel (TSS 0x10352740, 0x10352848 for 0x44): the copy's colour; alpha k x copy alpha (0x80) x stage 0's
        // 2 x diffuse x TFACTOR, k 2 for 0x44 else 4. The mesh texture is never bound.
        var t = textureSample(tex, samp, i.uv).rgb;
        if (d.fog.z > 0.5) { t = pow(t, vec3<f32>(1.0 / 2.2)); }
        rgb = t;
        a = sat(select(4.0, 2.0, mode == 0x44u) * (128.0 / 255.0) * sat(2.0 * dd.a * f.a));
    } else if (variant == 12u) {
        // DistRing: colour 2 x diffuse x copy, alpha 4 x diffuse a x copy alpha (0x80).
        var t = textureSample(tex, samp, i.uv).rgb;
        if (d.fog.z > 0.5) { t = pow(t, vec3<f32>(1.0 / 2.2)); }
        rgb = sat3(2.0 * dd.rgb * t);
        a = sat(4.0 * dd.a * (128.0 / 255.0));
    } else if (variant == 7u || variant == 8u) {
        // D3b stages (TSS 0x103470b8): texture x diffuse x 2, then x TFACTOR x 2 (alpha x 4).
        let t = textureSample(tex, samp, i.uv);
        let s0 = sat3(2.0 * dd.rgb * t.rgb);
        rgb = sat3(2.0 * s0 * f.rgb);
        a = sat(4.0 * sat(dd.a * t.a) * f.a);
        if (variant == 8u && a <= 96.0 / 255.0) { discard; }
    } else {
        let t = textureSample(tex, samp, i.uv);
        let s0 = sat3(2.0 * dd.rgb * t.rgb);
        var s0a = sat(dd.a * t.a);
        if (vonly) { s0a = dd.a; }
        rgb = sat3(2.0 * s0 * f.rgb);
        a = sat(4.0 * s0a * f.a);
        if (mode == 0x40u) { a = f.a; }
    }
    // Scenery fog on view depth unless flag 25; black in mode 0x48 (FUN_10045950). D3a turns fog off.
    if ((variant < 4u || variant == 7u || variant == 8u || variant == 11u || variant == 12u) && (flags & 0x2000000u) == 0u) {
        let ff = sat((d.fog.y - i.depth) / (d.fog.y - d.fog.x));
        var fc = d.fog_color.rgb;
        if (mode == 0x48u) { fc = vec3<f32>(0.0); }
        rgb = mix(fc, rgb, ff);
    }
    if (d.fog.z > 0.5) { rgb = pow(rgb, vec3<f32>(2.2)); }
    return vec4<f32>(rgb, a);
}
