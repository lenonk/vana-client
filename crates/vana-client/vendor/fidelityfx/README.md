# AMD FidelityFX Super Resolution 3.1 (shaders)

The FSR 3.1 upscaler's GPU code from AMD's FidelityFX SDK 1.1.4
(https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK, tag `v1.1.4`), MIT licence: `LICENSE.txt`.
Copyright (C) 2024 Advanced Micro Devices, Inc.

- `gpu/`: the SDK's `sdk/include/FidelityFX/gpu` headers FSR 3.1 needs (core, fsr1 for RCAS, spd, fsr3upscaler),
  unchanged.
- `passes/`: the SDK's Vulkan pass sources (`sdk/src/backends/vk/shaders/fsr3upscaler`), unchanged.
- `gen.py`: builds `src/render/upscale/fsr/*.wgsl` from them (glslc, spirv-opt, naga). It patches a copy of the GLSL
  callbacks for wgpu first; the changes are listed in `PATCHES`:
  image atomics become storage buffer atomics, SPD's coherent mip 5 gets a coherent buffer mirror, the output image
  gets a format, the sampler bindings move below wgpu's limit.

The host side (resources, constants, dispatch order) is ours, after the SDK's `ffx_fsr3upscaler.cpp`:
`src/render/upscale/fsr.rs`.
