## Missions: the held-key header stop (2026-11-03)

miss00's OnKey (`FUN_1022e760`, the Up / Down path at 0x1022e99c..0x1022ea19) classifies the row the cursor landed on
with `FUN_1022df70`: -1 is a header, -2 a blank row. The two cases differ by whether the key is a held repeat
([esp+0x18]):

- **Held repeat on a header** (0x1022e9f4): `FUN_10222150(1 /* Down */, ...)` takes the move back by one row and the
  paged-list base's remembered direction **+0x60 = 2** (Up). In the base, +0x60 == 2 at the top edge limits the move
  instead of wrapping through the count, and +0x60 == 1 does the mirror at the bottom. So a held Up that reaches the
  "Current:" header sits on the row under it and stays there.
- **Fresh press on a header, or a blank row**: the move repeats in the same direction (skip over it) and +0x60 = 0, so
  a fresh Up from the first row still wraps to the last.

SE 0001 plays only when the cursor ended on another row, which ours already did. `List.dir` is the remembered
direction, and `World::key_repeat` carries the repeat flag into the handler the way `key_shift` carries Shift.
*(inf: the edge test is modelled as "no wrap"; retail moves by one page there)*
