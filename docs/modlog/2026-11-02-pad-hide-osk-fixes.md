## Gamepad: UI hide covers the modern layer; the keyboard's opening press (2026-11-02)

- Key function 0x56 (Back / Scroll Lock) hides the UI: retail's menu manager +0xb4 draws no window at all (see "UI
  hide, screenshots, Pause"). Ours: the modern layer's windows are not drawn either while it is set (they stood
  where retail's windows would). B / Esc still shows it again.
- Ours: buttons held when the on-screen keyboard opens or closes are ignored until let go. RB raised the keyboard and,
  still held, struck its Enter, closing the chat line, and on release of the keyboard re-pressed 0x24 (open the
  chat line), so a held RB opened and closed it over and over and the game took no other pad input meanwhile.
