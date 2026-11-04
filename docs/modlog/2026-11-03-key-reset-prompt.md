## Config: the key-reset prompt in comyn (2026-11-03)

- conf6win's Default (reset keyboard assignment) asked in 39542's "menu    yesno" with the question in a frame above
  it *(inf)*. Retail (checked in Steven's unpacked FFXiMain.dll, sha256 8097dbf8..., 2026-11-03) opens
  "menu    comyn" through FUN_10210e30 on DAT_10664520: group 0xb 0x8f "Reset keyboard assignment to default?",
  default No, Escape answers No, at comyn's own layout place; Yes resets both k1assign and k2assign (0x4a..0x89).
- Ours: the config stack's page is now `comyn` (configui2.rs), the question inside at (+4, +4) wrapped to w - 8
  (FUN_10211050). Not checked on screen (no game data in the cloud container).
