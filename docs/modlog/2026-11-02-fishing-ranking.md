## Event op 0xb3: the Fishing Ranking (2026-11-02)

- Op 0xb3 (FUN_100bbaf0, current build 30261006_0) is the Fishing Ranking, used in Jugner Forest (104), Pashhow
  Marshlands (109), Meriphataud Mountains (119), Ru'Lude Gardens (243), Upper Jeuno (244), Selbina (248) and Mhaura
  (249). Ours sent every sub to the host as an unhandled effect, so the request never went out and sub 1 / 5 left
  their variables untouched.
- Subs (every one ends the frame): 0 / 3 / 4 / 6 / 7 start request 1 / 10 / 11 / 12 / 13 with val@2 (FUN_100a3310:
  only while no transfer runs, the server message's included; a value of 0 or over 7 is 0). 1 waits for a result,
  then set@2 = the result (i8), @4 the entry count (i16), @6 our contest rank, @8 our main job (only for values 1..4,
  6, 7), @10 dataset_b, @12 share. 5 does the same and also @14 score and @16 submit time, and also waits while no
  request was made. 2 opens "menu    ranking" when the request's value is 1..7, 8 closes it, 9 set@2 = it is open.
- The request (DAT_10480aa8) uses the server message's transfer (c2s 0x04B / s2c 0x04D) with kind 2:
  - Command 1 asks the table in 0xb4-byte pieces (five 36-byte entries), up to four at once after the first, each
    piece asked twice at most (3 s apart); 450 ticks without a reply starts over once, then fails. The reply carries
    the entry count @0x18, our entry @0x1c and the piece's data @0x40 (FUN_100a2dd0). Any piece out of order, past
    0xe10 bytes or with another timestamp fails the request.
  - Commands 10..13 send one empty request; the reply's result byte is the answer, with the entry count and our
    entry when it is -3, -2 or 1 (FUN_100a2d20).
  - A request that ends with result 0 reads -1 (FUN_100a2810), which the scripts check before their "no ranking"
    messages.
- The window (FUN_10209b90, list base FUN_101f67c0): 15 rows 16 apart from (+0, +4), rows with a contest rank
  only (FUN_100a3500). Columns and cells by type (FUN_1020a2a0):
  - 1..3 (0, 18, 160, 243, 275, 305): rank, job / level cell (FUN_101ca6c0 over the entry's jobs, levels, race,
    allegiance and fishing rank), name, "[score]", the submit time as a Vana'diel date (FUN_101c92b0(t, 2)).
  - 4 (0, 18, 24, 120, 243, 305): rank, fishing rank icon (keytops3 0xc0 + rank + 1), name, "[score]".
  - 5 (0, 18, 185, 200, 265, 305): rank, name, score, "[%d-%d-%d]" of +0x14 (short), +0x17, +0x16, the submit time
    moved to Earth time (t - game clock + time()) as a long date (FUN_101c8fd0(t, 7)).
  - 6 / 7 (0, 24, 160, 250, 265, 305): rank, name, "score P", the Earth date when on or after 2002-04-01.
  - The rank is right-aligned, dimmer above 10. Up / Down move, Left / Right page by 15. Escape closes it *(inf:
    the generic window handler)*, which ends the scripts' sub 9 loop. Modern mode lists the same rows as a panel.
- Ours: client/ranking.rs, Host::ranking / ranking_open. Tests: `table_in_pieces`, `bad_piece_fails`,
  `answer_result`, `fishing_ranking_b3`. Event sweep of the seven zones: unchanged but Selbina 0x2717, which now
  takes the script's failure path (message 7669, then `ret`) since a headless run has no server; 0 faults. Not
  checked live or on screen.
