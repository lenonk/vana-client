## The GM window's text fields keep their keys (2026-11-05, ours)

- The GM window (modern/gmwin.rs) took the typed text from the egui events, but the key functions (input.rs, run first
  in `key_functions`) only knew the fields listed in `typing_elsewhere`, so each letter typed there also fired its game
  hotkey. It now counts as typing while it has the keys (`gmwin::takes_keys`: shown, nothing over it, no event). Key
  functions with mask 0x41 then fail, as for the chat line. Its footer says "Vana Client only" instead of "Ours".
