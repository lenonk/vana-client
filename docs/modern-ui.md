# Modern UI

The plan for vana-client's modern interface, from the user's planning document (Vana_Client_UI_Implementation_Plan,
26 October 2026), with the decisions taken since and the state of the work. Branch `ui-update`.

## Decisions (2026-10-26)

- **Runtime toggle.** The retail UI stays the default and untouched. Settings > Linux > Interface switches between
  Classic and Modern (config `modern_ui`). While Modern is on, the modern layer draws the windows it has and the
  retail windows they replace are left out; everything else stays retail until it has a modern counterpart.
- **egui.** The modern layer is egui (already in the client, on wgpu): windows, layout, focus, drag-and-drop,
  tooltips and text come from it; the pack's art is drawn through its painter.
- **Fonts bundled.** Inter (interface), Cinzel (titles), JetBrains Mono (logs), all SIL OFL 1.1, unmodified, from
  their official releases, in `crates/vana-client/assets/fonts` with their licence texts, embedded in the binary.
- **Order.** Milestone 1 (theme and atlas loader, panels, movable windows with saved layout, HUD basics), then the Mog
  House storage slice.
- **Config pages.** The modern Config pages leave out the options that mean nothing with the modern UI (window type,
  chat window width and the like; user, 2026-10-26).
- **Hunt the old limits.** Retail's designers worked under limits we don't have (ten-row lists, numbered bags, pages
  of item text, modes to switch between, remembering where things are). Seek those out and remove them, keeping
  everything that makes it FFXI. User, 2026-10-26.
- **One command window.** Every window the command menu opens is the same panel ("window") with different contents, so
  it stays where the user put it whatever it shows; a few exceptions (the map first of all). User, 2026-10-26.
- **Equipment by slot, from everywhere.** Enter or a click on a slot lists every piece the character can wear there,
  from every bag, with where it is; the player never has to remember the bag. Inventory and wardrobe pieces equip
  directly; satchel, sack and case pieces (and the Mog Safe, Storage, Locker and Safe 2 at the moogle) are moved to the
  inventory and then equipped; the others show greyed with their place. Drag and drop equips without closing or
  resetting anything (retail's pain point). User, 2026-10-26.
- **Numbered entries merged.** Mog Wardrobe 1..8 is one Mog Wardrobe showing them all; a piece put in takes the next
  free slot in any of them. The same for every entry with numbered versions (Mog Safe / Safe 2, Config's Misc 1..4
  and the like). User, 2026-10-26.
- **Nameplates.** Replaced in Modern with plates like the mock-up's; no HP bars over heads. User, 2026-10-26.
- **Jobs are written WAR22/WHM11**, exactly so (main and level, slash, sub and level). User, 2026-10-26.
- **Breathing room.** Rows and frames keep a few pixels between their lines and their contents.
- **Icons.** The pack's icons are placeholders. A full icon set (every job, spell, buff and debuff) may come later;
  game art is mapped by stable game ids, never by atlas sprite ids.

## Goal

A genuinely modern interface without turning FFXI into FFXIV or reskinning the 2002 menus. Keep the deliberate,
tactical, auto-attack combat and the deep job and spell systems. Make exploration, inventory, storage, navigation and
everyday interactions friendly to the mouse, which is useful but never required.

Success: a player explores and manages their character with clickable, movable, discoverable windows; plays combat
entirely from the keyboard; finds any spell or ability quickly without memorising sequences; and still recognises the
pacing, decisions and atmosphere of Vana'diel.

## Firm design rules

- Keyboard-first combat; directional menus and Enter / Escape stay first-class.
- Mouse-friendly exploration and management: clickable actions, movable / resizable windows, drag-and-drop between
  inventory and Mog House storage, context menus, tooltips.
- Mouse optional everywhere: every action has a keyboard path and a visible focus.
- No wall of hotbars. A small optional favourites strip at most; the main system is a command browser.
- Modern feel over fidelity to the old window places, blue palette or menu structure.
- FFXI mechanics intact: deliberate action selection, auto-attack, skillchains / magic bursts, party play, targeting.
- Suggestions are allowed but explainable, never automatic, never hiding the full list, and never reorder the
  highlighted row while a menu is open.

## Interaction

- **Exploration:** NPCs, doors, objects, party members, targets, menu entries and equipment clickable where sensible;
  tooltips, right-click context actions, hover and focus feedback, configurable bindings. Windows movable, optionally
  resizable, dockable / pinnable, layouts saved per character. Nothing needs the mouse.
- **Combat:** a compact HUD (own HP / MP / TP and effects, target / focus / sub-target, party, cast bars, a restrained
  log) and a keyboard command palette (Attack, Magic, Abilities, Weapon Skills, Items, Check, Disengage ...): arrows
  move, Enter acts, Escape backs out; a separate fuzzy search ("has" finds Haste); recent picks remembered.
- **Spells and abilities:** one searchable action registry: categories, favourites, recent, job filters, costs,
  recasts, range, valid targets and why an action is disabled.
- **Recommendations:** a deterministic, testable rules engine (Poisona for a poisoned ally, Cure for low HP, an
  expiring Refresh, burst spells in a skillchain window, weapon skills at TP); never casts, never moves the cursor,
  updates at safe points, shows its reason; no runtime LLM.
- **Targeting:** target, sub-target, focus target, last target and party selection shown explicitly; clicks may set
  a target; cycling and target-by-name stay.

Interaction contract from the pack (docs/INTERACTION.md there):

- Left click selects a target; double click the default safe interaction; right click a context menu. Every visible
  control is clickable and keyboard-focusable.
- Storage: drag between inventory and the Mog House containers by the server's rules; teal / red for valid / invalid
  drops; Shift splits a stack, Ctrl-click sends to the paired pane; a keyboard path for all of it. Never discard
  optimistically: wait for the server, restore and explain on failure. Item identity is the slot, never a sprite.
- Combat: the palette never steals focus or auto-targets; auto-attack carries on with menus open.
- Window manager: layouts per character, display and UI scale; window chrome drags apart from content drags; Reset
  Layout always there.

## UI states

| Mode | Emphasis |
|---|---|
| Exploration | World, interactions, minimap / coordinates if on, quiet chat and status. |
| Combat | Target, party, HP / MP / TP, casts and skillchain cues, effects; keyboard commands. |
| Management | Inventory, storage, equipment, jobs, magic, macros; movable multi-pane windows. |

Modes are adaptive presets, not fixed screens; widgets can be pinned and transitions turned off. Never take the
keyboard in combat.

## Visual system and assets

Charcoal / slate, muted bronze, parchment, restrained jewel tones, readable contrast; Vana'diel-inspired icons and
type, little ornament. Text is rendered at run time, never baked into sprites.

The pack in use is the production pack v2 (2026-10-26, `crates/vana-client/assets/modern`; the first pack was
replaced the same day):

- `manifest.json`: 304 items with logical sizes, 2x atlas rects and nine-slice insets in logical pixels; six 2x
  atlases; `svg/` sources; `theme.json` (the palette); `NINE_SLICE.md` (the frame contract).
- Window frames (normal, focused, inactive, alert, tooltip, border-only): 80 x 80 logical, 24 insets, ornamented
  edges meant to be tiled, never stretched. Every variant shares the border shape and has a pure vertical gradient
  inside; the layer cuts each variant's border out with the border-only frame's alpha and draws the gradient
  stretched under it, so a window of any size has no banding and no stretched ornament (client/modern/paint.rs).
- Gauges as empty / full pairs (HP, MP, TP, cast, experience, recast), drawn as the empty bar with the full one cut
  at the value.
- Controls: buttons, list rows, inputs, tabs, title bars, item slots and drop outlines, arrows, check boxes, key caps,
  scroll thumbs. 22 job pictograms (not SE's emblems). Indicators: target rings by relation, priorities, item and
  status badges. 131 generic icons (placeholders for game art).
- Text is never in the art; fonts are Inter and JetBrains Mono (Cinzel was dropped with the first pack).

## Technical plan

A. Input contracts: canonical actions (navigate, confirm, back, search, target next, open menu, drag, drop ...) from
   keyboard, mouse and pad adapters; input focus kept apart from world targeting.
B. Retained UI state: widget ids, focus paths, z-order, window geometry, docking / pinning, layout persistence.
C. Theme and assets: atlases, nine-slice, DPI scaling, fonts with fallback, clipping; manifest-driven lookups.
D. Action registry: actions from job, spells, abilities, learned state, recasts, costs, targets, server state; one
   search / filter API for the palette, favourites and suggestions.
E. Combat navigation: menu > category > action > sub-target > server command, fuzzy search, recents, favourites.
F. Storage slice: two-pane Mog House transfers with drag-and-drop, keyboard parity, server-confirmed results.
G. Recommendation rules, tested against recorded game states.
H. Polish: 1080p / 1440p / 4K, ultrawide, mouse-only exploration, keyboard-only combat, high UI scale, layouts,
   combat focus stability, accessibility.

## Milestones

1. **Interaction prototype:** movable windows, focus, keyboard traversal, theme loader, text and nine-slice.
2. **Storage:** inventory and Mog House transfers by drag-and-drop and by keyboard; errors and limits handled.
3. **Combat palette:** every command without the mouse; searchable spells and abilities; explicit target states.
4. **Context and customisation:** suggestions, favourites, saved layouts, per-mode visibility, UI scale.
5. **Production pass:** asset QA, consistency, performance, tests, real play sessions.

Done means: common exploration is discoverable by click; storage works by both inputs; combat plays by keyboard
alone; no hotbar chores; menus stay still while navigated; state changes are server-confirmed; layouts persist and
scale.

## Open questions

- Which keys open the command palette and the search? (Tab is FFXI's target cycling.)
- Click-to-move, or only mouse targeting and interaction?
- Which windows hide in combat and which stay pinned?
- How macros and equipment sets show in search and favourites.
- How to show skillchain windows without a rotation HUD.
- Which icons come from the DATs and which are new art.
- How latency, failed transfers and invalid targets are shown.

## State (branch ui-update)

Milestone 1:

- client/modern: `theme.rs` (palette, manifest, atlases as egui textures, the frames, fonts), `paint.rs` (windows of
  any size, nine-slices, gauges), `hud.rs` (the panels from views gathered from the World), `chat.rs`, `command.rs`,
  `combat.rs`, `mod.rs` (the layer, zoom, panels with saved places), `shot.rs` (`vana-client modern-shot
  [--backdrop PNG] --out PNG`: the panels with sample values and every frame, offscreen).
- egui's zoom follows the window height against 1080, times Settings > Linux > Interface Size (config modern_scale,
  50 .. 150% in 5% steps, default 60%).
- Panels: Status, Party, Target, Location (zone, grid, time, day, weather, and the compass: retail's disk turned so
  the camera's heading is up and tilted by cos 1.0996, N E S W, the radar dots, the tracking / call-for-help arrow),
  the cast bar, Chat (tabs, the log, the chat line, the Enter key cap), the Command Menu (the main menu with
  submenus), the combat window (the action menu's rows).
- Every panel moves by a drag on its background (contents keep their clicks); places are screen fractions per
  character in config `modern_places`, saved when the pointer is let go. Only the chat window resizes, by the glyph
  in its corner: more room for the lines, nothing scales. Its settings button (the tabs' right) sets the font size
  (config `modern_chat_font`, 10 .. 32, default 15). User's call (2026-10-26): no lock button, no other resizing.
- The top bar: retail's titlewin, helpwind (marquee, busy strip, "Help" legend) and netstat (meter, %, S / R) in
  their places and by their fade rule (hudtop.rs fills a TopView), in the pack's frames on a layer that takes no input.
- The chat window runs the retail log's rule (logwin::LogAnim, shared with the retail log) on its own wrapped rows:
  a period per new row up to the lines its room holds, a row off every period down to the min lines, Escape, the
  chat line's two rows (the line in the bottom row, an empty row above it), the eased height, the slide-in, the full
  log closing it. With no lines it goes on folding down through its tab row until it is gone, at the same rate
  (user's call: no fade). It sits on the screen's bottom and grows upward; moving or resizing a panel holds it open.
- Docking: a panel let go with its bottom within 5 px of another's top (over it) sits on it and keeps the distance
  as the other moves, grows or folds away; chains follow, a hidden target counts as no height where it sits.
  Defaults as retail: the location panel on the chat window (the compass on the log), the combat window on the
  location panel, the target on the party window (targetwi on the party window). The game's windows still drawn
  retail-style that stack on the log (flag 0x1000) stack on the modern chat window, the Timers window too.
- The game's windows and the world take no click, press or wheel over a modern panel.
- The effects panel: our status icons and times alone (user's call: the name, jobs and bars are the party
  window's), shown while any. Party rows show HP over max: ours known, a member's from when it was last at 100 %
  (party_hp_max), else HP / HP %.
- The magic and ability windows (lists.rs) over magic.rs's state: the category window (mgcmenu, magselec, abimenu,
  abiselec, commenu, Monstrosity's) by name, the magic / ability / mount / emote lists with the game's icons, MP
  cost and recasts, greyed rows, the sort strip (Auto-sort, Manual Sort, Set Spells; the Yes / No), the cost box,
  a bar; the mouse as retail's (hover moves the cursor, a click on it is Enter), the wheel.
- Search (listsearch.rs): typing in a category window or a list searches its names (start, word start, anywhere,
  letters in order; roman numerals as digits), from a category window every list behind it; Up / Down, Enter
  picks through retail's keys (the category, the row, Enter: checks and the target prompt as ever), Backspace,
  Escape ends it.
- The Items window (modern/items.rs) over items.rs's state: the list and the action menu's direct-use list with the
  game's icons, quantities and row colours, the count, a bar; itmsort2 as a strip (Auto-sort and its question, Manual
  Sort, the Recycle Bin), iuse (Use / Drop) and the drop question under the rows, Manual Sort's hint; the mouse as
  retail's, the wheel. Search there gives one hit per item with its total; a pick puts the cursor on its first slot
  and presses Enter. The storage views are still drawn retail-style.
- The command window ("window"): the Items window and the Equipment screen show in the same panel at the same place
  (user's rule: every window the command menu opens).
- The Equipment screen (modern/equip.rs; equipwin.rs's modern path): the persona (name, main / sub job, HP / MP / TP),
  the sixteen slots, the resistances under them and Attack, Defense, STR..CHR at the bottom in the slots' columns
  (monospace). Enter or a click on a slot lists every piece the character can wear there now from every bag (the one
  worn first, then by level), with where it is: inventory and wardrobe pieces equip at once; satchel, sack and case
  pieces (the moogle's bags in our Mog House) go to the inventory first (c2s 0x029) and on when they land (tick, 5 s);
  the rest greyed with the reason. Twelve rows (retail's ten were its limit). Drag a listed piece onto a slot to put it
  on, a worn one onto another slot to move it, onto the list column to take it off; nothing closes or resets. The
  list compares with the slot's piece in the info panel at once. Seen live (2026-10-26): every path above.
- The item info panel for every item the game describes (the lists, shops, the auction, trades, events ...):
  iteminfo::build wrapped by egui instead of paged, the marks as tags, the compared equipped item above it; it rides
  the combat window (retail's on the log). Seen live (2026-10-26): list, info, Use / Drop, drop, Auto-sort's
  question, hover, wheel, search.
- Suggestions (suggest.rs, tested; ids checked against LSB's spell list and effect enum): an ailment on a party
  member or us -> its -na spell; the lowest at 75% HP or under -> the Cure tier fitting the HP missing (three or more:
  Curaga); our Refresh, Haste, Protect, Shell, Regen, Phalanx, Stoneskin, Blink, Aquaveil or Reraise under 30 s ->
  renew it; TP 1000 engaged -> a ready weapon skill. Castable ones only, three at most, each with its reason; taken
  when a magic / ability window opens or changes and held while it is navigated. They head the category window
  and the list: Tab moves into them (Up / Down, Enter takes one through retail's keys, Tab / Escape back), a click
  takes one. Seen live as WHM 75: Poisona for our poison, Cure V at 29% HP, the pick cast Poisona.
- The party window as retail's ptwN ("Solo" outside a party): us first, then the members; per row the job, the
  leader marks, the name over the HP bar with the HP number in its hpcol colour, the MP bar and number, TP with
  config 0xae bit 0 (never on trusts), a member elsewhere greyed with the zone; the hand on the party cursor's or
  our target's row; a click targets. Bottom-placed with its exact height, growing upward. Two alliance panels at
  raid1 / raid2's places, down while retail hides them. Seen live with LSB bots (2026-10-26).
- Bars carry their label and value over their top edge (Inter Bold Italic, 18 pt, a black border, the label a
  little in from the bar's start). The locked target as retail's windowps 212: the alert frame, red arrowheads out
  of the side borders, "Locked" between red pointers on the bottom border. The cursor over the target stays.
- Art edits go through VectorCraft's MCP server (`vectorcraft-cli mcp`); the atlases are what the client loads, so an
  SVG change is rendered at 2x into its atlas cell.
- The retail party, alliance, target, log, chat line, effects, cast bar, compass and action menu are not drawn while
  Modern is on.
- Seen live (Lnxcsfix, 2026-10-26): every panel above.
- Party rows show the sub job small on the main job's corner.
- Synthesis and trade (modern/craft.rs): the slots as tiles beside one inventory list of every row; a click on an
  item puts it in the cursor's slot; the count and the gil asked inside the panel with buttons for their keys (gil:
  All and 0 outright). Synthesis keeps its menu, crystal list and history (favourites starred, Favorite /
  Unfavorite as buttons); player trade shows the partner's side. Seen live but player trade (no second player).
- Macros (modern/macros.rs): the forty books in one list, the palettes as keycaps with the sets as tabs, the editor's
  lines clicked to edit; Edit Equip. Set's pages as tabs, its slot editor as a grid of tiles beside the storage as
  tabs and one list; the palette while Ctrl or Alt is held over the chat window. Seen live.
- Rest ticks (ours, user 2026-10-27): resting, the party window shows a strip under its title: "Timing rest ticks..."
  until the first clean rest has been timed (config `rest_tick`), then "Next tick", a bar and the seconds left, on the
  server's own clock as learnt (client/resttick.rs). Seen live.
- Name plates (modern/plates.rs): the name in Inter Bold Italic with a dark outline, retail's size (capitals a little
  taller than retail's letters, set wide as retail's are; the user's pick, 2026-10-26), place, colours, target pulse and depth test (walls hide them; drawn in the same UI pass as
  quads). Each name is rasterized once per size bucket (16 / 28 / 48 / 80 px em) and uploaded as a UI image. Retail's
  badges stay at its size left of the name until the SVG badge set is approved; the master stars above. No bars.
- Edge windows (modern/extras.rs edge_window): the Timers, the minimap and Wide Scan stand on the screen's left or
  right edge (the place's x: 0 or above left, below 0 right), no border on that side, rounded corners and a double
  bronze line elsewhere; dragged they slide along the edge and change edge on the screen's other half; folded, a tab
  out of the edge with the name running up (left) or down (right) and a double chevron. Open, they keep clear of the
  command column and retail's windows at the edge; folded, those draw over the tab. T folds the Timers (Ctrl / Alt +
  T is /tell). The minimap is square, north up, with the zone and grid; Wide Scan sits under it, folded by default,
  named as retail's list, asking every 10 s while open and nothing is tracked, the last full list kept until the next
  is all in. Its filter (ours): Players / NPCs / Monsters toggles (config `modern_scan_kinds`) and a name field (a
  click takes the keys, every word must be in the name; Enter keeps it, Escape or the cross clears it). The map
  overlay draws in egui at its box and opacity.
- Retail windows while Modern is on: the pack's frame (render/ui.rs push_frame), the game UI's text in Inter through a
  glyph atlas (render/mtext.rs; game font, sprite font, labels), and the captions baked into menu sprites read back
  from their glyph cells ("font    font" and "menu    menu2fon") and drawn as text; the lobby's too when Modern is
  chosen.
- Config (modern/config.rs): retail's list without Windows, Misc. 1..4 as one page; Gameplay, Misc., Mouse/Cam.
  and Global drawn natively (rows from the pages' controls, named with their layouts' own captions, help lines);
  the list pages open retail's windows over it.
- Storage (modern/storage.rs): the bags as tabs, Mog Safe / Safe 2 and Mog Wardrobe 1..8 merged; the bag beside the
  inventory; a click moves a stack (right click one), into a merged bag the first part with room that takes it;
  retail's colours say what may move.
- Plate badges: the approved SVG set (assets/modern/badges) in place of retail's glyphs; the pearl tinted by its
  linkshell; rank stars, the recruiter star in its orb, the master stars above.
- Icons: batches 1 and 2 (131) redrawn in VectorCraft and approved.
- Status (modern/status.rs): the persona, the level gauge, retail's rows on the left with the page under the cursor
  beside them at once (profile with the attributes, job and master levels with the job icons, combat / magic / craft
  skills named by their sprites' captions, Currencies 1 and 2 as one list with "only those held"); Unity, Play Time,
  Merit Points, Job Points and Alter Ego Points open retail's windows over it.
- Journal (modern/journal.rs): Missions, Quests and Key Items as tabs of one window (Tab or a click switches): every
  log / area / category on the left with counts, the chosen one's rows on the right (a log's or an area's current and
  completed together), the cursor row's description under them at once; a new key item is marked seen by Enter or a
  click (no "Examine?" question); the Records of Eminence values and buttons under Quests open retail's lists. Typing
  searches every key item, quest or mission there is (listsearch.rs).
- Social (modern/social.rs): the Party menu, Search and Linkshell windows drawn from retail's own menus (their rows
  named by their label sprites, retail's disabled rows and swaps, a window opened over another shown beside it);
  link11 / link12 stay retail's. Set Bazaar the same way. View House is the storage view itself, opened on the Mog
  Safe, with every bag its menu allows as a tab (each opened as its row would).
- Chat timestamps are in the monospace face.
- The full log is the chat window itself at full height (under the top bar), with its tabs, scrolled by the wheel and
  the full log's keys; the chat line's channel is a tab on its top edge. A line sent on Say, Party, Linkshell,
  Linkshell 2 or Unity becomes the default channel (ours; retail sets it only by /chatmode and the Chat window).
- The game's windows that stack on the log stand on top of what rides on the chat window.
- Default places are the user's arrangement (2026-10-26): the status icons, the command window and the Timers at the
  left under the top bar, the main menu and the minimap at the right, the chat window most of the screen's width.
- Keys: Config's list takes Right as Enter (into the page); the Items sort strip is reached by Up from the first item,
  walked by Left / Right, left by Down.
- Keys follow the drawing (user, 2026-10-27): a retail window drawn as a column (MenuWin through social::column)
  records its rows top to bottom each frame and GameUi::menu_links steps Up / Down along them, wrapping, whatever its
  retail layout's links. The Journal has two columns: on the left Up / Down step the log, area or category (retail's
  Left / Right), Right or Enter go to the rows, Left or Escape back (World::journal_on_list; the cursor of the column
  without the keys dims). The macro book list, both pages as one column, runs Up / Down across them (Left / Right do
  nothing there).
- The treasure pool (2026-10-27, modern/loot.rs over treasure.rs): one panel docked on the action menu, the pool's rows
  oldest first (icon, name and count, the best lot and its lotter, ours or "Passed"), Lot / Pass on the cursor's row
  (lootope's choices, lit while it is open; once lotted our lot stands where Lot was), "Pass on the rest" (lnowin).
  Retail's keys stay (Enter, Up / Down, Num+ to lnowin); hover moves the cursor, a click on a button acts. The party
  window shows retail's chest (fontshp 0x7a) on its top edge while the pool holds items. Live with a bot in the party
  (a solo pool hands items out at once) and !addtreasure.
- A panel taller than the room under its place (a window dragged low, a long list) rises until it fits (user,
  2026-10-27: a list cut at the screen's bottom, its cursor walked into rows not shown); its saved place stays.
- Chat tabs as retail's full log (decoded 2026-10-26, FUN_101299f0 by the line's chat mode): All, Say, Tell, Party,
  Linkshell, Linkshell 2, Assist E, Assist J, Unity, Shout; each chat line keeps its mode (combat.rs ChatLog).
- The mouse, both interfaces (user, 2026-10-26: as modern MMOs; clicks unchanged): a drag holds the pointer hidden
  where it began (locked, raw motion; confined and warped back on X11); a right drag turns the camera and the character
  faces where it looks; first person looks with the mouse (0x8d / 0x8e invert it); the wheel moves the camera in and
  out (0.5 .. 3 of retail's 3..6 rest, saved as cam_dist), fully in into first person and out of it; no screen edge
  panning; Mouse/Cam.'s Mouse Control and Screen Edge Panning greyed in Classic, left out in Modern.
- Edge windows: dragged by the heading, the background or the tab, a see-through copy follows the pointer and the
  window goes to the edge on the pointer's half; the minimap resizes by the grip on its far bottom corner (the
  Extras page's minimap size and place rows, and the Timers' place, are left out in Modern).
- To do: the lobby screens' own layouts; BattleMod / rChat in the chat.
- The target panel's name in retail's name colour (FUN_1008db40 as the plates: unclaimed monster pale yellow, our
  party's claim red, another's magenta).
- Not yet: keyboard focus between panels, the target's cast, a way to reset places, alliance rows live.
