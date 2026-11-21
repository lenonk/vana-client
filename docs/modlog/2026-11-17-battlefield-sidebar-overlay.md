<!-- dll-build: 30261102_0 -->
## Zone 0x11e: the battlefield sidebar and map overlay (2026-11-17)

Follow-up to "Zone-in: the handler walk's c2s 0x0DB, 0x112 and 0x05A; s2c 0x072": the readers of s2c 0x072's two
blocks and the c2s 0x112 kind 2 send. All of them are gated to zone 0x11e, which has no area name in this client.

- Sidebar: the compass draw FUN_102219c0 (ECX = the compass struct DAT_1066803c) calls FUN_10220a20 first, then the
  objective bars (FUN_102212f0). FUN_10220a20: our entity (FUN_101cdac0) with +0x1d0 bit 10 (team byte +0x1ff in 2..99,
  FUN_1008e290); +0x1ff 0x28..0x2b draws nothing; 8 / 9 draws nothing outside zone 0x11e (session +0x3c400), and in
  it FUN_10220f60 then FUN_10220a90; any other team byte FUN_10220a90 alone (the Conflict sidebar, not built).
- FUN_10220f60 at FUN_10220970's rect (our objective anchor: x 16, the bottom of cnttime / casttime / gaugewin /
  titlewin), y = its bottom:
  - y + 4, FUN_102210a0: glyph 0xa9 (sprite font, white); FUN_101cc660 at (+0x1a, +7) of record 1's first field
    (FUN_101f8810(1)); "Base" (0x10386fdc) at (+0x14, 0) in name colour 0x13 (DAT_104e2d9c = table 0x104e2d50 [0x13]);
    FUN_101cc800 at (+0x24, +0x11) of FUN_101f8870(1) = record 1's third field x 100 / 15000.
  - y + 0x20, FUN_10221190: FUN_101ccb20 at (+0x10, +7) of FUN_101f87d0(1, 2) = record 1's second field x 100 / (both
    sides' second fields); glyph 0xa9 at (0, 0), glyph 0xab at (+0x50, 0).
  - y + 0x34: as y + 4 for record 2, glyph 0xab, name colour 0x12 (DAT_104e2d98).
  - y + 0x52, FUN_10221120: glyph 0xaa, then "x %02d" (0x10386fe4) of the block's 3-bit value at +0x10.
- The gauges: FUN_101cc660 -> FUN_101cc5d0 -> FUN_101cc4d0 with windowps 97 / 38 (lib 1 +0x184 / +0x98), FUN_101cc800
  with 98 / 39 (+0x188 / +0x9c) and the fill 0.7 wide. FUN_101cc4d0(v, &st): equal, the fill to v; else the fill to
  min and a ghost to max; then st += trunc((v - st) x 0.15) (0x1032be5c), or st = v when that is 0. FUN_101cc2d0: out of
  [0, 1] counts as 0; the fill (blend 8) when lo x 100 + 0.5 truncates to non-zero; the ghost (0x30808080) at x + 4 +
  trunc(fill width x lo), (hi - lo) x sx wide; the frame 0x30505050 while lo is 0. FUN_101ccb20 -> FUN_101cca00 ->
  FUN_101cc850: f = min(v, st) x 0.01; 0x80001080 to f, 0x80801000 for 1 - f (each clamped and rounded as above),
  frame 38 in 0x80808080. The state bytes are the compass struct's +1, +2, +4, +5, +6, zeroed once by FUN_102204c0
  (FUN_10010d20 -> FUN_10222db0 -> FUN_10220340).
- Map overlay: win_mapframe_v10 = the base draw, FUN_101fbfd0 (the player arrow, the grid text), FUN_101fc130.
  FUN_101fc130 runs while FUN_101fc0c0 holds: the map's record (+0xfc >= 0), its zone (+0x100) 0x11e, titlewin open
  (DAT_10579638 +8) and the player's record set (+0x98, FUN_101fa4b0). FUN_101fc150(i) for i 0..9 while the overlay
  block is valid: (x, z) = the record's third and fourth fields to map pixels (FUN_101f8b80 = our Region::px) and to
  the window (FUN_101f11d0: (px - +0x6c - +0x24) x +0x58 + 0.5, truncated); keytops3 150 (lib 2, table 0x10383240
  [1]) at (-6, -6), blend 0x48, colour by the second field (table 0x1033b904: 0x80202020, 0x80001080, 0x80801000; 3 or
  more the first); "#%02d" of i at (+10, -2); a second field other than 0 adds FUN_101fc270 at (-13, +10): windowps 97
  at (+2, +1) scaled (min(first field x 0.6 x 0.01, 1), 0.72) in that colour (blend 8), windowps 38 scaled (0.6, 0.8)
  in 0x80404040 (blend 0x48).
- Kind 2: FUN_101fb060 (every mapframe setup: main menu, /map, an event map or illustration, Change Map, the preview,
  the Primer) ends with FUN_101fc100: FUN_101fbc90(.., 2) while FUN_101fc0c0 holds. FUN_10098100 sends only while the
  sidebar callback is clear, so in zone 0x11e it goes once the zone-in's kind 0 has been answered.
- Ours: battlefield.rs (draw_sidebar, draw_overlay, map_set_up, the getters), MapWin::battlefield / title_up set
  each frame, World::title_up (GameUi::was_up: the last walk's titlewin, read without opening it). The gauges ease
  only on frames the sidebar is drawn, as retail's draw does: one step at the first draw, then 30 a second, and a
  frame without the sidebar drops the time since (Battlefield::frame / ease).
- Not built: FUN_10220a90 (the Conflict sidebar of the entity's +0x27c..+0x298, which we do not parse).
- Not matched exactly: the sidebar draws in the classic UI only, with the objective bars (the modern layer has no
  panel for it); the gauge
  bytes start at 0 with each World, retail's once a run; FUN_101f87d0 with both second fields 0 is an integer divide
  fault in retail, 0 in ours; titlewin's "open" is our window walk's (last frame's) view.
- Tests: battlefield.rs (getters, the ease step and its per-draw timing, the gates, kind 2).
