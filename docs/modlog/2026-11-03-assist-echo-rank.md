## Chat: the assist echoes' rank icon (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch7.md item 21): only the Assist channel echoes have
  it. /assistj (FUN_10078be0, "%c%c%s(J) : %s") and /assiste (FUN_10078d80, "(E)") put 0xEF R before our name when
  our mentor rank (G+0x40deb, s2c 0x01B @0x65) and mastery rank (G+0x40dec, @0x66) are both non-zero and MentorFlg
  (s2c 0x0B4 byte 7 bit 1) is on. R = mentor x 10 + mastery + 0x24, or 0x2f outside 0x2f..0x4c. Otherwise the plain
  "%s(J) : %s" is used. Other echoes have no icon.
- Ours: client/textcmd.rs adds it to the two assist echoes. The R formula is now client/mod.rs rank_icon, shared with
  the received 0x017 lines.
