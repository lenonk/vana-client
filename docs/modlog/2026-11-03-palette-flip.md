## Macro palette: a set change fades the palette out and back in (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 78): the "flip" MODLOG called a zoom is
  a fade. A set change through the play palette's handler 0x101a3960 (Up / Down while it is open) refreshes the cells
  at once (0x101a3d00(1)) and sets +0x29 = 1. Each frame (0x101a3a92..0x101a3b09) then drives the frame part's fade
  (the generic part tick 0x10119707..):
  - +0x44 = 2 for 20 ticks: alpha 0.05 x the ticks left, so 1 to 0;
  - +0x44 = 1 for the frame that sees the end: hidden;
  - +0x44 = 3 for 20 ticks: alpha 1 - 0.05 x the ticks left, so 0 to 1.
  The cells and titles are the part's children and take its alpha *(inf)*.
- Ours: Up / Down on the open palette start `World::palette_flip`, and the palette's draw (frame, cells, titles,
  hand) is faded by `palette_flip_alpha` over 40 ticks at 60 a second. The one hidden frame is folded into the turn.
  `UiBatch::fade_from` fades what was drawn since a mark (alpha, and the colour of the additive glyphs).
- The fields' Shift-JIS byte limits (15 / 8 / 60) were already built (macroedit.rs `limit`).
- Not built: the editor palettes mcr1edit / mcr2edit run the same flip on their own +0x48 (0x101a4cc3.., 0x101a53bf..).
