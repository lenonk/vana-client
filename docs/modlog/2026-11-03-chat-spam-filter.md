## Misc. 4: Chat filter setting (2026-11-03)

- Config 0xce (Misc. 4 chat filter: 1 hide, 2 replace, 0 off) was saved but never read. Retail (FFXiMain.dll,
  checked 2026-11-03): turning it on (and login, as the callback runs on registration) loads prohibited.txt and
  allowed.txt from the game folder (FUN_1019bec0 -> FUN_101bd820 -> FUN_101bd880); without prohibited.txt it
  prints group 9 0x15a and sets the option back to 0. Shouts, yells and assist lines (s2c 0x017 kinds 1, 0x1a, 0x22,
  0x23) from non-GMs are checked against the sender's name and the plain text (FUN_100da5e0 -> FUN_101bcf80): an
  allowed.txt hit passes, a prohibited.txt hit hides the line (1) or swaps its text for group 9 0x159 (2).
- Ours: client/spamfilter.rs ports the file parser ("(isn)" prefixes, "\" escapes, [..] / [^..] / [a-z] / [.]
  patterns, folding of case, width and half-width katakana) and the matcher (FUN_101bd510). Its results were
  checked against the retail functions run under an x86 emulator (about 1,500 random files and lines, all equal);
  the unit test keeps a sample.
- White space above U+00FF uses Rust's classes in place of Windows' GetStringTypeW; the "replaced" line's extra
  flag (LAB_100e37a0 local 0x18) is not used.
