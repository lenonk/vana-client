## Full log: Window 1 / 2 scroll back through 1000 rows (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch12.md item 77, handler FUN_10138230): the full
  log's 100 rows are a window over the history store, not the history itself.
  - At the older edge, Window 1 / 2 (tabs 0 / 8) load older rows from the store (FUN_1012a650). The store holds 20
    rotating "%d_%d.log" pages of 50 rows.
  - Channel tabs stop at the rows they hold (at most 100).
  - The wheel moves one row per notch (0x16 older, 0x17 newer), the hand stays on its screen row, and SE 0001 plays
    when the view moves.
- Ours already matched the wheel, the keys and the channel tabs' 100 rows. Window 1 / 2 kept 512 rows; they now keep
  1000 (logwin.rs `KEEP` = 20 x 50). A rewrap (resize, edit, route change) refills up to 1000 rows too; it used to
  refill only the ~50 rows retail loads at an open, which dropped the older history.
- Not modelled: the store's files on disk (ours keeps the rows in memory for the session).
