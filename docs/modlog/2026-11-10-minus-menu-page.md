## Main menu: '-' turns the page, key function 0x18's bindings (2026-11-10)

Issue lenonk/vana-client#171. Bindings read from Steven's FFXiMain.dll (30261102) through `input::Input::load`.

- On retail '-' opens the main menu on the page it was closed on and, pressed again with the menu up, turns to the
  other page and back; it never closes the menu (the issue's report of retail). Each page comes back on the row it
  last had, as its window's cursor *(inf: the two pages are two windows, each with its cursor memory, as
  `menu_mem` already models)*. The modern Command Menu has one page: there '-' still closes it (ours).
- Key function 0x18 (context mask 0x0004): slot 0 = 0x100c (flag 0x10: Ctrl or Alt held, not in Key Assignment;
  DIK 0x0c, the main row's '-'), slot 1 = 0x804a (flag 0x80: no Ctrl / Alt; DIK 0x4a, the numpad's '-'). Slot 0 is
  Key Assignment's k2 row 8, rewritten from config 0x72 (default 0x0c). Compact record 35 = {key 0x0c, rule 7 (any
  modifiers), states 4, bits 2}, and pad bit 2 is 0x18's: on Compact 1 / 2 the main row's '-' fires it too.
- So on a Full keyboard the bare main-row '-' opens nothing. egui maps both minus keys to Key::Minus; the app notes
  which one the last press was (window, gamepad menu slot, mods' keys), and test drives count as the
  numpad's. Ctrl + '-' is taken by egui's zoom before the game sees it (ours, unchanged).
