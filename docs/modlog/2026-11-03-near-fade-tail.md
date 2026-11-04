## The near-camera fade's tail: fishing, and the half-alpha bit (2026-11-03)

`FUN_100cd040` applies four more steps to the cylinder's fade, in this order:

1. **vt+0x260** (`FUN_100a47b0`, drawobj +0x8ac bit 0) would zero it, but its setter and clearer (vt+0x258 / vt+0x25c)
   have no callers anywhere and the actor init clears bits 0..2, so **this test never fires in this build**. Not
   built on purpose.
2. **First person**: the character the player's entity +0x2a2 names is hidden. +0x2a2 is the forced-follow target,
   written only by s2c 0x037 u16 @0x46, and the player's move step walks us toward it. Ours has no forced follow, so
   there is nothing to hide; left for whenever that lands.
3. **Fishing** (`FUN_100956c0(0xff)`: statuses 6, 0x26..0x2b, 0x32..0x35 and 0x38..0x3e), for a character entity
   other than ours while we have a drawobj (our rod, chair, mount and aura actors have no entity, so they keep their fade): by the horizontal
   distance d to the player, d < 1.0 hides it and 1.0 <= d < 2.5 multiplies the fade by (d - 1) x 2/3.
4. **Entity +0x130 bit 18** draws it at **0.5**, over anything the steps above said. The bit comes from s2c 0x00E
   u32 @0x28 bit 31 and our own 0x037 clears it.

The op 0xa4 no-fade bit (+0x12c bit 26) is applied by the caller after the whole function, tail included: the
actor update stores FUN_100cd040's result in model +0x664 (0x100c7d92), then sets +0x664 = 1.0 when the entity has
bit 26 (0x100c7dc0..0x100c7dd5). So an event's no-fade actor is never faded by fishing or by bit 18. The player in
first person skips FUN_100cd040, tail included (0x100c7d83).
Then the result is capped at 1. Steps 3 and 4 are `near_fade_tail`, with the bit carried on `Entity::half_fade`.
The team-outline and party-highlight flags that follow in the same function change no alpha.
