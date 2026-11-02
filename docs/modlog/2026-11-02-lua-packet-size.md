## Lua mods: injected packets fit their size field (ours, 2026-11-02)

- `packets.inject_in` / `inject_out` took a whole sub-packet as it came (4 bytes or more), so its header's size
  field could disagree with its bytes; a hook's result was already fitted. Injected packets now go through the same
  fit: padded to a word, the size field set from the length (issue #69).
- The size field is 7 bits of 4-byte words, so a sub-packet is at most 508 bytes. `build`, the `(id, body)` form of
  inject and a whole injected packet over that raise an error; a hook result over it (or under 4 bytes) is reported
  as the mod's error and the packet goes on unchanged. An id over 0x1ff is refused instead of being cut to 9 bits.
