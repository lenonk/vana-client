## Weather: the thunder screen flash, its occlusion test, and a census of every zone's weather (2026-11-03)

- Census (`weather-sweep --out sweep.tsv`): every zone 0..299 with a `weat` folder (265 zones, 1226 weathers),
  each weather started and run 600 ticks with the effect audit on, plus the ops by stream and the draw kinds of
  its generators and schedulers. Before this change the only gaps were emission op 0x88 (8 zones) and four
  resources not found; update ops 0x43 / 0x53 / 0x60 run on the GPU, where the audit does not see them, and
  0x53 / 0x60 had no case there. Rain, snow, blizzard, thunder, sandstorm, aurora and light had no other gap.
- Retail build for the addresses below: the unpacked 30261006 FFXiMain (several older addresses in the notes
  above do not exist in it: update stream FUN_1004a980 here, table 0x1004e03c; generator stream FUN_100496f0).
- Emission op 0x88 (0x1005156d): element +0x160 = word 1, a 4-character tag ('ki03', 'lt01', 'pl_c'); op 0x9c
  (0x1005157b) copies the parent's to a child. Nothing in the effects code reads +0x160: a no-op in ours.
  Weather use: only zone 153's `weat/{fine,suny}/kira/ray1..3`; the other seven zones carry it in zone effects
  (wheels, doors) that ran during the census.
- Update op 0x43 (0x1004d67b): IsType 0xe (sound elements) only, +0x1be = round(key x 127), the volume
  FUN_100364e0 reads. Ours already did this (Sound::vol_key); a no-op on the other classes, as before.
- Update op 0x53 (0x1004afb4, any class): the occlusion test. M = identity scaled by word 1 (the box's half size)
  at the element's view-space position (FUN_10044f10); its 8 corners projected (cube table 0x10353718), a 16 x 16
  grid over the screen rect counted against the scene (FUN_1006c300 / FUN_1006c530 / FUN_1006c8b0). Word 2 = 0:
  FADE = 0 when no point passes (FUN_1004a870); else FADE x= count / 256 (FUN_1004e630) x word 2. Only zone 26
  `t_ta/weat/thdr/wez4/frsh`: [0x353, 8.0, 1.0]. Ours: the update pass reads this frame's scene depth (drawn
  before it), each point tested at the box centre's depth *(inf: the depth tested, and retail's 0x100 margin
  around the screen is not kept)*; without a scene depth (the offscreen shots) the op does nothing.
- Update op 0x60 (0x1004abd3, any class): the screen flash. Skipped unless last frame's sort key +0x12c is in
  0..word 2 and FADE != 0. With v the view-space position, z = max(-v.z, 1) (skipped behind the eye), k = cam
  +0x2f4 / z, r^2 = (k v.x)^2 + (k v.y)^2: f = 1 within word 3, 1 - (r^2 - w3) / (w4 - w3) up to word 4, else
  skipped; then the eye distance d: skipped beyond word 2, f x= 1 - (d - w1) / (w2 - w1) beyond word 1; then the
  player factor FUN_1003c7d0 (flags word 6, f32 word 5; the rule of emission op 0x82). The colour bytes +0xf8..
  +0xfa x f are added to renderer +0x324..+0x326 (FUN_10014f70 / FUN_10014f90, saturating), which FUN_100132b0
  draws as the additive full-screen quad (alpha doubled; default alpha byte 0x80, 0x100149d4) that event op 0x72
  also uses. Users: zone 26 frsh [0x760, 50, 150, 4096, 65536, 0, 1] and zones 29 / 30 / 31
  `d_ba/weat/thdr/limg/nul1` [0x760, 1000, 1000, 4096, 65536, 0, 1].
- Both ops take +0x188 as FUN_10044f10 builds it: camera-relative elements (flags bit 2, the Riverne bolts are
  0x00800005) as view_rot x pos (bit 3 too: pos itself), 0x20000 the parent translation in view space plus pos,
  else view x the world position; op 0x60's eye distance still takes the raw position as a world point
  (FUN_10044c40), as retail does. Retail's op 0x60 reads +0x188 from its last refresh, likely a frame old; ours
  this frame's.
- Checked on the game data: the flash fires in zones 26 (about every 50 to 60 s) and 29 / 30 / 31 for 25 to 31
  ticks, peaking at 128 per channel and fading out; a scene depth of 0 suppresses zone 26's through op 0x53.
- Ours: the GPU update writes each element's flash bytes into its readback record (word 19), summed on the CPU
  (Effects::flash) and added to op 0x72's colour in Screen::flash, so the flash shows a frame or two late. Element
  word 42 keeps last frame's sort key, word 43 the instance's own-player bits for the player factor. The bytes
  are added channel for channel, as op 0x72's colour is.
- Checked and left as they are: the misses of DAT 143 'star' -> 'sta1', DAT 146 / 147 'sta1' -> 'sta2' (only an
  image of that name) and DAT 188 'cld2' -> 'clod' happen in retail too (the lookup FUN_10072290 / FUN_100722c0
  needs the exact type; none of those names exists as Vum, D3m or MMB in the file or DAT 0). Generator update
  op 0x5d (zones 25, 29..31, raw [0x15d]) takes the default arm 0x1004a55d of FUN_100496f0: a no-op. The sun and
  moon tilt (sun / moon object +0x1cc, Rx(degrees) in FUN_10187720) and the fixed sky time (+0x1c8) are set only
  by scheduler ops 0xc3 / 0xc0 (FUN_10057fb0 0x1005809b / 0x10058079), which no zone 0..299 scheduler uses, so
  zone weather never tilts them; ours leaves both out.
- Test: render::effects::tests::gpu_thunder_flash (visible, hidden behind scenery, beyond the far distance).
