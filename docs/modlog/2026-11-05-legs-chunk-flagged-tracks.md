## Motions: flagged tracks in a legs chunk still write the bone (2026-11-05)

- `FUN_10019b30` (one layer's tracks onto the pose, through `FUN_10019a50` from the layer's vtable +0x38,
  `FUN_1001ab60`): when a track's rotation index has bit 31 set, a chunk of region 1..4 sets the bone to identity
  and leaves it unmarked, so another region or layer can still write it. A region 0 (legs) chunk masks the flag
  off and uses the track like any other, constants included, and marks the bone as written.
- So a gesture's legs chunk resets the bones it flags. The Taru event motions (file 32752: th10, wav0, tlk0, poi0, ...)
  flag bones 0, 2, 72, 73, 77 and 82, and the Taru idle `idl0` keys bone 2 with a hip sway. We dropped every
  flagged track, so the idle under a gesture kept moving bone 2, and the NPC's feet slid about 3 cm on the
  idle's 2-second cycle (Kupipi's trust intro, Wetata). Retail footage of the same scene shows the feet still.
- Client: `parse_animation` keeps flagged tracks in chunks whose name ends in `0`. It masks bit 31 off every
  channel index there, and still skips flagged tracks in other regions. Test:
  `legs_chunk_flagged_tracks_cover_the_idle`.
