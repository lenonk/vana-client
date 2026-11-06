## The key that opens the list search is no hotkey (2026-11-05)

- The modern list search (listsearch.rs) opens on the first typed letter or digit, but `key_functions` runs before
  the frame's egui events reach `listsearch::events`, so that first key still ran its key function (later keys were
  fine: `list_search` is set and counts in `text_field_open`). `key_functions` now looks at the frame's typed text
  still queued in egui-winit (`egui_input().events`): a letter or digit while `listsearch::can_run` makes the frame
  a typing one, and key functions with mask 0x41 fail for it.
