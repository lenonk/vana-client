// The lens-flare visibility count (FUN_1006b350 / FUN_1006b580 / FUN_1006b900): the light's direction projected,
// a +-W/32 x +-H/32 box around it squeezed into a 16 x 16 grid, and 8 x 8 of those samples (even rows and columns)
// tested for sky (depth still at the clear value). vis = 4 x samples, into the element's draw record with the
// screen point.

struct FP {
    proj: mat4x4<f32>,
    view_rot: mat4x4<f32>,
    eye: vec4<f32>,
    // element count
    info: vec4<f32>,
};

@group(0) @binding(0) var<uniform> fp: FP;
@group(0) @binding(1) var<storage, read> el: array<u32>;
@group(0) @binding(2) var<storage, read> slots: array<u32>;
@group(0) @binding(3) var<storage, read_write> outs: array<f32>;
@group(0) @binding(4) var depth: texture_depth_2d;

@compute @workgroup_size(64)
fn flare_vis(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= u32(fp.info.x)) { return; }
    let b = i * E_WORDS;
    if ((el[b + E_STATE] & ST_ALIVE) == 0u || el[b + E_KIND] != DK_FLARE) { return; }
    let o = i * OUT_WORDS;
    outs[o + 27u] = 0.0;
    if (outs[o + 23u] < 0.5) { return; }
    // The parent's translation (the sun / moon placement) as a direction from the eye, x far 65535.
    let parent = el[b + E_PARENT];
    var dv = vec3<f32>(bitcast<f32>(el[b + E_POS]), bitcast<f32>(el[b + E_POS + 1u]), bitcast<f32>(el[b + E_POS + 2u]));
    if (parent != NONE && slots[parent * SLOT_WORDS + S_HAS_MATRIX] != 0u) {
        let s = parent * SLOT_WORDS;
        let t = vec3<f32>(bitcast<f32>(slots[s + 12u]), bitcast<f32>(slots[s + 13u]), bitcast<f32>(slots[s + 14u]));
        dv = normalize(t - fp.eye.xyz) * 65535.0;
    }
    let v = (fp.view_rot * vec4<f32>(dv, 0.0)).xyz;
    let c = fp.proj * vec4<f32>(v, 1.0);
    if (c.w < 0.0) { return; }
    let k = select(1.0 / abs(c.w), 1.0, c.w == 0.0);
    let dims = vec2<f32>(textureDimensions(depth));
    let w = dims.x;
    let h = dims.y;
    let sx = (c.x * k + 1.0) * w / 2.0;
    let sy = (1.0 - c.y * k) * h / 2.0;
    outs[o + 25u] = sx;
    outs[o + 26u] = sy;
    let rx = w / 32.0;
    let ry = h / 32.0;
    let ax = i32(sx - rx);
    let bx = i32(sx + rx);
    let ay = i32(sy - ry);
    let by = i32(sy + ry);
    let x1 = min(ax, bx);
    let x2 = max(ax, bx);
    let y1 = min(ay, by);
    let y2 = max(ay, by);
    let wi = i32(w);
    let hi = i32(h);
    let ux1 = clamp(x1, -256, wi + 256);
    let ux2 = clamp(x2, -256, wi + 256);
    let uy1 = clamp(y1, -256, hi + 256);
    let uy2 = clamp(y2, -256, hi + 256);
    let cw = clamp(x2, 0, wi) - clamp(x1, 0, wi);
    let ch = clamp(y2, 0, hi) - clamp(y1, 0, hi);
    if (cw <= 0 || ch <= 0 || ux2 <= ux1 || uy2 <= uy1) { return; }
    let cw16 = (cw * 16) / (ux2 - ux1);
    let ch16 = (ch * 16) / (uy2 - uy1);
    var n = 0u;
    for (var j = 0; j < 8; j = j + 1) {
        for (var q = 0; q < 8; q = q + 1) {
            let tx = 2 * q;
            let ty = 2 * j;
            if (tx >= cw16 || ty >= ch16) { continue; }
            // Texel (tx, ty) of the squeezed copy samples this pixel of the box (UVs from Ux1 + 1 to Ux2).
            let px = f32(ux1 + 1) + (f32(tx) + 0.5) / f32(cw16) * f32(ux2 - ux1 - 1);
            let py = f32(uy1 + 1) + (f32(ty) + 0.5) / f32(ch16) * f32(uy2 - uy1 - 1);
            if (px < 0.0 || py < 0.0 || px >= w || py >= h) { continue; }
            if (textureLoad(depth, vec2<i32>(i32(px), i32(py)), 0) >= 0.99999988) { n = n + 1u; }
        }
    }
    outs[o + 27u] = f32(4u * n);
}
