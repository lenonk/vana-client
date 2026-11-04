## 0x5b / 0x66: a cached motion file costs one call (2026-11-03)

`FUN_100b5210(kind, yield_after, with_speed)` (op 0x5b -> kind 0, op 0x66 -> kind 1) does the request and the test in
one pass:

1. Both actors are checked against their server ids and need +0x120 bits 9 and 7; otherwise the op skips.
2. If the per-kind "requested" flag (body [+0x260] + 0x160 + kind) is 0, the file is requested on actor 1's drawobj
   and the flag is set to 1 — **and the same call carries on**.
3. `FUN_100d12d0(drawobj)`, "all of the model's files are in", decides: false yields without advancing, and the next
   frame re-runs only this test, so no second request goes out.
4. True starts the scheduler (drawobj vt+0x298), clears the flag, advances and yields once.

So a resident file costs one call, not two; an uncached one costs 1 + the load frames, and the wait is on the model's
whole file set rather than the one file. Ours kept the op waiting a frame even for a file already in
`MotionFiles`, because `Busy::MotionLoad` only asked whether the actor had a pending motion. It now asks whether that
pending motion's files are still loading, which is the same test in the same call. The thread's state byte is the
requested flag, so a waiting op still does not re-request.
- Ours: because the VM can run two ticks in one drawn frame (30 fps) and an actor has one pending-motion slot, the
  ready test (Busy::MotionLoad) also starts the motion at once (actmotion::start_pending), as retail's call does,
  rather than leaving it for actmotion::update after the VM, where a second 0x5b / 0x66 could replace it.
