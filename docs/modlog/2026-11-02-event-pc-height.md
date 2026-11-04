## Event op 0xab 0x0f / 0x10: unbound player characters keep the sent height (2026-11-02)

- FUN_100bac90 0x0f / 0x10 set DAT_10489900 to 1 / 0 (2 bytes each). The event's end clears it (FUN_100ae3f0).
- The s2c 0x00D handler (LAB_10099ae0 at 0x10099e89) stands a player character's sent position on the floor
  (FUN_10181290, 50 up) unless DAT_10489900 is set, the player is held
  (FUN_10087060: DAT_10487e48, which events set and their end clears), and the character is neither bound
  (+0x120 bit 7) nor part of the event (bit 8, set by FUN_100aeea0). Then the sent height stays.
- Ours already leaves heights alone while an event runs. The one place it stood a player character on the floor was
  the first update (FUN_10091220's spawn). That now keeps the sent height while 0x0f is on and the character is not
  bound. Used in zones 51, 65, 243, 244 and 256 (7 uses; 0x10 in 9).
- Traced, unchanged: 0x12 / 0x13 set and clear +0x128 bit 24 on the thread's actor. The only reader is FUN_100883e0
  (0x10088426 / 0x10088466), which runs only for our own character (+0x1d0 bit 9, FUN_1008e290) and moves its
  fishing state (+0x128 bits 21..23) to 3. Every use (zone 48, nine, actor 0x206) names an NPC, so nothing happens;
  0x13 is not used.
