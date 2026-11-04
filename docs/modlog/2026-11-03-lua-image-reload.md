## Lua mods: a replaced image's old texture is freed (ours, 2026-11-03)

- `hud.image` and `draw.image` key a texture by mod, path and a hash of the file's bytes (`hud::load_png`), so a
  changed PNG uploads anew after a reload. A new version of a mod's image now marks the same file's older contents
  stale (`GameUi::image`), and `GameUi::render` frees a stale texture (`UiRenderer::drop_image`) once the frame's
  batch no longer draws it, so editing and reloading doesn't pile textures up in GPU memory, while two versions of
  one file drawn at once (changed on disk between two calls without a reload) both keep their pixels.
