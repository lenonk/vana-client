## Event gender values and dialog speaker picks (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03); notes in
`docs/re/notes/2026-11-03-modlog-gaps-batch3.md` §2 and `-batch4.md` §2..3.
- Actor +0x124 bit 25 is the gender bit, 1 male. s2c 0x00D and 0x00E write it from u32 @0x20 bit 15 under update
  mask 4, for every actor kind (NPCs and monsters too); the actor reset leaves it 0 (female) until then. Ours: the
  bit is now read for every kind, and an entity made from a packet without mask 4 starts female.
- Event values 0x7f0b / 0x7f8b (FUN_100af4c0) read it for the event owner / the player: scripts branching on
  gender (~475 sites). Ours read 0 (female) for both; now the speaker's and the player's bit (ours from the race,
  inf: retail copies 0x00A @0x20 bit 15).
- Dialog `7f 85` picks the `[a/b]` form by the player's bit, `7f 90` / `7f 91` by the message's actors 1 / 2
  (+0xbfe / +0xc00, FUN_100c2e20; male the first form). 0x036 / 0x043 / 0x02A set both to the speaker; 0x029,
  0x02D and 0x05A the actor and the target. Ours used the player's gender for all three; now event and zone text
  use the speaker for 7f 90 / 7f 91 (one not here: the first form, inf).
