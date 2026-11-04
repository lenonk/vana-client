## Event op 0x5c 0xb0 / 0xb1: the music stream's state and its restart (2026-11-02)

- FUN_100b6e30 0xb0 (case at 0x100b6f14): set@2 = FUN_10246710(0, -1), which is 1 while the BGM stream has a track
  (+0x94 >= 0) and 0 when it has none, or -1 with no stream player (DAT_1046393c). 4 bytes, no frame end.
- 0xb1 (0x100b6f3c): when the stream has no track and DAT_10463940 is set, FUN_100378b0(DAT_10463940) starts that
  track again. 2 bytes. DAT_10463940 is the track the music change task last started (0x1003748e / 0x100376ce, from
  its +0x3c), and FUN_100378b0 sets it too.
- Zones 210, 232, 236 and 239 use the pair in the same ambient loop: it sets music slots 0 / 1 to Z48 and, for tracks
  168, 131, 130 and 166, checks every frame whether the music stopped and restarts it.
- Ours: the VM stores the host's state for 0xb0 (the mixer has a track: 1 / 0; -1 without audio), and 0xb1 asks the
  audio to start its last task track again when the mixer has none. Ours skips the restart while a music change is
  under way, since that change starts its own track, and for 2 s after its own restart until the mixer has the track
  (the scripts ask every frame, and each request reads the whole file).
