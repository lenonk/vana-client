## Event op 0xd3: end the actor's motions (2026-11-02)

- Op 0xd3 sub 0 (FUN_100b7140, from the dispatcher table at 0x100bc960; the old FUN_100b5310 name was off) takes
  actor@2. When it resolves to a CXiSkeletonActor with a model (+0x120 bit 9), FUN_100ce070 goes through both
  motion lists of its model (+0x674 -> +0x44, lists +0x60 / +0x9c) and calls FUN_1001b520(8.0) on each layer.
  A steady layer (state 0) then has state 2 with 8 frames left, so it plays on at its weight and is dropped when
  that time runs out (FUN_1001b5d0 returns 0). A layer fading in (state 1) gets state 3 with 8 frames. Layers
  already ending (2 / 3) keep their time unless it is 1 or less. The scheduler task is not touched. While actor@2
  does not resolve, the op does not advance.
- Scripts use it after a looping event motion (motion_file 'fm01' ...), usually followed by set_idle_motion
  'idl0' (zones 25..293, mostly Adoulin and later). Ours: `Play::end_in(8)` on the actor's play, after which
  `current()` is None and the status motion or the event idle shows. Before this, the loop played on until the
  idle change cut it.
