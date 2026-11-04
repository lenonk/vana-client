## The page arrows disable their list while hovered (2026-11-03)

The paged-list base has a disable / enable pair (confirmed with the helpers read for batch 5 item 5):

- `FUN_10221cd0` (disable): window +0x62 = 0, so no cursor and no hand, and list **+0x5c |= 2**.
- `FUN_10221ce0` (enable): +0x62 = 1, and if bit 1 was set it rebuilds (`FUN_10221ed0`) and clears it.

Hovering an arrow (children 9 / 10, message 9) disables; hovering a row enables; every rebuild clears the bit. While
the bit is set the list ignores **every** message, keyboard pages included. A click on an arrow is Enter on child
9 / 10, re-sent as Left / Right through `vt+0x18`, which tests the page modifier 0xa8 again
(`FUN_10123a70(0x3e, 0xa8, 4, -1)`): with Ctrl or Alt held that is a page on the disabled list, so **nothing
happens**; without it the log / area / week / category changes.

Built for the three windows whose arrows take the test: miss00, questlst and primer. RoE's arrows and jbpcat's page
areas skip it (a Ctrl-click switches there), so they are left as they are.

`World::list_arrow_hover` carries the disabled bit. Retail keeps it until a row is hovered or the list is rebuilt;
ours is recomputed from the pointer each frame, so a pointer that leaves the window cannot strand the keyboard.
