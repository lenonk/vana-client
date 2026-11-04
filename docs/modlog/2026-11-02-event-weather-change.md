## Event op 0xae sub 0: weather change (2026-11-02)

- Op 0xae sub 0 (FUN_100bb620, from the dispatcher table at 0x100bc960; the old FUN_100b9790 name was off) calls
  FUN_10094040(val@2, 0, val@4): weather val@2 through FUN_101874f0, blended in over val@4 x 60 clock units
  (0x1009405d..66; the clock runs 3600 units a Vana minute, 1500 a second, so val@4 / 25 s), the same call the
  s2c 0x057 path makes. It does not touch the server's number (DAT_10485fe0), and the event's end does not undo it;
  a later 0x057 or op 0x78 (FUN_100940f0, back to DAT_10485fe0) does. 57 uses in 11 zones (zones 9, 10, 17, 19,
  21, 70, 79, 85, 90, 210, 277).
- Ours: the weather chain and the weather effects as for 0x057, `w.weather` unchanged. Op 0x78 now also puts the
  chain back to the server's weather at once when it differs, as FUN_100940f0 does whatever set it.
