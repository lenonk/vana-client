## Item lists and storage: the equipped linkshell's row colour (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03).
- FUN_100e9730(n), n = 1 / 2: linkshell n is equipped from slot = char byte +0x9808+n, bag = +0x980f+n, valid
  while that slot holds an item. Only the s2c 0x0E0 handler FUN_100ea780 writes them (@4 n, @5 slot, @6 bag).
- Item lists (FUN_101f06e0, MODLOG's FUN_101e8700), first match: dimmed grey; equipped charged gear ready blue;
  equipped green; **the equipped linkshell / linkshell 2 from this slot of this list's bag green**; usable /
  crystal yellow; white. Ours: `items::list_rows` (was never green).
- Storage (FUN_101db220) rule 7 (asm 0x101db46a..0x101db4e6): a row from bags 0 / 5 / 6 / 7 going to anything but
  a wardrobe is **grey** (it may not move) when it is the equipped linkshell / linkshell 2. Not green: the green
  rule is only in the item lists. Ours: `items::judge` (with a modern explanation line). The Manual-sort colours
  (FUN_101dbdb0) have no linkshell rule.
- Not checked on screen (no game data in the cloud).
