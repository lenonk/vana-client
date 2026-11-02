## Gamepad: round stick dead zone (2026-11-02)

- Ours: the movement and camera sticks' dead zone is round (a stick's two axes zeroed together while its push is
  under it). Retail's FUN_10005a40 zeroes each axis under 42 on its own, which snaps a stick within asin(42/127) ~19
  degrees of an axis onto it, so turning the stick turned the character in jumps. The d-pad from an axis keeps the
  per-axis rule.
