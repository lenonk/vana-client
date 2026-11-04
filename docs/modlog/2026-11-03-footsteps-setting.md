## Misc. 2: Footsteps setting (2026-11-03)

- Config 0x3f (Misc. 2 > Footsteps, 0 On / 1 Off, `/footstep`) was saved but never read. Retail (FFXiMain.dll,
  checked 2026-11-03): LAB_101aa650 case 0x3f sets DAT_10456a70 = 1 - value (FUN_10002e20). FUN_100d4bc0 tests it
  at 0x100d4caa: Off skips the water surface for the wade point (it stays the position) and lands on FUN_100d6550,
  which stops the wading ripple. At 0x100d51ab / 0x100d5351, after the step sound has played (0x100d514f), Off
  skips the footprints (`fmrk`) and the dust generator. The step sound, its folder and the foot heights
  (LAB_100d53bc) are unaffected.
- Ours: footsteps::wade passes no water while config 0x3f != 0; footsteps::steps still plays the step sound but
  passes no dust and no footprint.
