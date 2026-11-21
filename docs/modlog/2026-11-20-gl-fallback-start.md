## The OpenGL / GLES fallback starts: depth reads, the shadow feet format, adapter limits (2026-11-20)

Not retail (wgpu backend support). The GL backend did not reach a first frame; each of these was a hard failure,
seen on Mesa llvmpipe (GL 4.6) and zink.

- naga's GLSL writer refuses `textureLoad` on a `texture_depth_2d` ("WGSL `textureLoad` from depth textures is not
  supported in GLSL"). The scene-depth reads (classic UI depth test, effects occlusion / soft particles / distortion,
  flares, actor occlusion, SSAO, sun and lamp shadow receive) now bind the Depth32Float view as an unfilterable
  `texture_2d<f32>` and read `.x`: the same texel, so the same picture on every backend. The lamp shadow array's
  comparison sampling is unchanged.
- Mods' post passes (render.shader) keep their header: `depth` stays `texture_depth_2d` except on GL, where it is a
  `texture_2d<f32>` and the mod's `textureLoad(depth, ...)` calls get `.x` added (postfx::gl_source), so a mod
  written for one works on both. Other depth uses (none in the bundled mods) still fail there and skip the pass.
- The sun shadow's feet target was R32Float, which wgpu-GL only renders to when `GL_EXT_color_buffer_float` is
  listed; Mesa's core profiles don't list it, so every Mesa GL driver failed at ZonePipes, shadows on or off. The
  format is now picked from the adapter (R32Float, else R16Float, else no sun / moon shadows and a warning; lamp
  shadows are depth only and still draw).
- The device asked for WebGPU's default limits, which GLES parts don't all meet (a Raspberry Pi's V3D has 15
  inter-stage variables against 16; it failed on its Vulkan driver too). The defaults are now lowered to the
  adapter's own where it has less (`wanted_limits`); a desktop GPU asks for exactly what it did before. Start-up logs
  the limits and the game stops, naming the GPU and what it lacks (headless tools log it), when one is below the client's needs (9 storage buffers in
  one stage for the effects update, 13 inter-stage variables in zone.wgsl, 256 compute invocations for the mip
  generator), instead of a pipeline panic later. `FFXI_LOW_LIMITS` (tests) caps the limits at a GLES 3.1 Mali / V3D
  class device's and goes on past that check.
- The offscreen scene is scaled down (aspect kept) to the device's texture size (4096 on some GLES parts).
- Not fixed here: NVIDIA's own GL gets a 3.3 context from wgpu-hal (no compute). Vertex-stage storage buffers on
  GLES drivers that offer few were not checked on a device.
