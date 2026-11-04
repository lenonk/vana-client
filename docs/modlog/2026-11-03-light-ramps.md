## Scheduler ops 0x6f / 0x70: point-light ramps (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03); notes in
`docs/re/notes/2026-11-03-modlog-gaps-batch3.md` §5.
- Both are 5 dwords: @4 wait, @6 duration, @8 generator 4cc, @0xc a runtime cache, @0x10 f32 target (753 / 438 uses
  in ROM..ROM9). Asm 0x10059da7: the 4cc resolves to a type-5 generator that must be static (FUN_100539e0) with a
  live CMoPointLightProgElem; a sub-task (FUN_10060990, a child of the scheduler task) lerps 0x6f's intensity
  multiplier (+0x1a4) or 0x70's range multiplier (+0x1a8) from its current value to the target over dur x speed
  ticks; its destructor writes the exact target, also when the scheduler dies first.
- The light frame (FUN_10056400) uses intensity = max(0, +0x1a0) x +0x1a4 x fade and range = +0x1a8 x +0x19c.
- Ours: the template instance's first live point-light element ramps; the start value is the CPU's copy of the
  multiplier (set at emission, and by earlier ramps; a GPU keyframe on it (update op 0x5d) is not seen). A second
  ramp on the same word replaces the first (retail runs both).
  A ramp whose generator has no light out yet (started in the same step: ours emits after the scheduler ops) waits
  for it, its time running from the op.
