## Coded chat lines: message source 9 (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 16): FUN_100964a0's table entry 9 is
  FUN_10096440 -> FUN_100bd760 (the expander singleton) -> FUN_100c1620: the record cache at +0x858, language slot
  0x22 (en file 55693, the trust lines; 12,201 entries), the id sign-extended. An id outside the table gives nothing
  and the line is dropped.
- Ours dropped source 9. client/event.rs source_text now reads it from the same slot 0x22 table grammar kind 0x88
  uses (BattleText::grammar_line).
