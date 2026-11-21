## City openings: the hidden player, no position packets, event maps (2026-11-12)

From a survey of the nine city openings (LSB `New_Character_Cutscenes.lua`: Bastok Markets 0 then 7, Bastok Mines 1,
Port Bastok 1, Northern San d'Oria 535, Southern 503, Port 500, Windurst Waters 531, Woods 367, Port Windurst 305; all
Mode 0x83) played live on the test bed and set against retail footage. Addresses are 30261102_0.

- Reported: in a Windurst opening NPCs floated on the water and the player's own character stood among them.
  Reproduced on Port Windurst (event 305): the three Windurst openings film their night narration in Windurst Walls
  (op 0x34 #239 at the start, cameras from DAT 30843, op 0x35 back to the city before the talk), and during it our
  player (at its zone-in place, -140, -7, 172) and the port's NPCs (Calixte, Alizabe, Machichi, Yapam-Alpam, Skopopo,
  Tujaja ...) were drawn at their own coordinates inside the Walls scenery.
- **Our own character starts a zone-in event hidden.** s2c 0x037 Flags0 bit 0 (HideFlag, u8 @0x28; FUN_1009d0f0 at
  0x1009d1d6) is our actor's +0x120 bit 14, bit 1 its bit 18, bit 2 its bit 20 and bit 3 its bit 22. FUN_1008e4d0
  hides an actor no running event has bound by bits 14 / 15, and the bind (FUN_100bb300) copies them into the
  script's hide flag 0x20000, so the event begins without us until an op 0x22 / 0x4e shows us. LSB sets the flag for
  every zone-in event (`luautils::OnZoneIn`, `m_isPCHidden`). The scripts count on it: Port Windurst's 305 never
  hides the player, it only runs `4e 0, player` after the flyover; Port Bastok's 1 opens with `22 00`. Ours read
  none of the four bits. Now `event::self_hidden` (bit 0): hidden while no event has us bound, and hidden at the
  bind.
- Seen: retail shows only the actors the event binds. In Port Bastok's first shot (the gate plaza from above) retail's
  plaza is empty; ours had Argus, Flying Axe, I.M., the Voidwatch Purveyor and the home point crystal standing in it,
  and they stood behind Cornelia and Nicadio in the talk too. The same in every city (guards and crystals behind the
  San d'Oria knights).
- The client has no rule that hides them. FUN_1008e4d0 (the per-actor hide) unloads an NPC that takes no part only
  with (DAT_10481aac | Mode) & 0x10, a player with & 0x02; Mode is the 0x00A's EventMode (+0x40e44, FUN_10096640
  0x1009679a), 0x83 here, and none of these scripts but Southern San d'Oria's runs op 0x38. The model loader
  FUN_10095900 has no event gate and the 0x00E handler FUN_1009b550 reads no event state.
- What differs is what the client sends. FUN_100983f0 (c2s 0x015, each frame from FUN_10096640) builds nothing
  - while the actor is bound to the running event (+0x120 bit 7 with DAT_104819a5) unless (DAT_10481aac | Mode) & 4
    (0x1009842c: the return; with bit 4 the event body's place goes out), and
  - while the control lock DAT_104840c8 is set (0x10098530).
  FUN_10094190 sets that lock (and DAT_10488ff8) in the zone-in set-up itself when the 0x00A carries an event
  (0x1009533e), before the event starts; the event's end clears it (FUN_100ae3f0). So from the zone-in to the end of
  an opening the server hears no position at all, and the retail server sends the area's NPCs by position: they fade
  in once the opening is over *(inf: the server half)*.
- Ours sent 0x015 through events; LSB spawns NPCs, monsters and trusts on every 0x015 (`0x015_pos.cpp`). Now
  none while we are bound to the running event without Mode bit 4 (`event::no_position`, `mode::SEND_POSITION`; unit
  test `position_packets_stop_in_events`), on top of the control lock gate (`SelfState::locked`, 0x10098530), which
  the zone-in set-up sets (FUN_10094190, also on the net thread at the 0x00A) and the opening keeps to its end. The
  datagram itself still goes, as for riders.
- Also in FUN_10094190 with Mode & 0x80: DAT_104871a4 = 1 (the event's end then resets the camera as Mode & 1 does),
  the message window's cinema place (FUN_10137c70(1, 0, 0), built), first person off (FUN_100a8cc0(0)), and
  DAT_10489179 = 1, which nothing reads.
- LSB still sends the bystanders once: its SendResRdy handler (c2s 0x01A action 0x14, `0x01a_action.cpp`) spawns
  NPCs, monsters and trusts whatever the player is doing, and its 0x00A says SendCount 1 with no King NPC, so ours
  sends that packet 600 ticks after the zone-in (retail sends it in the frame the zone-in event starts, FUN_100882d0,
  after the same wait). A server change, not a client one.
- **Event maps never show NO MAP.** FUN_101fb060 (the map window's open) calls FUN_101f9970 with its "owned"
  argument 1 in modes 1..3, the event maps (ops 0x89 / 0x8d / 0xc8 through FUN_100b2570 -> FUN_101f8ef0); only modes 0
  and 5 ask FUN_10097e80 for the area's key item. Ours tested the key item there too, so a character without
  Windurst's map got the NO MAP sheet in the Windurst Woods opening (seen live with a Bastok character). (The
  negative sub-key under the loaded area's origin, FUN_101f8ef0's last argument 0, was already right here.)
- Ops 0x34 / 0x35 (evop_34 0x100b5a90 / evop_35 0x100b5b70), first frame: FUN_10092900 on every entity, ours
  included, so all models go and load again in the new area with their load fade; the scripts' `80 <actor>` waits
  after the swap wait for that. Already handled here by the event models' area swap (`models.free`, `remade`).
- 0x037 Flags0 bits 1 and 3 on our own actor (+0x120 bits 18 and 22): see 2026-11-19, SleepFlag and the unload marks
  (item 166). Not retail, and already so before this change (left open): a zone-in event starts as soon as its bound
  actors are there, where retail first waits for the King NPC count of the 0x00A (DAT_104840f3, or 600 ticks); LSB
  sends a count of 1 and no King NPC, so retail's order would hold every zone-in event 10 s on black.
- Live (test bed, character Opencs, 2026-11-12): Port Windurst 305 and Windurst Waters 531 start with 6 and 4 actors
  hidden, ours among them; the player is not drawn through the flyover and appears from the script's `4e 0` on, and
  after the swap back every actor is there again for the talk.
  The port's bystanders still arrive with LSB's answer to SendResRdy, 10 s after the zone-in (see above).
- Checked against retail footage and matching: camera paths and their order, narration timing, the player's
  walk-ins (Port Bastok's tunnel, Bastok Markets, Bastok Mines' three walks through `s000` / `s002` / `s003`), the
  scripted placements, name plates and the log during the talks, in Port Bastok, Bastok Markets (event 0), Bastok
  Mines, Northern and Southern San d'Oria and Windurst Waters. Windurst Woods has no footage; Port San d'Oria and
  Bastok Markets' event 7 were not compared.
