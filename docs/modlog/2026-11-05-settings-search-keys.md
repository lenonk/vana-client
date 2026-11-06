## The Settings window's search keeps its keys (2026-11-05, ours)

- The Settings window's search (modern/config.rs) took the typed text from the egui events, but the key functions
  (input.rs, run first in `key_functions`) only knew the fields listed in `typing_elsewhere`, so each letter typed there
  also fired its game hotkey. It now counts as typing while it has text (`config::finding`). Key functions with mask
  0x41 then fail, as for the chat line.
