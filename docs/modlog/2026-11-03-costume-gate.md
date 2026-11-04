## Costume: the trade and command refusal (2026-11-03)

Checked in Steven's unpacked FFXiMain.dll (sha256 8097dbf8..., 2026-11-03); notes in
`docs/re/notes/2026-11-03-modlog-gaps-batch1.md` §7, `-batch3.md` §1 and `-batch4.md` §1 / §4.
- s2c 0x037 @0x44 (u16) is our costume id (an NPC model id). A new id sets actor +0x120 bit 4 (pending,
  FUN_1008cce0). Once a frame FUN_10095e90 puts a pending costume on (bit 5) unless the actor is bound to the event
  (+0x120 bit 7), or takes it off for id 0. While on, statuses 2, 3, 5, 6, 7, 0x21, 0x26..0x2c, 0x2f, 0x30,
  0x32..0x35, 0x38..0x53 and 0x55 take it off and leave it pending again; they never stop it going on.
- FUN_101cc0d0's mask 0x40 refuses with group 7 line 0xb1 while bit 5 is on, checked after 0x04 and before 0x08.
- Ours: `client/costume.rs` keeps the id and the two bits for our own character; `gate_line` checks 0x40 in that
  place (trade, the storage moogles' gate 0x44, every command passing 0x40). "Bound" is our character among the running
  event's bound actors. Reset at zone-in. The costume model itself is not drawn.
