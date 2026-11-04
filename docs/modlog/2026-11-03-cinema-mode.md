## Cinema mode: only the event text box draws (2026-11-03)

- Retail (docs/re/notes/2026-11-03-modlog-gaps-batch5-build.md §1): rem4line +0x48 is cinema mode. While it is on,
  FUN_101177d0 draws no window but rem4line's own (and mapframe / cnqframe, registry flag 0x40): no log, top bar,
  netstat, party, target, status icons, gauges or chat line, and the compass skips its draw. The 3D passes (name plates)
  are not gated. FUN_1015dce0 (cinema or UI hidden) blocks the menu keys, targeting, macros, hotkey categories 3 / 4 and
  opening the chat line; movement still works and the event text still advances.
- Set by event op 0x67 (x, y), text code 03 (y, with the x code 02 stored) and a zone-in event with Mode bit 0x80 (at
  0, 0); cleared by op 0x68, the event end, and the select / logout path. Code 02 alone does not turn it on.
- Ours had placement only (the narration drawn at x / y) and kept every window. Now `event::cinema` is the flag:
  `world_hud` ends by clearing the frame and drawing the name plates, an open map and the event text box only; the
  modern layer is off; Ctrl / Alt chat shortcuts and paste can't open the chat line. Code 02 alone no longer places the
  window, and a login event with Mode 0x80 starts in cinema mode (`EventStart::login`). Test
  `login_mode_80_starts_cinema`.
- Not done: the log's shrink / fade is not frozen (its state still updates underneath); the compass's fade update; the
  OS cursor's shape limits; hotkey categories 3 / 4 (event keys already take precedence).
