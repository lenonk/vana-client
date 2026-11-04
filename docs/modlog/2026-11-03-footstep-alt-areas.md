## Footsteps: areas 704..706 give surface 3 surface 2's sound (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 72): the alt-name flag is scene +0x1cf.
  FUN_1018b500(area) sets it only for areas 704..706 (area - 0x1bc in 0x104..0x106), and only when the session flag
  word S+0x3c35c has bit 0x100 clear. With it set, a step on surface 3 is named "02.." (surface 2's sound).
- Ours: client/footsteps.rs swaps the name's surface byte 3 -> 2 while the area shown (op 0x34's, else the zone) is
  704..706. The dust and footprints keep surface 3. The session flag isn't known to us *(inf: taken as clear)*.
- Checked against the same note:
  - A failed fses / fser lookup already ran again on the next step (`Effects::foot_set` caches only a hit).
- Open:
  - The culled-pass skip: 2 passes with no step after a culled pass.
  - The note finds no model-scope search in this build, but ours still falls back to the model file's folder and
    then DAT 0's 'foot' (MODLOG "Footsteps"). Left as is until checked live.
