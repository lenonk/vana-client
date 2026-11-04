## Select screen: Escape in the prompts (2026-11-03)

- Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03): the lobby's yes / no is its own
  window "menu    ptc6yesn" (DAT_10669004, handler FUN_1023d050), not comyn. Escape (0x1023d14c..0x1023d16e)
  answers No only while +0xa5e1 is set; every opener clears it and only FUN_10164580 sets it, when +0xa5e0 == 1,
  which only FUN_10243f10 gives (types 0xb / 0xc, the PlayOnline settings-file prompts). So the log-in (0xd),
  rename (5), delete (4) and creator prompts ignore Escape. hnyesno (FUN_1023f700) gives -1 with SE 0003;
  ptc9dele (FUN_1023d2e0) has no Escape case.
- Ours: select.rs answers No on Escape for `Prompt::Settings` (before, never outside the modern layer).
- "Connecting to the server..." (ours, the status line while connecting) has no retail source: not in the DLL,
  FFXi.dll, FFXiResource.dll, polboot.exe, 39541 or any ROM DAT. The nearest retail line is group 8 id 3
  "Connecting to lobby server." (lobby step 1, FUN_100ed7e0 at 0x100edada). Left as is.
