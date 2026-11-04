## Event op 0xc2: party members in their Mog House (2026-11-02)

- Op 0xc2 (FUN_100bbf80, from the dispatcher table at 0x100bc960) feeds the residential-area NPCs' "visit a
  party member's Mog House" choice (zones 230..241, 245, 256, 257). Sub 1 stores FUN_100e9850's mask: bit i for each
  roster record (G+0x57d8, 20 x 0x70) that is listed, not ours, and has +0x1a set. Sub 2 stores FUN_100e98b0's
  answer for display slot val@2 (1..19, the order table G+0x9534): that member's id under the same test, else 0.
  Other subs only advance 2. Before this, both stored nothing, so the scripts hid every member from the choice.
- Record +0x1a is set by the 0x0DD / 0x0E2 handlers (LAB_100ea390 / 100ea560): MoghouseFlg (+0x1b) set gives
  ZoneNo, or the current zone (G+0x3c350) when ZoneNo is 0; clear gives 0. `ListUpdate.moghouse` and
  `Member.mog_zone` carry it (client/party.rs). LSB sends the flag (PARTY_MOGHOUSE).
- Op 0x9e (FUN_100ba8a0) only sets _DAT_104898fc = (u8@1 == 0). Nothing in the DLL reads that word (writes only: here,
  FUN_10094190 and the event resets in FUN_100ae3f0 / 100aed00), so ours rightly does nothing with it.
