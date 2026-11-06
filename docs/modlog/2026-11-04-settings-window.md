## Modern Settings window (2026-11-04, ours)

- Not retail. The modern interface's Config window (client/modern/config.rs) becomes "Settings": retail's Config list
  (Gameplay, Chat Filters, Font Colors, Misc. 1..4, Effects, Mouse/Cam., Global) and our Settings / Extras entries are
  replaced by fourteen topic pages in five groups (Game: Gameplay, Interface, Chat & Log, Maps & Timers; Graphics:
  Display, Image Quality, Lighting & Shadows, Characters & World, Effects & Colour; Sound; Input: Mouse, Camera & Keys,
  Controller; System: Startup & Account, Mods).
- `PAGES` lists each page's lines: a heading, retail's setting by its Config page and first control (its captions,
  options and help still read from the page's layout), ours by its row's label (`SettingsUi::item` / `item_set` find a
  row of Settings or Extras whatever page that state is on), or a button opening Chat Filters, Effects, Font Colors,
  Mods or the Lua mods' pages (`SUBS`, Escape or Back returns to the topic page). Rows of ours left out here (the
  lobby's volumes in game, the minimap's and timers' places with the modern interface) go, and so do headings with
  nothing under them.
- Unchanged: the Windows and Log pages and Mouse/Cam.'s mouse rows stay left out; the title screen's Config keeps its
  own list; the classic interface keeps retail's Config windows. `/lua settings` opens Mods > Mod Settings.
- Tests: client::modern::config::tests::every_setting_has_a_page (each retail setting of the old list and each row of
  ours on exactly one page), empty_headings_go. `modern-shot --settings` draws the Display page.
