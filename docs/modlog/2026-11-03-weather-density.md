## Misc.: Weather effects setting (2026-11-03)

- Config 0x3b (Misc. > Weather, 1 Normal / 0 Off) was saved but never read; the generator clock had a fixed
  1.0. Retail (FFXiMain.dll, checked 2026-11-03):
  - LAB_101aa650 case 0x3b (FUN_10002d70): DAT_10456a68 = the value as a float. FUN_10002d80 reads it, or 1.0
    while DAT_10487ef5 is set: event op 0xab subs 9 / 10 (FUN_10087120 = 1 / 0); the zone load FUN_10093720
    clears it.
  - FUN_100496f0 (0x1004a654): a generator with +0xde bit 1 counts its emission clock down by dt x density, so
    Off stops new weather particles.
  - FUN_10063c80 (CMoVuDist, 0x10063c89..0x10063cc5): N from 20 up on a generator with +0xde bit 0 draws
    N x density x 0.5 each draw (was a fixed 0.5 at emission). Ours: Draw::VuDist keeps N and the weather bit, and
    render/effects.rs vu_dist_count works out the draws each frame; the batched particle count in effects.wgsl
    uses the same formula.
  - FUN_1003fbb0 (D3b), FUN_10040ba0 (D3m) and FUN_10043320 (DistModel) draw nothing from a generator with +0xde
    bit 7 while the density is 0.
- Ours: fx::Effects::weather_density each frame (World::weather_forced for the event flag), Params.extra.z in
  render/effects.wgsl.
