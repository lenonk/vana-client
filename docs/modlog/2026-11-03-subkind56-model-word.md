## 0x00E SubKind 5 / 6: the model word's low 12 bits (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch2.md §5): SubKind 5 / 6 store u16 @0x32 | 0x9000 in look slot 9;
  bits 0..11 are the model id (always the NPC table, FUN_100c51c0(0, id)) and bits 12..15 the look slot
  (FUN_100d0b50), 9 = the whole model.
- Ours (`dat::npc::parse_look`) took the whole word as the model id, so a word with any of bits 12..15 set named a
  wrong file. It now keeps the low 12 bits. SubKind 0 is unchanged. Test `looks`.
- Not done: SubKind 6's +0xf6 = 2 for raw words 0x7b8..0x7cb / 0x7d3..0x7da (not traced); slot 9 attached at +0x864
  over a race or a slot 0 base (event op 0xb6 sub 0x0c, kind 7).
