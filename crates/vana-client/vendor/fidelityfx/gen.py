#!/usr/bin/env python3
"""Regenerates src/render/upscale/fsr/*.wgsl from AMD's FSR 3.1 GLSL here.

Needs glslc, spirv-opt and naga-cli of the naga version wgpu uses (`cargo install naga-cli --version 30.0.1`).
The headers are patched on a copy (target/fsr-gen) for wgpu / naga: see PATCHES. Run from anywhere.
"""
import pathlib
import re
import shutil
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
CRATE = HERE.parent.parent
WORK = CRATE.parent.parent / "target" / "fsr-gen"
OUT = CRATE / "src" / "render" / "upscale" / "fsr"
NAGA = shutil.which("naga") or str(pathlib.Path.home() / ".cargo" / "bin" / "naga")

CALLBACKS = "fsr3upscaler/ffx_fsr3upscaler_callbacks_glsl.h"
# Our changes to the GLSL callbacks (the backend's half of the shaders):
# - naga has no image atomics: the reconstructed depth and SPD's counter are storage buffers.
# - WGSL has no coherent images: SPD's mip 5 (read back by the last workgroup) is mirrored in a coherent buffer;
#   the image stays, for the passes that sample the chain.
# - The upscaled output gets a format (ours: rgba16float).
# - The samplers' bindings 1000 / 1001 become 98 / 99 (wgpu's highest binding is 1000).
PATCHES = [
    ("layout (set = 0, binding = 1000) uniform sampler s_PointClamp;",
     "layout (set = 0, binding = 98) uniform sampler s_PointClamp;"),
    ("layout (set = 0, binding = 1001) uniform sampler s_LinearClamp;",
     "layout (set = 0, binding = 99) uniform sampler s_LinearClamp;"),
    ("""layout (set = 0, binding = FSR3UPSCALER_BIND_SRV_RECONSTRUCTED_PREV_NEAREST_DEPTH) uniform utexture2D r_reconstructed_previous_nearest_depth;

FfxFloat32 LoadReconstructedPrevDepth(FfxInt32x2 iPxPos)
{
	return uintBitsToFloat(texelFetch(r_reconstructed_previous_nearest_depth, iPxPos, 0).r);
}""",
     """layout (set = 0, binding = FSR3UPSCALER_BIND_SRV_RECONSTRUCTED_PREV_NEAREST_DEPTH, std430) readonly buffer ReconstructedPrevDepthR { FfxUInt32 data[]; } r_reconstructed_previous_nearest_depth;

FfxFloat32 LoadReconstructedPrevDepth(FfxInt32x2 iPxPos)
{
	return uintBitsToFloat(r_reconstructed_previous_nearest_depth.data[iPxPos.y * MaxRenderSize().x + iPxPos.x]);
}"""),
    ("""layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_RECONSTRUCTED_PREV_NEAREST_DEPTH, r32ui) uniform uimage2D  rw_reconstructed_previous_nearest_depth;""",
     """layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_RECONSTRUCTED_PREV_NEAREST_DEPTH, std430) buffer ReconstructedPrevDepthW { FfxUInt32 data[]; } rw_reconstructed_previous_nearest_depth;
#define RECONSTRUCTED_AT(p) rw_reconstructed_previous_nearest_depth.data[(p).y * MaxRenderSize().x + (p).x]"""),
    ("imageAtomicMax(rw_reconstructed_previous_nearest_depth, iPxSample, uDepth);",
     "atomicMax(RECONSTRUCTED_AT(iPxSample), uDepth);"),
    ("imageAtomicMin(rw_reconstructed_previous_nearest_depth, iPxSample, uDepth);",
     "atomicMin(RECONSTRUCTED_AT(iPxSample), uDepth);"),
    ("imageStore(rw_reconstructed_previous_nearest_depth, iPxSample, uvec4(uValue, 0, 0, 0));",
     "RECONSTRUCTED_AT(iPxSample) = uValue;"),
    ("""layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_SPD_GLOBAL_ATOMIC, r32ui) coherent uniform uimage2D  rw_spd_global_atomic;""",
     """layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_SPD_GLOBAL_ATOMIC, std430) coherent buffer SpdGlobalAtomic { FfxUInt32 counter; } rw_spd_global_atomic;"""),
    ("spdCounter = imageAtomicAdd(rw_spd_global_atomic, ivec2(0, 0), 1);",
     "spdCounter = atomicAdd(rw_spd_global_atomic.counter, 1);"),
    ("imageStore(rw_spd_global_atomic, ivec2(0, 0), uvec4(0));", "rw_spd_global_atomic.counter = 0;"),
    ("FSR3UPSCALER_BIND_UAV_UPSCALED_OUTPUT /* app controlled format */)", "FSR3UPSCALER_BIND_UAV_UPSCALED_OUTPUT, rgba16f)"),
    ("""layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_SPD_MIPS_LEVEL_5, rg16f) coherent uniform image2D  rw_spd_mip5;""",
     """layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_SPD_MIPS_LEVEL_5, rg16f)          uniform image2D  rw_spd_mip5;
layout (set = 0, binding = FSR3UPSCALER_BIND_UAV_SPD_MIPS_LEVEL_5 + 100, std430) coherent buffer SpdMip5 { FfxFloat32x2 data[64 * 64]; } rw_spd_mip5_mirror;"""),
    ("""    LOAD(4);
    LOAD(5);
""", """    LOAD(4);
    if (index == 5)
    {
        return rw_spd_mip5_mirror.data[iPxPos.y * 64 + iPxPos.x];
    }
"""),
    ("""    STORE(4);
    STORE(5);""", """    STORE(4);
    STORE(5);
    if (index == 5)
    {
        rw_spd_mip5_mirror.data[iPxPos.y * 64 + iPxPos.x] = outValue;
    }"""),
]

# The one permutation we run: LDR input, render-size motion vectors without jitter, standard depth, no Lanczos LUT,
# 32-bit floats (naga has no 16-bit types here), SPD through shared memory (no subgroup operations, so no wgpu
# subgroup feature).
BASE = {
    "FFX_SPD_NO_WAVE_OPERATIONS": 1,
    "FFX_GPU": 1,
    "FFX_GLSL": 1,
    "FFX_HALF": 0,
    "FFX_FSR3UPSCALER_OPTION_REPROJECT_USE_LANCZOS_TYPE": 0,
    "FFX_FSR3UPSCALER_OPTION_HDR_COLOR_INPUT": 0,
    "FFX_FSR3UPSCALER_OPTION_LOW_RESOLUTION_MOTION_VECTORS": 1,
    "FFX_FSR3UPSCALER_OPTION_JITTERED_MOTION_VECTORS": 0,
    "FFX_FSR3UPSCALER_OPTION_INVERTED_DEPTH": 0,
    "FFX_FSR3UPSCALER_OPTION_APPLY_SHARPENING": 0,
}
PASSES = [
    ("prepare_inputs", "prepare_inputs", {}),
    ("luma_pyramid", "luma_pyramid", {}),
    ("shading_change_pyramid", "shading_change_pyramid", {}),
    ("shading_change", "shading_change", {}),
    ("prepare_reactivity", "prepare_reactivity", {}),
    ("luma_instability", "luma_instability", {}),
    ("accumulate", "accumulate", {}),
    ("accumulate_sharpen", "accumulate", {"FFX_FSR3UPSCALER_OPTION_APPLY_SHARPENING": 1}),
    ("rcas", "rcas", {}),
    ("debug_view", "debug_view", {}),
]

HEADER = """// Generated by vendor/fidelityfx/gen.py from AMD FidelityFX SDK 1.1.4 (FSR 3.1), MIT licence
// (vendor/fidelityfx/LICENSE.txt). Do not edit: change gen.py and run it again.
"""


def run(cmd):
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit("%s failed:\n%s%s" % (cmd[0], r.stdout, r.stderr))


def main():
    if WORK.exists():
        shutil.rmtree(WORK)
    shutil.copytree(HERE / "gpu", WORK / "gpu")
    cb = WORK / "gpu" / CALLBACKS
    s = cb.read_text()
    for a, b in PATCHES:
        if s.count(a) != 1:
            sys.exit("patch does not apply: " + a.splitlines()[0][:80])
        s = s.replace(a, b)
    cb.write_text(s)
    OUT.mkdir(parents=True, exist_ok=True)
    for name, src, extra in PASSES:
        defs = dict(BASE, **extra)
        spv, opt = WORK / (name + ".spv"), WORK / (name + ".opt.spv")
        wgsl = WORK / (name + ".wgsl")
        run(["glslc", "--target-env=vulkan1.1", "-fshader-stage=comp", "-I", str(WORK / "gpu")]
            + ["-D%s=%d" % kv for kv in defs.items()]
            + ["-o", str(spv), str(HERE / "passes" / ("ffx_fsr3upscaler_%s_pass.glsl" % src))])
        # Dead code out, so every binding left is one the pass uses (wgpu's derived layout holds only those).
        run(["spirv-opt", "-O", str(spv), "-o", str(opt)])
        run([NAGA, str(opt), str(wgsl)])
        (OUT / (name + ".wgsl")).write_text(HEADER + wgsl.read_text())
        n = len(re.findall(r"^@group\(0\) @binding", wgsl.read_text(), re.M))
        print("%-24s %3d bindings %7d bytes" % (name, n, wgsl.stat().st_size))


main()
