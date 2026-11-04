## Zone ambient loop: crossfade at a change (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03); notes in
`docs/re/notes/2026-11-03-modlog-gaps-batch3.md` §7.
- FUN_10036170 starts a new ambient resource without stopping the old one. The sound engine (FUN_1024a7c0 ->
  FUN_1024a1b0) fades every other playing flag-0x20 voice to level table[1] over 0x96 sound steps and keys it off,
  while the new voice ramps in over the same 150 steps (about 2.5 s). So a time of day, weather or indoor change
  crossfades.
- FUN_10036170(0) keys the voice off at once. It comes from the zone's SeSep release (FUN_10037ac0) at a zone
  change, and when a weather has no ambient: a hard cut, then the new zone's loop fades in.
- Ours: the old loop now fades out over 150 steps while the new one fades in (two voice handles take turns). A
  scene change or no loop still cuts at once. A loop still fading from a change moments before is cut when the next
  change comes (retail lets it finish).
