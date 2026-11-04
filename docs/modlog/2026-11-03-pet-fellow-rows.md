## Party window: the pet and fellow rows (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch5-build.md §5 and batch 1 §4) draws two frameless
  rows beside the party window: our pet and, above it (or in its place), our adventuring fellow. Ours had neither.
  - FUN_102041d0 (0x10204462..0x10204597): the rows end 8 px left of the party window when they fit beside the log,
    else they sit over the log's (or log window 2's) right side; first row y = that bottom less 0x17, rows 16 px
    apart. Skipped while a group-8 window is open (lists, config windows, map, full log, macro editor, ...), an
    event runs, or a two-step party pick is on its second stage.
  - FUN_10202960 / FUN_10202a40: the name (fontshp, cut at 102 px with the ellipsis glyph, FUN_101c9d50) in ncol 1
    (pet) / ncol 4 (fellow); with a model the HP gauge (windowps 38 frame, 97 fill, never eased); the pet's TP with
    /showtp (not for the luopans 0xb22..0xb31, only from 0x068 when its index is our pet's).
  - The party-list cursor of `<stpt>` / `<stal>` gets row -1: past the last row (or Up from row 0) it lands on the
    fellow, then row 0; Enter passes the fellow's id or name. The party menu's picks and the mouse can't reach it.
    The hand there is the anc2s hand at the row, tinted by range.
- Ours: client/petrow.rs (origin / draw), target.rs PartyCursor.fellow and fellow_hand, render/screens.rs
  push_shape_limited.
