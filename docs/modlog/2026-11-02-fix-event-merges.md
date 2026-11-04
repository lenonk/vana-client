## Fix the build after the event-op merges (2026-11-02)

- The conflict fixes made while merging #43 to #50 kept both sides of each conflict in `event_vm.rs`, `client/event.rs`
  and `dat/event_ops.rs`, so master stopped compiling: an unclosed `game_time` in the test host, tests spliced into
  each other, a half-kept 0x78 arm still reading the removed `State.time`, the old 0x5f sub 2 arm left open, and
  duplicate op table rows for 0x5f, 0x95..0x98, 0xc1 and 0xc2.
- Rebuilt from the feature branches merged in the same order: every test from each PR is back as its own function,
  0x78 both restores the server weather held by op 0x77 and resets op 0xae's blend, and each op row keeps its newest
  description. No behaviour beyond what the PRs added.
