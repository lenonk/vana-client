## Faded actors: drawn offscreen, blended as one quad (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch3.md §4): a translucent actor (draw alpha < 1) is drawn opaque,
  alpha-tested and with z written, into a helper target cleared to 0 (FUN_1002b6e0 with FUN_1001d240), then that
  target goes over the scene as one viewport quad, SRCALPHA / INVSRCALPHA, z off, point sampled, at the actor's alpha
  (FUN_1001d3b0). So a faded model never shows its own back faces or overlapping parts through itself. Translucent
  actors are drawn back to front (DAT_1047bc94).
- Ours blended each triangle in the main pass. Now `ZoneRenderer::render` leaves GPU-skinned actors with alpha < 1 out
  of its pass and `render_faded` draws them farthest first: an offscreen pass (`skin_solid_pipeline`, `fs_actor_solid`:
  the actor's colour at alpha 1, its shine too) on the scene depth, then the composite (`faded_pipeline`, `vs_faded` /
  `fs_faded`: the texel, alpha = target alpha x fade). `prepare_faded` makes the target at the scene size each frame;
  callers that don't (the select screen, shot tools) keep the old blend. At most 64 a frame; more fall back.
- *(inf)* The target's alpha where drawn is taken as 1 (retail's MODULATE4X saturates above the 0x45 alpha test).
- Not done: CPU-posed actors (no skeleton on the GPU path) still blend per triangle; the DAT_104566e8+0x18 second-target
  path and the +0x9c effects are not traced. `flash-shot` writes faded_40.png for a live look. Test zone_pipes_build.
