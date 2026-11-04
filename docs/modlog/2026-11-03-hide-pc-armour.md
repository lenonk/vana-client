## Misc.: Hide PC armour (2026-11-03)

- Config 0xa5 (Misc. > PC armour, `/hidearmor`) was saved but never read. Retail (FFXiMain.dll, checked
  2026-11-03): FUN_1009afe0, the look of a PC (0x00D look mask), with 0xa5 != 0 and the actor not ours
  (DAT_10485f7a), not a client-made model in the slot (+0x78 high byte, set by FUN_100ab750 for the menu preview
  FUN_101cd3b0 and the lobby's FUN_10242cd0, never a server actor's 0x00D) and not a model look (+0xee): head,
  body, hands, legs and feet become (slot << 12) | table 0x1032d678 (0, 8, 8, 8, 8); the face, main, sub and
  ranged stay from the packet. Nothing re-reads the looks on a change (FUN_10080350 only sets the id and prints
  line 0x100), so it shows as each PC's next look arrives.
- Ours: client::hide_armour on the 0x00D look of every PC but us, for the model only; the entity's appearance
  (and the Lua look) keeps the packet's gear.
