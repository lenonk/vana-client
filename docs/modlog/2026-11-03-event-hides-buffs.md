## Events: op 0x46 sub 1 hides the status icons (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch5.md item 5): op 0x46 sub 1 (FUN_100b6440 at
  0x100b652f) also calls FUN_10221c30 on "menu    buff" (DAT_106671b8): window +0x69 / +0x6a = 0 (not drawn), every
  item disabled, +0x62 = 0 (no cursor), +0x5c |= 2 (changes kept for later). Sub 0 doesn't undo it. The event end
  wrapper FUN_100ae960 (0x100ae98b) shows it again through FUN_10221c70, rebuilding the icons if anything changed.
- Ours: event_vm emits Effect::HideBuffs after sub 1's camera flag. client/event.rs sets World::buffs_hidden and drops
  the buff focus (log focus 2). The key 0x78 focus cycle skips the buff list while hidden, and buffs::draw draws
  nothing. finish() clears it. Our icons come from the live 0x037 / 0x063 state, so nothing needs a rebuild.
- The party icon object (DAT_106671bc) is a separate window and stays up. The modern status panel is ours and is
  left as it was.
- This answers MODLOG L7359: retail hides the buff window in a cutscene only when the script runs 0x46 sub 1, or
  when cinema mode hides everything.
