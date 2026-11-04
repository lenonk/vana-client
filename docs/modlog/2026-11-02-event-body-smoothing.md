## Cutscene actors drawn between VM steps (ours, 2026-11-02)

- NPCs shook during cutscenes. The event VM moves its bodies in 60 Hz ticks, as retail does, but we draw at up to the
  frame limit (300 by default). The bodies stood still for several frames and then jumped, while the scheduler camera
  path is sampled every frame, so a walking actor visibly vibrated (and a camera framed on actors jumped with them).
- Not retail (it draws a whole number of ticks a frame): `event::pose` draws each bound actor on its way to its last
  VM step, from where it was drawn when that step came, over the tick after it (`Pose::since` counts the ticks drawn,
  the step's own frame included). It never goes back, rotation and tilt go the short way round, and a placement (s2c
  0x05B / 0x065 snap, or a jump of over 2 a tick) puts the place and tilt down at once. A frame of a tick or more
  (60 fps and below) draws the stepped pose, as before.
- The VM, its host (`drawn_at`), our own place (`keep_pose`) and the event's end still use the stepped pose.
