<!-- dll-build: 30261102_0 -->
## Zone-in: the handler walk's c2s 0x0DB, 0x112 and 0x05A; s2c 0x072 (2026-11-17)

- Retail captures (7 sessions, 62 zone-ins, client 30261102_0): after c2s 0x00C, 0x061, 0x118, 0x11B, 0x053, 0x0E0
  and 0x00F, every zone-in sends c2s 0x0DB kind 1 (mask 0x02), 0x112 kind 0 (`00 00 00 00`) and 0x05A (no body), in
  that order and in the same frame. Ours sent none of the three at a zone-in.
- Where they come from: FUN_10160530 (each frame) sets +0xac when game mode becomes 0x60 and then, with the session
  and DAT_1047e5a0 (set by FUN_10093db0 with the zone-in's 0x00C and 0x061) present, runs FUN_101604c0: every
  registered handler's vtable slot 0x30, in registry order (0x10373320, 0x2c a row). Three send: prty5 (row 94, also
  prty5us / fr / de, one handler DAT_10665574) FUN_10207070, mapframe (row 204) win_mapframe_v30, cnqframe (row 210)
  win_cnqframe_v30. netstat's 0x0E0 / 0x00F come from its open slot (FUN_10202c50 -> FUN_100e5900), before.
- prty5 FUN_10207070: while its +0x30 is clear, +0x2c = config 0x98, or 1 << table 0x10386450 [client language x 5]
  when 0; FUN_100e4980 -> FUN_100e46a0(1, mask) (c2s 0x0DB: @6 kind, @8..@0x14 the three config words, @0x24 mask);
  +0x30 = 1. Slot 0x2c (FUN_10203120) clears +0x30 from the zone teardown's walk FUN_10160470(1) (FUN_10093b70), so it
  sends once a zone. The window's open (slot 1, FUN_102070c0) sends nothing; rows toggle +0x2c (FUN_102071b0); the
  close (FUN_102070e0) saves and sends when +0x2c differs from config 0x98. Ours sent on every open: now the zone-in
  sends, World::party_langs is +0x2c, and the open reads it.
- mapframe: FUN_101fbc90(zone, map, 0) -> FUN_10098080: c2s 0x112 {kind 0} (no gate), callbacks DAT_1048904c =
  FUN_101fbd00 and DAT_10489050 = FUN_101fbdd0, both blocks' valid flags (DAT_10664960 / 61) cleared. Kind 1
  (FUN_100980c0) and kind 2 (FUN_10098100) send only while DAT_1048904c is clear (kind 2 checks the sidebar's callback
  though it sets the overlay's). FUN_101fbc90 is called with 0 (win_mapframe_v30) and 2 (FUN_101fc100 from the map's
  open FUN_101fb060, only for a map of zone 0x11e, FUN_101fc0c0); kind 1 has no caller.
- s2c 0x072 (FUN_100999a0, registered in FUN_10094190 next to 0x05E / 0x071): Kind @4; 0 runs and clears
  DAT_1048904c over @8, 1 runs and clears DAT_10489050. FUN_101fbd00: words a @8, b @0xc, c @0x10; records from
  0x10664964 (4 words each): 0 zero, 1 = b split 7 / 10 / 14 / 1 bits, 2 = a split the same; c bit 0 and bits 1..3.
  FUN_101fbdd0: 10 records from 0x1066499c: byte @8 + i, then the word @0x14 + 4i's bits 0..1, bits 2..16 and 17..31
  (both signed); not valid, its getters give 0, 3, -1, -1. The zone load FUN_10093720 clears both callbacks.
- Readers: the sidebar's getters (FUN_101f8810 / 8870 / 87d0 / 87b0) only from FUN_10220f60 (FUN_10220a20: session
  +0x3c400 == 0x11e, and FUN_101cdac0's object with +0x1d0 bit 10 set and +0x1ff 8..9), the overlay's (FUN_101f88c0..8930) only from
  FUN_101fc150 (win_mapframe_v10 -> FUN_101fc130, a map of zone 0x11e). Zone 0x11e has no area name in this client
  (file 55466 entry 0x11e empty) and the captures hold no s2c 0x072. Ours: client/battlefield.rs keeps the callbacks,
  the gates and both blocks as retail does; the two zone 0x11e readers and the kind 2 send followed (the entry
  "Zone 0x11e: the battlefield sidebar and map overlay").
- cnqframe win_cnqframe_v30: _DAT_10664744 = 0, then FUN_101e9600 (FUN_101e8030 in 30261006_0): the 120 s gate always
  passes, and FUN_10098010 sends c2s 0x05A unless DAT_10489044 is set, which the zone load cleared. s2c 0x05E
  (FUN_100998a0) only runs that callback (FUN_101e96c0), so the zone-in's reply now fills the conquest data; a 0x05E
  nothing asked for is still dropped, as retail does. FUN_10093720 does not clear the colonization callback
  (DAT_10489048); the 0x071 mode 2 callback (session +0x3c200) goes with the session block. Ours: region::Data
  zone_in / zone_load / take.
- Ours: client/mod.rs `zone_walk` right after seacom::zone_in (0x0E0, 0x00F) at our zone-in.
- Not replicated: the one c2s 0x0D8 a session in the captures (first event end, tabula 0, slot 0, bytes that change
  per session). FUN_100ae3f0 (event end) calls FUN_101fe5c0 with ECX = prvdungp's handler DAT_10664c64 (0x118 bytes,
  operator_new in menu_comyn_fn; FUN_101fe610 sets only +8..+0xe and +0x24..+0x2a). Its +0x14 / +0x16 / +0x17 / +0x18 /
  +0x30 / +0x3c / +0x58 are heap garbage until the tablet window first opens, and the send happens when that garbage
  passes the checks; FUN_101fec10 then copies its encoding to +0x3c, so later event ends match and send nothing.
- The existing sends in retail's order too: ours sent 0x053 (equipsets::field_entry) before 0x118 and 0x061
  (StatusRequest) after 0x11B; now 0x061 (FUN_10093db0, after its 0x00C), 0x118 / 0x11B (FUN_10095650), 0x053.
- Open: retail sends the zone-in's batch (0x00C on) when the zone is loaded (FUN_10093db0, about 2 s after the
  0x00A); ours sends it at the 0x00A.
- Tests: battlefield.rs (sends, gates, both replies, dropped replies), region.rs (zone-in ask, one request out, the
  0x05E drop, colonization kept), partymenu.rs (the captured 0x0DB byte for byte), mod.rs (the walk's order).
