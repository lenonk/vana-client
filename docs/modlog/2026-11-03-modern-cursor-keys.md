## Modern UI: keys no longer hide the mouse cursor (2026-11-03)

- Ours, a deliberate deviation from retail. Retail hides the cursor on any key function press outside a drag
  (FUN_10125f70(0)), so walking with the keys makes the pointer vanish until the mouse moves. With the Modern UI
  on, keyboard presses leave the cursor shown (login and character screens too); the classic UI keeps retail's behaviour. The other hides are
  unchanged in both modes: a controller press, a look drag (the pointer held) and the UI hidden.
