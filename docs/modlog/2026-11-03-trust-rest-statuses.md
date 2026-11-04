## Trusts ignore rest statuses in 0x00E (2026-11-03)

- Retail (FFXiMain.dll, docs/re/notes/2026-11-03-modlog-gaps-batch6.md item 7), the 0x00E handler's mask 4 block at
  0x1009c118..0x1009c16d: after HP% (@0x1e), the server status @0x1f goes to actor +0x16c unless the status is held by
  event op 0xab 1 / 2 (+0x120 bit 1), or the actor is a trust (u8 @0x28 bit 0) and the status is healing 0x21,
  sitting 0x2f or a chair 0x3f..0x53 (FUN_10095790). Then the old status stays. 0x00D (PCs) has no such rule.
- Ours took every status: client/mod.rs now skips those three for a trust's 0x00E (Marks.trust) as for a held status.
