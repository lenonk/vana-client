## Macro windows: remembered cursors, and the palettes switch on hover (2026-11-03)

**FUN_101262e0 is not a left click.** It is "the mouse is driving the menus": mouse +0x4d == 2 (the pointer is over a
focused window) and no event lock. All 23 callers are per-frame hover tests paired with `FUN_10118c10`, so MODLOG
L10129's "left click" reading is wrong.

- mcr1edit's frame (`FUN_101a47e0`, 0x101a4c3d..) and mcr2edit's twin: the pointer inside the **other palette** calls
  `FUN_101a4540(col)`, which flips the focused palette, sets both columns to that cell and plays SE 0001; the pointer
  inside **mcrbedit** has the palette send itself message 8, i.e. **Num+**. "macro"'s frame does the same for
  mcrledit.
- **Default cursors**: no macro window carries registry flag 0x10000, so `FUN_10118870` restores the cursor the window
  last had (`FUN_10118830` saves it at every hide or close) and clamps below 1 to 1 — child 1 on the first open of the
  session. mcrmenu, mcrselec, mcrselop, mcrbedit, mcrledit and "macro" (child 1 = Prev) all follow that.
- **mcr1edit / mcr2edit override it**: their open and their focus hook vt+0x24 put the cursor on **column + 1**
  (`FUN_101a44b0`), so the edit palettes always come up on the remembered Ctrl / Alt column, and a hover switch
  carries the column across.

Ours opened mcrselec, mcrselop, mcrbedit, mcrledit and the editor on a fixed child and reset the palettes' columns
every time. The macro windows now go through `cursor_recall` / `cursor_keep` like the other windows, and
`World::macro_palette` keeps E's focused palette and both columns across opens. The hover switch between palettes was
already there; hovering mcrbedit or mcrledit now acts as Num+.
