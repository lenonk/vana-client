## Item lists: blue names for charged gear ready to use (2026-11-03)

- FUN_100fbc70 (FUN_100febe0 in Steven's unpacked FFXiMain.dll, sha256 8097dbf8...; checked 2026-11-03) reads the
  extdata only, never the item id: byte 0 == 1 (the charged / enchanted item layout, use times at +4 and +8) and
  byte 3 bit 0x40 (ready to use; Windower calls it "usable"). The main item list (FUN_101e8700) draws an equipped
  weapon or armour that passes it blue 0x80407090 instead of green 0x80509050.
- Ours: `Inventory::slots_of` sets `Slot::equipped_ex` from it (`items::charged_ready`, also used by the direct
  list's FUN_101f0810 rule, which tested the same bit as u16 @2 & 0x4000). Test items::charged_tests.
