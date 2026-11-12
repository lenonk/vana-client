## Distortion copies: retail's 256x256 target for every distortion draw, the copy's fog (2026-11-09)

Modlog-gaps item 124 ("Distortion: fog under vertex fog is driver-defined; whether device +0x194 is the 3D target in
every mode; the +0x198 target and after-image path; retail's soft 256x256 look as a setting", MODLOG L11700).
Addresses are Steven's FFXiMain.dll.
- Device +0x194 is always the 3D picture: made at the 3D resolution in FUN_10010d20 (FUN_1000a6e0; failing to make
  it fails start-up), set as the render target every frame (FUN_10011a00 -> FUN_1000a800) and put to the window at
  the end (onto +0x1e0 when the UI size differs). No mode draws the world elsewhere.
- Device +0x198 is the 3D LCD Mode (config 0030, DAT_10456abc) eye-1 picture, made only with that mode
  (DAT_104566e8 +0x18). Every eye-1 twin is copied from the eye-0 target by the actor composites (FUN_1001d3b0,
  FUN_10038a80, FUN_10038e10: the translucent-actor target DAT_10456d50 / 54 and the 256x256 DAT_1047b958 / 5c), but
  FUN_10042d00 and FUN_100426f0 copy +0x194 only into the one DAT_1047bb88. The distortion draws then run on both
  eyes (the draw wrappers draw twice), so retail's eye 1 shows eye 0's background through a distortion, 0.6 % of
  the width off. Ours draws the world once for both eyes and crops it, which cannot hold two copies: not modelled.
- The "after-image path" is FUN_10038e10 / FUN_10038a80 (from the actor draw FUN_1002b6e0), which copy actors'
  targets into DAT_1047b958 / 5c, not DAT_1047bb88; none of it touches the distortion copy.
- FOGENABLE during FUN_10042d00's copy is the element's (FUN_10046070 -> FUN_100467e0 sets fog before vt +0x48
  draws: on unless op-1 flag 25, the mode by the device caps +0x92d / +0x92e: table fog LINEAR whenever the device
  has FOGTABLE, which every current device does; mode 0x48's fog colour is black, FUN_10045950). The quad is
  XYZRHW with rhw 1, so table fog (W fog for a pre-transformed vertex) uses depth 1: the copy is mixed toward the fog
  colour by 1 - (end - 1) / (end - start). Under vertex fog (no FOGTABLE cap) a pre-transformed vertex takes its fog
  from the specular alpha, and FVF 0x144 has none: 1.0 (no fog) in D3D's default specular and in DXVK
  (d3d9_fixed_function_vert.vert calculateFog). FUN_100426f0's own passes set FOGENABLE 0 (RS 0x103530d0 /
  0x10353178). Census (fog_span_census, all zone weather keys with retail's end <= start -> start + 1): start is 0 in
  most keys; the copy's fog is at most 8.6 levels in most fog-heavy zones (DAT 319, end 29.7) and the whole fog colour
  in DAT 83925 `suny` (start 0, end 0).
- Ours: `copy_fog` in effects.wgsl for DistModel / DistMorph / DistRing (variants 11, 14 / 15, 12), applied at the
  lookup (fogging a texel before the bilinear lookup is the same), then the element's fog as before.
- DistModel (0x1004348e) and DistMorph (0x10043609) call FUN_10042d00 like DistRing (0x100438ea): all three now
  sample the squeezed 256x256 copy (flipped; vs_distmodel / vs_distmorph uv = (s.x, 1 - s.y) + the UV scroll), WRAP
  for DistModel / DistMorph, MIRROR for DistRing. The 256x256 target is one shared texture (EffectsGpu dist_copy) and
  is never cleared.
- CMoDist / VuDist (FUN_100426f0): the five cross points projected by FUN_1006e7d0 (plain projection, |w|, 0 -> 1;
  X = (x / w + 1) W / 2, Y = (1 - y / w) H / 2); the box over them; k = 255 / extent when the extent is 256.0 or more,
  else 1 (0x1032a344 = 256, 0x10329a20 = 255). Pass 1 draws only the four tips (strip 1, 2, 4, 3) into DAT_1047bb88 at
  (P - min) k with uv P / (W, H) (TSS 0x10353028: SELECTARG1 TEXTURE, alpha TFACTOR 0x80, linear, MIRROR); pass 2's
  fan (FVF XYZ | DIFFUSE | TEX1, the tip 1 repeated to close it) samples (P - min) k x 1/255 (0x10329e94), so the lookup
  runs 256 / 255 of the copy's scale (up to a pixel's drift across the box) and outside the diamond the target holds
  what the last copy left. Ours: vs_dist_copy / fs_dist_copy into dist_copy (half-pixel terms for D3D8's pixel
  centres), vs_dist's uv from dist_box. A diamond up to 255 pixels looks as before; a larger one (the Altepa heat
  haze close by) is squeezed and so softer.
- Seen offscreen: zone-shot 114 --hour 13 --weather heat --effects 240 --eye 60,-30,120 --target 200,-20,120: the haze
  softens the background's edges under the large diamonds (at most 13 levels). fx-shot 230 --category 3 --anim 41
  (fs_i/md00): the DistModel band is softer, same shape. No retail capture compared.
