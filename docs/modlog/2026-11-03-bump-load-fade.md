## The bump needs the load fade finished, and has no event test (2026-11-03)

**+0x59c is the actor's load fade.** It starts at 0, rises by frame ticks x 1/32 to 1.0 while the model object
reports ready, and falls by ticks x 1/16 to 0 once the actor is going away (vt+0x36c). The draw multiplies by it, and
`FUN_100a8898`'s bump test requires **+0x59c >= 1.0**: an actor is solid only once it has faded in, about half a
second after its files land, not while it fades in or out. Ours is `Actor::appear`, which the bump now checks.

**The bump has no event test of its own.** `FUN_100a8770` is called from the player's move step `FUN_100a6240` after
the delta is built, and reads no event state; its gates are the GM level, the per-actor eligibility and the fade
above. What stops it during an event is the move step itself, which returns before any movement when `FUN_100964f0`
("can move") is false — no player, entity +0x12c bit 5, the talk watch, statuses 0 / 2 / 7 / 10 / 0x1c in the icon
list, the zone-in not finished, fishing, and so on. The forced-follow path at the top of that function moves the
player without the bump, and a script-driven walk never goes through it at all.

Our `!in_event` gate on the bump call stands in for that early return, since our movement is already blocked in the
same situations. Not built: the leaving half of the fade (ticks x 1/16 down), which needs a despawn state our client
does not keep - actors are dropped when they go.
