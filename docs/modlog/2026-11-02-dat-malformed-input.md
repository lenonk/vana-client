## DAT parsers: malformed files no longer crash (ours, 2026-11-02)

- A corrupt DAT (from the install or a mod pack) could panic or abort the client (issue #87, found by fuzzing).
  Fixed in event::parse (a block count past the size table reserved up to 77 GB), zone::parse_mmb (offset table
  read past the end), zone::parse_mzb (short header, records past the end), zone::light_list (read before its
  length check), zone::parse_rid (a bad count looped up to u32::MAX times), character::parse_skeleton (short
  chunk), title::parse_ops (ops too short for their argument now come out as Other), title::Title::parse (camera
  key count past the chunk), ui::parse_sprite_lib (sprite count cut), sched::loops (an offset below 0x10 wrapped)
  and the counts that sized allocations in autotrans, dialog and names (d_msg). zone::decode_mzb and
  zone::decode_mmb need their 8-byte header. A camera path with no keys (title flyover, eventdump, the title dump) is skipped instead of sampled.
- Each fix only changes what happens on input that used to crash; files that parsed before parse the same.
- dat/fuzz.rs runs about 50 parsers on 4000 random and chunk-shaped inputs each (plus each chunk's payload) and
  fails if any panics or aborts. New parsers should be added to its list.
