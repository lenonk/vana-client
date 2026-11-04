## Misc. 2: Macro palette position setting (2026-11-03)

- Config 0x9e (Misc. 2, second macro palette option) was saved but never read; the palettes always used the
  0 behaviour. Retail (FFXiMain.dll, checked 2026-11-03): FUN_1019b6d0 copies the Misc. 2 values into the macro
  object (0x9d to DAT_106218ed, 0x9e to DAT_106218ee), storing -1 for 0x9e when the UI is under 640 wide and the long
  palette is off. The palette placers (0x101a38a0, 0x101a3f20, 0x101a4871, 0x101a4f4b) read DAT_106218ee: 0 moves
  the palette 0x82 right while gaugewin or cnttime is up, 1 always moves it, anything else never.
- Ours: macroedit::palette_dx follows the same rule for the play and edit palettes.
