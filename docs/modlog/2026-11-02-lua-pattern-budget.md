## Lua mods: string patterns keep to the time budget (ours, 2026-11-02)

- `string.find` / `match` / `gmatch` / `gsub` ran Lua's matcher in C, which executes no VM instructions, so the
  budget hook (every 10,000 instructions) never fired while it backtracked: `string.find(string.rep('a', 40000),
  '^.-.-b')` blocked the game thread for 3.6 s, and unanchored for hours (issue #83).
- The four are now a Rust port of Lua 5.4's lstrlib.c matcher (client/lua/pattern.rs), step for step: the same
  results and errors (a test runs a set of calls through both and compares), and every 1024 match steps it checks the
  mod's deadline. Past it the call fails with "ran past its time budget", as a runaway loop does. A plain find
  (`plain` or no magic characters) is a linear substring search.
