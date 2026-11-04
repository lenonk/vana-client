## Gameplay Auto-sort: sorts and stacks new items (2026-11-03)

- Config 0x9a (Gameplay > Auto-sort) was saved but never acted on, so a new item never sorted the inventory and
  partial stacks of it never merged (with either Sort Type). Retail (FFXiMain.dll, checked 2026-11-03):
  - FUN_101978e0: 0x9a != 0 sets +5 on the inventory's list record only (DAT_10666e2c); 0xca == 1 sets
    DAT_10666e74 (Sort Type 2).
  - FUN_1021c4c0, each frame in the field (FUN_1015fa00, state 0x60 / 0x70): the free slots in 1..size
    (FUN_100eb8d0) against the last count (+8). With +5, a fall (an item took a new slot) and no hold (+6) calls
    FUN_1021c7c0(0, 1, 0): the view by Sort Type (FUN_1021c7e0) and c2s 0x03A {0} (FUN_100ebf40). Any change clears
    +6; the count is kept either way.
  - +6 is set by Manual Sort's split of one off a stack (FUN_101f0b30 at 0x101f0c95; FUN_101dbed0 at 0x101dc0a0,
    inventory only), so the split's new slot doesn't sort.
  - Both Sort Types send the same 0x03A; the stacking is the server's (LSB GP_CLI_COMMAND_ITEM_STACK merges
    partial stacks of the same item). Type 2 only changes the view: copies of an item gathered beside the first.
- Ours: items::retail_auto_sort. The count starts once the inventory settles (s2c 0x01D state 1 with the
  inventory's loaded bit) after login or a zone-in, since our zone-in empties the slots and LSB resends them.
  An Auto-sort within 1 s of the last inventory 0x03A waits out the second: LSB drops a 0x03A sent sooner and logs
  the player out at the 4th dropped (map.LIGHTLUGGAGE_BLOCK). The kept-sorted Auto of the sort window (ours,
  2026-10-25) no longer also queues the inventory while config 0x9a is on.
