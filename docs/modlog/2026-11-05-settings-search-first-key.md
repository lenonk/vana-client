## The Settings window search opens on its first key too (2026-11-05)

- Ours: the Settings window's search (modern/config.rs, `find_open`) opens on the first typed letter like the list
  search, and gets the same check in `key_functions`, so that letter runs no key function.
