## Settings window: search, Retail only, details (2026-11-04, ours)

- Not retail. The modern Settings window (client/modern/config.rs) gets a search field over its topic pages: typing
  there (`find_events`, ahead of the game's keys) shows every page's matching settings under the page's name. A
  setting matches by its name, by a word of its help starting with the text (3 letters or more), or by the heading
  it is under. Backspace deletes a letter, Escape clears the search, a page picked in the list ends it.
- A Retail only button (or Ctrl+R) leaves our settings out of the pages and the search.
- A details column on the right shows the setting under the cursor or the pointer: its name, its help (the bottom
  line now lists the keys instead), whether it is retail's, and its page while searching.
- Mods and Mod Settings show only their own tabs (Mods; the Lua mods' pages), not the classic Settings' or Extras'.
- Tests: client::modern::config::tests::search_and_retail_only, mod_tabs_only. `modern-shot --settings` now draws a
  search for "shadow".
