## Login events start on a black screen (2026-11-02)

- Bug: after creating a character, the opening cutscene briefly showed the player out of bounds of the starting city
  before its first shot. The world faded in as soon as the zone was up, while the event still waited for its actors
  (about 3.6 s live) and then for its own first 30 ticks: the San d'Oria opening 0x1f7 starts with
  `actor_flag17 1`, `camera_mode sub 1, 1`, sound / weather ops and `wait #30`, and only then starts its camera
  `s043` and its fade-in `fdi1`.
- Retail (FFXiMain.dll, the 30261006_0 build; function names as elsewhere in this log): FUN_10093db0 (sends c2s 0x00C
  after the zone load) sets the zone fade state DAT_104807f3 = 1 only when the 0x00A's EventNum (+0x40d90) is 0.
  State 1 is what runs the zone's fade-in (FUN_1008abe0: file 30904 `fdi0`). With a login event there is no zone
  fade-in, so the scene's colour multiply stays black until the event fades it in itself.
- File 30904 (sched_file #200): `fdi0` is op 0x0f to 0x80 over 30 ticks, `fdi1` op 0x0f to 0x80 over 60, `fdo0` /
  `fdo1` to 0 over 30 / 60. Each starts from the colour drawn now, so a fade-in only shows on a black screen. Ours
  started every zone at 1.0 (the zone fade is our separate World::fade), so `fdi1` did nothing and the world was
  visible from the zone load on.
- Fix: a zone-in that carries a login event sets the scene multiply to black (Screen::hold_black); the event's first
  0x0f (scheduler or effect task) fades it in from there. Ours, not retail: if the event ends, is cancelled or has
  no script without any 0x0f of its own, the scene fades in over 30 ticks as `fdi0` would.
- Other retail details seen on the way, not changed here: FUN_10094190 (the world start) with a login event skips
  FUN_100955d0 and, with EventMode & 0x80 (LSB's OPENING_MODE), sets DAT_10485ff4 = 1, which makes the event end
  (FUN_100ae3f0) put the camera behind the player as Mode & 1 does.
- Unit test client::event::tests::login_event_holds_black_until_its_fade. Not checked live yet (needs a new character).
