## Chat: every Shift-JIS byte reaches the font as the server sent it (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 76) never converts chat. The bytes go
  from the packet to the log to the screen, and each two-byte pair indexes the font through 0x1011c940 (our
  `sjis_index`, which already matched it, unknown rows included: JIS ≥ 0x9320 gives glyph 0xcb, ＝).
- Ours turns text into a String (`decode_sjis`) and back (`encode_sjis`) before drawing. The quotes 0x8167 / 0x8168
  already came back as themselves (glyphs 135 / 136), so the old '?' note was out of date. But 398 pairs did not:
  - NEC row 13 duplicates (0x8790.. came back as 0x81xx);
  - the NEC-selected IBM rows 0xED / 0xEE (came back as 0xFA..0xFC, which draw ＝);
  - 0xEEDC.., which share glyph indices with the usgaiji codes (came back as 0xEF1F..).
  Single bytes Shift-JIS has no character for (0x80, 0xA0, 0xFD..0xFF) became '?'.
- Now `decode_sjis` keeps any pair whose encoding would differ, as private-use U+F0000 + its bytes, and any such
  byte as U+F1000 + the byte (clear of the chat icons at U+F0000..U+F0FFF), and only 0xEF pairs are read as usgaiji. So decode then encode gives the server's bytes back for every byte
  and pair, which is what keeping the bytes end to end does. Test `sjis_bytes_survive`.
- Left as is: text we type is Unicode and still becomes '?' where Shift-JIS has no character (retail's input line is
  Shift-JIS already).
