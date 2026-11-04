## Chat: received lines cut at a control byte (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 20): FUN_1018fc40 walks the formatted
  line by characters (FUN_1018fa80) and stops at the first byte FUN_1018fa20 rates 2 or 3: 0x1E, 0x1F, 0xEF, 0x7F not
  followed by 0xFB / 0xFC, and any other byte below 0x20 except NUL, LF and CR. 0xFD auto-translate blocks are kept
  whole. Every s2c 0x017 kind gets it except the assist kinds 0x22 / 0x23, whose lines carry an icon; those cut only
  the name and the text.
- Ours: dat/font.rs chat_cut does the walk on the raw Shift-JIS bytes. client/net.rs chat_text cuts 0x017's name and
  text before decoding. Because retail cuts the whole formatted line, a cut in the name drops the text too, except
  on 0x22 / 0x23.
- Not done: MODLOG also counts the filtered shout / yell as an icon line that sanitises only the name and the text.
  Which packet and Kind those arrive on is not traced yet, so only the assist kinds 0x22 / 0x23 are treated that way.
- The cut only drops the text where the formatted line holds the name: the system kinds 6 / 7 and the NS kinds print
  the text alone, so a control byte in their unused name cannot reach the line. Kind 0x19 picks its format from the
  name the packet sent, so a name that is only a control byte still formats with it and loses the text.
- Inferred: a 0x7F 0xFB / 0xFC pair is stepped over as two bytes. FUN_1018fa80's length for it was not read.
