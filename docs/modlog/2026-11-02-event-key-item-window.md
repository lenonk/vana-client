## Event op 0xcc 0x20: the Key Items window on one key item (2026-11-02)

- Op 0xcc sub 0x20 (FUN_100b9d90, case at 0x100b9fb6) reads val@2 and advances 4 bytes. With a key item number it
  calls FUN_1022a640 on the evitem object (DAT_106671d4): event mode on (+0x89 = 1), then FUN_1022a660 stores the
  number (+0x8e), opens "menu    evitem" and goes straight to the description state (FUN_1022a7b0: +0x90 = 3).
  With 0 it calls FUN_1022a6d0: event mode off, the number cleared, the window closed.
- In event mode FUN_1022aaa0's description state ignores Enter and Escape, so only the script closes it. The event's
  end closes it too (FUN_100ae3f0 calls FUN_1022a6d0 at 0x100ae926), whoever opened it.
- Ours ignored the sub. Now it opens our evitem on that key item's name and description, the keys and a click leave
  it up, and 0 or the event's end closes it. 10 zones use it (130, 154, 165, 230, 235, 241, 245, 257, 280, 291), e.g.
  zone 257's `cc 20 #2842`, a message, then `cc 20 #0`.
