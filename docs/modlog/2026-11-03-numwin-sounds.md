## Number windows: no sound on the arrows (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03); notes in
`docs/re/notes/2026-11-03-modlog-gaps-batch3.md` §8.
- Only two digit-entry windows exist: "menu    itemctrl" (quantity, OnKey FUN_1013a6c0) and "menu    moneyctr"
  (gil, FUN_10140bc0). Every quantity or amount screen reuses them (delivery, hand-over, trade, recycle bin,
  bazaar, guild and NPC shops, auction).
- Neither OnKey calls a sound helper: Up / Down / Left / Right and hitting a limit are silent. The generic key layer
  FUN_10118260 plays OK 0002 and cancel 0003; its cursor sound 0001 never fires because the number item has no
  neighbours. The openers play their own 0002 when the window opens.
- Ours played the cursor sound on every arrow in both windows (delivery, hand-over, trade, shops, auction, storage
  and the synthesis quantity); now silent. The synthesis repeat count (ours) keeps its sounds.
