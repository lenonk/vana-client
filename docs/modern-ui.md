# Modern UI

The plan for vana-client's modern interface, from the user's planning document (Vana_Client_UI_Implementation_Plan,
26 October 2026), with the decisions taken since and the state of the work. Built on branch `ui-update`, merged into
master 2026-10-27 (kept).

## Decisions (2026-10-26)

- **Runtime toggle.** The retail UI stays the default and untouched. Settings > Client > Interface switches between
  Classic and Modern (config `modern_ui`). While Modern is on, the modern layer draws the windows it has and the
  retail windows they replace are left out; everything else stays retail until it has a modern counterpart.
- **egui.** The modern layer is egui (already in the client, on wgpu): windows, layout, focus, drag-and-drop,
  tooltips and text come from it; the pack's art is drawn through its painter.
- **Fonts bundled.** Fira Sans Regular (interface, chat, the retail windows' text), Fira Sans SemiBold (titles), Inter
  Bold Italic (name plates, the bars' labels, the party's names), Inter SemiBold (party names out of the zone),
  JetBrains Mono (timestamps, logs); no Cinzel (planned once, never loaded). Fira Sans replaced Inter for the interface
  on 2026-10-27 (the user's pick from nine faces rendered by `modern-shot --font`; plates and party names kept). All SIL
  OFL 1.1, unmodified, from
  their official releases, in `crates/vana-client/assets/fonts` with their licence texts, embedded in the binary.
  None has kana or kanji: a system Japanese font (Noto Sans CJK, Source Han Sans, Yu Gothic, Meiryo, Hiragino...;
  theme.rs `CJK`) is the last fallback of every family, for Japanese game text and chat (2026-10-28).
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
- **Combat:** a compact HUD (your HP / MP / TP and effects, target / focus / sub-target, party, cast bars, a restrained
  log) and a keyboard command palette (Attack, Magic, Abilities, Weapon Skills, Items, Check, Disengage ...): arrows
  move, Enter acts, Escape backs out; a separate fuzzy search ("has" finds Haste); recent picks remembered.
- **Spells and abilities:** one searchable action registry: categories, favourites, recent, job filters, costs,
  recasts, range, valid targets and why an action is disabled.
- **Recommendations:** a deterministic, testable rules engine (Poisona for a poisoned ally, Cure for low HP, an
  expiring Refresh, burst spells in a skillchain window, weapon skills at TP; RUN's rune for what it fights and
  when to spend runes; GEO's colures and luopan); never casts, never moves the cursor,
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

## The widget kit (modern/kit.rs, 2026-10-30)

Every control a modern page draws comes from the kit, and every page's keys go through its one key model, so like
controls behave alike everywhere. A fix to how a kind of control behaves goes into the kit, never into one page.

- A page lists its stops top to bottom: tabs, values (options, toggles, sliders), rows of buttons, lists.
- Up / Down move between stops (round the ends) and along a list; Left / Right change a value at once, walk a row of
  buttons, page a list or turn tabs; Tab / Shift+Tab turn a page's tabs from anywhere; Enter steps an option or
  presses; Escape goes back. A prompt's Escape is No.
- The mouse: hovering only lights a control and never moves a cursor; a click puts the cursor there and takes it; a
  drag moves a slider.
- One look per control: buttons (lit, off, the keyboard cursor's ring), list rows, sliders, tab strips, text fields,
  yes / no prompts, scroll bars that show whenever a list is longer than its room, and the cursor kept in view.
- Retail windows still drawn in the modern UI get the modern frame at the panels' size, standing out past the window
  with a margin.

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
- Text is never in the art; fonts are Fira Sans, Inter and JetBrains Mono (Cinzel was dropped with the first pack).

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

## State

Milestone 1:

- client/modern: `theme.rs` (palette, manifest, atlases as egui textures, the frames, fonts), `paint.rs` (windows of
  any size, nine-slices, gauges), `hud.rs` (the panels from views gathered from the World), `chat.rs`, `command.rs`,
  `combat.rs`, `mod.rs` (the layer, zoom, panels with saved places), `shot.rs` (`vana-client modern-shot
  [--backdrop PNG] --out PNG`: the panels with sample values and every frame, offscreen).
- egui's zoom follows the window height against 1080, times Settings > Client > Interface Size (config modern_scale,
  50 .. 150% in 5% steps, default 60%).
- Party: the members' rows, then retail's pet and fellow rows (by the classic party window: name, HP gauge, the
  pet's TP), a click targets them (2026-11-07).
- Panels: Status, Party, Target, Location (zone, grid, time, day, weather, and the compass: retail's disk turned so
  the camera's heading is up and tilted by cos 1.0996, N E S W, the radar dots, the tracking / call-for-help arrow
  with its target's name beside the grid label),
  the cast bar, Chat (tabs, the log, the chat line, the Enter key cap), the Command Menu (the main menu with
  submenus), the combat window (the action menu's rows).
- Every panel moves by a drag on its background (contents keep their clicks); places are screen fractions per
  character in config `modern_places`, saved when the pointer is let go. Only the chat window resizes, by the glyph
  in its corner: more room for the lines, nothing scales. Its settings button (the tabs' right) sets the font size
  (config `modern_chat_font`, 10 .. 32, default 15). User's call (2026-10-26): no lock button, no other resizing.
- The chat settings' Tabs (2026-11-06, issue #78): a button per tab, lit while it shows; a hidden tab leaves the row
  (its lines still show on All), All always stays, and the open tab going hidden opens All (config
  `modern_chat_hidden`, by name). Num+ skips hidden tabs. Tabs that don't fit before the settings button are left
  out, the open one always kept (those before it give way).
- The top bar: retail's titlewin, helpwind (marquee, busy strip, "Help" legend) and netstat (meter, %, S / R) in
  their places and by their fade rule (hudtop.rs fills a TopView), in the pack's frames on a layer that takes no input.
- The chat window runs the retail log's rule (logwin::LogAnim, shared with the retail log) on its wrapped rows:
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
  Escape ends it. Recent picks (2026-10-27): what is picked through the search is remembered per character (config
  modern_recent, newest first, 24 kept; a spell, ability, mount, emote, item, key item, quest or mission by its id or
  place) and ranks first among the matches (+400, less the older it is), marked "recent". Live: Potion over
  Hi-Potion for "potion" until Hi-Potion was picked, then Hi-Potion first, marked, and saved.
- Search in every list (2026-11-05, user: "type to search ... on every list box"; modern/search.rs): the same
  typing, field, hits, keys and recent picks in the other lists, each pick going through the list's cursor and
  Enter (what a click on the row does). Storage searches every bag the view can show (the bag as each hit's note;
  a pick shows its bag and moves the stack across); the Equipment list; shops (buy, sell, Set Prices, a bazaar);
  the synthesis crystals, history and picker; the trade picker; the Delivery Box's picker; the auction's item list,
  Sales Status and the inventory for sale; the treasure pool; Merit Points, Job Points and Alter Ego Points; the
  macro books, the equipment sets and the set editor's storage; the search results, Friend List, PlayOnline
  messages, Link List and Level Sync; the map's Wide Scan, Change Map and Markers; the Primer, the Linkshell
  Concierge, the Fishing Ranking, the block list, gardening and layout lists; Set Spells, the automaton's parts and
  the mannequin's storage. Left out: menus of commands (the main menu, the battle menu, the moogle's, Region Info,
  the party menu), the key assignment lists (a key pressed there is the assignment), display-only lists (the
  currencies page) and the Lua mods' windows. Twelve hits show; past that a line says how many more.
- The Items window (modern/items.rs) over items.rs's state: the list and the action menu's direct-use list with the
  game's icons, quantities and row colours, the count, a bar; itmsort2 as a strip (Auto-sort and its question, Manual
  Sort, the Recycle Bin), iuse (Use / Drop) and the drop question under the rows, Manual Sort's hint; the mouse as
  retail's, the wheel. Search there gives one hit per item with its total; a pick puts the cursor on its first slot
  and presses Enter.
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
- Suggestions (suggest.rs, rewritten 2026-10-29; user: "the suggestions should go at the top of the root battle
  menu, not in each subcategory", clickable and on the keyboard): up to four, each with its reason, retaken every
  half second while the cursor is on the commands. Raise, shadows, ailments, healing, bursting the chain made,
  closing the open resonance, Provoke / Flash for a monster on a non-tank, fight job abilities, Yonin / Innin,
  sambas, debuffs, opening a chain with a partner, the strongest weapon skill, renewing buffs (MODLOG "Skillchain
  helper and the battle menu's suggestions"). They head the battle menu: Up from the first command walks into them,
  Enter or a click opens the list on the action and takes it, into retail's target prompt (on the member a
  suggestion names). Seen live: Provoke, Berserk, a closing Fast Blade, an opening Red Lotus Blade. The bundled
  Battle Suggestions mod makes them (on by default; off on its Extras page): its rules are Lua since 2026-11-11, and
  the rows are the `battle_rows` slot, which any mod may fill.
- The skillchain helper (the Skillchain mod's edge window, its view a `lines` element): in a party, the open resonance on
  the target with its wait / window bar and closers, the chain made with its burst elements, and every chain the
  party can make with its elements and ways. Seen live with Naji and Ayame.
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
- Party and alliance rows show the main job's level on the job icon's lower right corner, as the status icons'
  counts (Utsusemi's shadows) are drawn (ours, user 2026-11-06).
- The experience bar (ours, user 2026-11-06): a slim bar centred on the party window's top edge between its corner
  ornaments, in a 4-point strip the window keeps over its frame (so what docks on the window sits on the bar). It
  fills with the level's EXP; once a master level is unlocked, its exemplar points; in merit mode, the limit points
  toward the next merit point. Blue while capped. Its tooltip gives the numbers, the percent and what is left. Mods
  hide it with `ui.native.hide("experience")`.
- Synthesis and trade (modern/craft.rs): the slots as tiles beside one inventory list of every row; a click on an
  item puts it in the cursor's slot; the count and the gil asked inside the panel with buttons for their keys (gil:
  All and 0 outright). Synthesis keeps its menu, crystal list and history (favourites starred, Favorite /
  Unfavorite as buttons); player trade shows the partner's side. Seen live but player trade (no second player).
- Macros (modern/macros.rs): the forty books in one list, the palettes as keycaps with the sets as tabs, the editor's
  lines clicked to edit; Edit Equip. Set's pages as tabs, its slot editor as a grid of tiles beside the storage as
  tabs and one list; the palette while Ctrl or Alt is held over the chat window. Seen live.
- Rest ticks (ours, user 2026-10-27; reworked 2026-11-06, issue #75): resting, the party window shows a strip under
  its title: "Timing rest ticks..." until a rest has shown two heals, then "Next tick", a bar and the seconds left, on
  the server's clock as learnt per server (config `rest_models`; client/resttick.rs, MODLOG "Rest ticks"): intervals
  that quicken to a floor are followed, each rise re-anchors the countdown. Seen live.
- Name plates (modern/plates.rs): the name in Inter Bold Italic with a dark outline, retail's size (capitals a little
  taller than retail's letters, set wide as retail's are; the user's pick, 2026-10-26), place, colours, target pulse and depth test (walls hide them; drawn in the same UI pass as
  quads). Each name is rasterized once per size bucket (16 / 28 / 48 / 80 px em) and uploaded as a UI image. Retail's
  badges stay at its size left of the name until the SVG badge set is approved; the master stars above. No bars.
- Edge windows (modern/extras.rs edge_window): the Timers, the minimap and Wide Scan stand on the screen's left or
  right edge (the place's x: 0 or above left, below 0 right), no border on that side, rounded corners and a double
  bronze line elsewhere; dragged they slide along the edge and change edge on the screen's other half; folded, a tab
  out of the edge with the name running up (left) or down (right) and a double chevron. Open, they keep clear of the
  command column and retail's windows at the edge; folded, those draw over the tab. T folds the Timers (Ctrl / Alt +
  T is /tell). The Timers' recasts are rows with bars; its buffs and debuffs a line per bearer (user, 2026-11-01: rows
  ran too long in a full party): the name in a column, then each effect as its icon with the time under it (m:ss under
  10 minutes, then retail's "24m" / "1h"), wrapping with the window's width; a cell's tooltip names it. The minimap is square, north up, with the zone and grid; Wide Scan sits under it, folded by default,
  named as retail's list, asking every 10 s while open and nothing is tracked, the last full list kept until the next
  is all in. Its filter (ours): Players / NPCs / Monsters toggles (config `modern_scan_kinds`) and a name field (a
  click takes the keys, every word must be in the name; Enter keeps it, Escape or the cross clears it). The map
  overlay draws in egui at its box and opacity.
- Retail windows while Modern is on: the pack's frame (render/ui.rs push_frame), the game UI's text in Fira Sans through a
  glyph atlas (render/mtext.rs; game font, sprite font, labels), and the captions baked into menu sprites read back
  from their glyph cells ("font    font" and "menu    menu2fon") and drawn as text; the lobby's too when Modern is
  chosen.
- Config (modern/config.rs): retail's list without Windows, Misc. 1..4 as one page; Gameplay, Misc., Mouse/Cam.
  and Global drawn natively (rows from the pages' controls, named with their layouts' captions, help lines);
  the list pages open retail's windows over it.
- Storage (modern/storage.rs): the bags as tabs, Mog Safe / Safe 2 and Mog Wardrobe 1..8 merged (only the parts the
  account may use: no Safe 2 without the 2F); the bag beside the inventory; a click moves a stack (right click one),
  into a merged bag the first part with room that takes it; retail's colours say what may move. A move made while the
  last is pending waits for it, and the send gate holds it (one a second) rather than dropping it. The moogle's Storage row opens it at once on the first open bag (no container
  list); closing it comes back to the moogle's menu. Each pane has Auto-sort / Manual Sort (Num+ reaches them):
  Auto-sort sorts its bag (every part) and keeps it sorted as items come in (config auto_sort, saved); Manual Sort
  stops that and puts rows in order by hand (pick a row, then the row to put it at, within one part of a merged bag;
  Escape or Manual Sort ends it).
- Plate badges: the approved SVG set (assets/modern/badges) in place of retail's glyphs; the pearl tinted by its
  linkshell; rank stars, the recruiter star in its orb, the master stars above.
- Icons: batches 1 and 2 (131) redrawn in VectorCraft and approved.
- Status icons (2026-10-27): one of ours for each distinct retail image (193 for 598 status ids; Signet split from
  KO's skull, Signet / Sanction / Sigil / Ionis one crystal in blue / red / green / yellow), drawn in VectorCraft from
  `status-dump`'s stock icons, the bare set (user's pick): atlases/status_0@2x.png, manifest "status/s<id>",
  status_map.tsv (status id -> icon); Modern::status_icon draws them everywhere (effects, party rows, Timers, Level
  Sync), the game's icons only for an id without one. Sources ~/ffxi/lab/scratch/uiupdate/vc/status.
- Spell and ability icons (2026-11-07): one of ours for each distinct retail list picture (the HD set, 688 for 2383
  named spells and abilities), drawn in VectorCraft from `action-icon-dump`'s stock pictures and approved batch by
  batch. Abilities sit on a tile in their job's colour (SP abilities framed in gold), weapon skills show the weapon on
  the element's colour, spells a disc in the element's colour (light: holy gold); retail's tier numerals and area star
  are redrawn; the 66 standard images (songs, ninjutsu, blue magic, Geomancy, the generic ability tiles) one per
  school and element. atlases/actions_<n>@2x.png, manifest "actions/<name>", action_map.tsv (list picture: magiconw N
  as N, magico2w N as 1000 + N -> icon); Modern::list_icon / row_icon draw them in the magic and ability lists, search,
  the battle menu and its suggestions, favourites, Timers, the BLU set and mods' windows and HUD icons; the game's
  icon for a picture without one.
- Weather icons and target markers (2026-10-27, bare): weather/w00..w19 by weather id (the location panel shows one
  icon, Clear and the skies too, a double weather drawn as a stronger icon); the target arrow, subtarget arrow
  and the five relation rings redrawn under their indicators/ ids; indicators/party_invite_pending in place of
  retail's keytops3 69 on the party window. Atlases weather_0 / markers_0. Sources vc/weather, vc/markers.
- Status (modern/status.rs): the persona, the level gauge, retail's rows on the left with the page under the cursor
  beside them at once (profile with the attributes, job and master levels with the job icons, combat / magic / craft
  skills named by their sprites' captions, Currencies 1 and 2 as one list with "only those held"); Play Time shows on
  the page; Unity, Merit Points, Job Points and Alter Ego Points open retail's windows over it.
- Journal (modern/journal.rs): Missions, Quests, Records of Eminence and Key Items as tabs of one window (Tab or a
  click switches): every log / area / category on the left with counts, the chosen one's rows on the right (a log's or
  an area's current and completed together), the cursor row's description under them at once; a new key item is
  marked seen by Enter or a click (no "Examine?" question). Records of Eminence shows the totals, then retail's lists
  (Current, Objective List, Vana'Bout) in place, the rows and the description in retail's colours (the name as its
  row, the count blue when complete or at its goal, the once-only note); in a category the sibling ones are a Left /
  Right away, or a click on the arrows by its heading (retail's children 9 / 10). Typing searches every key item, quest
  or mission there is (listsearch.rs).
- Social (modern/social.rs): the Party menu, Search and Linkshell windows drawn from retail's menus (their rows
  named by their label sprites, retail's disabled rows and swaps, a window opened over another shown beside it);
  link12 is the People member list and link11 (naming a new shell) a modern window. Set Bazaar the same way. View House is the storage view itself, opened on the Mog
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
- The treasure pool (2026-10-27, modern/loot.rs over treasure.rs): one panel docked on the item info of its cursor's
  row, which rides the action menu; the pool's rows oldest first (icon, name and count, the best lot and its lotter,
  ours or "Passed"), Lot / Pass on the cursor's row (lootope's choices; once lotted our lot stands where Lot was),
  "Pass on the rest" (lnowin). Keys (2026-10-31, user: "No way to lot or pass with the keyboard"; retail's two-step
  lootope with Up / Down was hidden behind side-by-side buttons): the cursor's row always has a lit choice (Lot, or
  Pass once lotted), so Enter acts at once; Left / Right pick Lot or Pass; Up / Down the rows and, past the last, Pass
  on the rest; Escape closes the pool; Num+ still goes to Pass on the rest. A click on a row moves the cursor, a click
  on a button acts. The party
  window shows retail's chest (fontshp 0x7a) on its top edge while the pool holds items. Live with a bot in the party
  (a solo pool hands items out at once) and !addtreasure.
- Shops (2026-10-27, modern/shop.rs over shop.rs): NPC and guild shops and Set Bazaar's prices in the command
  window's place: Buy / Sell as tabs (shopmain / guildsho, a click on the other tab leaves the list and opens it), the
  whole list (icon, name in retail's tint, the requirement's words, the price or count), the quantity (itemctrl's
  buttons), the price (moneyctr's) and the confirm ("Buy 2 x Name for N G?", shopbuy / shopsell's captions) inside the
  panel. Live at Capucine: buy by mouse, appraise and sell by keys, Escape back out. Guild shops and Set Prices not
  seen live yet.
- The auction house (2026-10-27, modern/auction.rs over auction.rs): its window stack side by side in the command
  window's place (the last three): the command menus as the Social pages draw retail's (auc1, auc2 and the category
  trees, auc3 / auc4, the sort menus), the item list (icon, name in retail's tint, stack forms "x12", "[n]" listed or
  the sale's price), the price history (date, seller → buyer, price) and the inventory for sale; the bid or sale price
  (moneyctr's buttons) and shopsell under them; the prompts are the modern confirm. Live: Bid → Weapons → Katana, auc3,
  Price History, Bid's price (cancelled), Sell, Sales Status, by keys and mouse. No bid or sale made.
- The Delivery Box (2026-10-27, modern/post.rs over post.rs): in the command window's place (the moogle's menu waits
  under it, not drawn). Receive: post1's eight slots as tiles, the cursor slot's sender, post2's three choices as
  buttons (Return off for an AH sale). Send laid out as retail's delivery and stringdl (their links decide the keys):
  the recipient field with OK / Exit under it, the gil field over the slots, OK (send) and Cancel (close) right of the
  two rows, the inventory picker beside while a slot asks for an item; the count and the gil inside the panel. Live:
  Receive (empty box), Send with a recipient typed and accepted, Gil's amount asked and dropped, Cancel back to the
  moogle; nothing sent.
- /check on a player (2026-10-27, modern/inspect.rs over inspect.rs): the name, the help bar's jobs line, the sixteen
  slots in retail's grid (child 1 + column x 4 + row, so the links walk it as drawn), the linkshell's tinted icon and
  name, View Wares and the bazaar message under it while it has the cursor. It opens on View Wares as retail's; a
  disabled button with the cursor shows it dimmed (craft::button). Live: a bot checked, arrows, hover and click with
  the item info following.
- Merit Points, Job Points and Alter Ego Points (2026-10-27, modern/points.rs over merits.rs, jobpoints.rs and
  aepoints.rs; their rows as merits::PointRow): in the command window's place, Status's page hidden under them. The
  limit or capacity gauge and the points in the title; merit1 / merit2 as columns (their links walk them as drawn), the
  lists beside with every row (level / most, NEXT and the cost in retail's colours; a category's spent / cap; a job's
  two groups under their captions with their totals), the cursor row's description under them (bluehelp), merit3 and
  sortyn as buttons, the raise asked by the modern confirm (merityn: Escape is the error sound, as retail). Job Points'
  two pages as tabs. Live on Lnxcsfix (WAR 99 with Limit Breaker, Job Breaker and the Cipher Bracelet for the test,
  restored after): every list, Raise up to the confirm (declined) in the Mog House, an alter ego row by the mouse.
- Unity (2026-10-27, points.rs over unity.rs): ut_menu as a column with the accolades in the title, quest00 beside it
  (Unity Information's lines and the chat setting with a dot on the current one, or the two weeks), quest01's ranking
  of every unity under its rows; Unity Lists opens the area list's panel. Live: both pages, a week's ranking, a
  row and Unity Lists by the mouse.
- Level Sync (2026-10-27, social.rs over levelsync.rs): the party's members as the search results' people rows beside
  the party menu, Level Sync's status icon on the synced one, grey under level 10 or anonymous.
- The block list and the chat mode window (2026-10-27, modern/minor.rs over blklist.rs and chatctrl.rs): the names
  with blkmain's Delete as a button while it is up; chatctrl's seven rows as a column on the combat window with the tell
  name, the shells ("No Linkshell") and the Unity at the right, subwindo's range under them. Retail's engaged gauge
  window (gaugewin) is not drawn: the party panel shows our HP / MP / TP. Live: a name added, picked by the mouse and
  deleted by the button; the chat mode window from the action menu's Chat arrow; Level Sync with a bot in the party.
- The comment editor and the Linkshell Concierge (2026-10-27, minor.rs over seacom.rs and concierge.rs): the editor's
  three lines (the edited one live, a click picks a line, OK closes as Escape does) on the combat window as retail's
  sits on the log; the concierge's event title, lscswin's four filters as tabs (clickable at any time: the filter's
  focus and Enter), the header and every shell with its icon in the shell's colour, green for ours, the language and
  the members wanted. Live: Set Bazaar's Edit Comment typed and erased (nothing sent); the Bastok Mines concierge with
  two seeded rows (removed after), the English tab, Escape back to the event.
- Mog House Remodel (2026-10-27, minor.rs over remodel.rs): its five rows in the command window's place (the moogle's
  menu waits under it), the patio grey without its key item. A pending party invite: keytops3 69 centred on the party
  panel's top-left corner as retail's (hud.rs; not seen live: bots don't invite). Live: /remodel on Lnxcsfix (char_stats
  mhflag 0x20 for the test, restored after), the rows by keys, Escape; nothing sent.
- The Adventuring Primer (2026-10-27, minor.rs over primer.rs): guide00's categories and guide01's topics side by side
  in the command window's place (Help Desk's faqsub waits under it), guide01's arrows as < / > buttons beside the
  category's name; a topic's page stays the map window's (mode 7, 2x by default as retail's), the lists hidden while it
  is up. Live: /primer, a category, a topic's page and back, > to the next category, a row click opening its page.
- Set Spells (2026-10-27, modern/bluset.rs over bluset.rs): one panel in the command window's place (the magic list
  waits under it, as retail's window walk suspends it): the points used / total in the title, the slots (their numbers
  in the band colours, the spell's icon and name) beside the known spells (Type, Remove All, every spell in its tint:
  picked orange, set green, too dear or too high grey), the cursor spell's help under them; Refine by Type's families
  (the layout's captions) in the spells' place while it is open. A row of the other list takes a click while
  nothing is picked (its focus moves first, as Num+). Live on Lnxcsfix as BLU 75 (back to WAR 22 after): the lists,
  the help, Undead filtered, a spell picked and dropped; nothing set.
- Layout mode's windows (2026-10-27, minor.rs over layout.rs): the rmlo window as a column with its dim rows, bankmenu
  beside it, the furniture list as the item list (installed here green, locked grey) in their place; the title is
  layout mode's (title slot 0). Live: /layout in the Mog
  House, rmlo1, bankmenu (Mog Safe 2 dim), the list (empty safe). The mannequin's dressing (inspect.rs dress over
  mannequin.rs) uses /check's grid: the worn pieces, the slots it can't wear shaded, Remove All under it (dim with
  nothing worn), the Storage list beside it in its tints; the side without the keys takes them on the mouse first, as
  retail's pointer (not seen live: Lnxcsfix has no mannequin now).
- pupequip and the fishing gauge (2026-10-27, minor.rs over equipwin.rs and fishing.rs): a PUP's Equipment row opens
  pupequip as a column in the command window's place; while fishing the fish's gauge is a labelled bar ("Fish", grey
  at 0) at retail's place, the screen's centre at 0.2 H (not seen live yet). Live: pupequip on Lnxcsfix as PUP 30 (back
  to WAR 22 after), Master by the mouse opening the Equipment screen.
- Event choices and the Mog House door menu (2026-10-27, modern/mod.rs log_list): stacked on the chat window as the
  confirm is, the question and every option with the log's in-line colours (the concierge's green "linkpearls"), the
  cursor's lit and kept in view; hover moves it, a click picks; the keys stay retail's. The door menu's rows come from
  client/mod.rs mogdoor_rows, shared with the retail draw (not seen live: LSB gives Lnxcsfix's house no exit bit, so
  the door asks Yes / No). Live: the Bastok Mines concierge's choice by keys and mouse, Escape cancelling.
- An event's text input (passinpu): a panel stacked on the chat window, its title (group 0xb by kind) or "Password:"
  and the text with its caret; the keys stay event.rs's (not seen live: no event on hand asks for text).
- UI Setup (2026-10-27 as Panel Places, settings.rs, Settings > Client, in game only): Reset drops this character's
  saved panel places (config modern_places) and saves; every panel goes back to where it starts the next frame
  (positions come from the places each frame). From <character> <job> (2026-11-10) copies another kept setup in (its
  places, mode presets and chat window settings; this job's favourites stay). Both replace this job's windows, so
  stepping the slider only picks ("Reset?", "From Nine WHM?"): Enter, a click or letting go of a drag does it, and
  Escape drops it. The row reads As Moved again. Live: the Settings panel dragged away, Reset by keys and by the
  mouse; From Lnxcsfix WHM on WAR folded WHM's minimap.
- Setups per character and job (2026-11-10, client/uiconf.rs; issue #94): each character's main job keeps its
  window places (every display's), folded edge windows, mode presets, chat window settings and favourites, in
  <data>/ui/config/<World>-<Name>/<JOB>.json (beside an FFXI_CONFIG copy in test runs); the config's fields hold the
  one in use, loaded when the character or the main job changes (MON in Monstrosity) and written with every save. A
  job new to the character starts as the one it comes from (this session's, else the character's latest file) with
  no favourites (a job's spells and abilities, as the hotbar's bars, which the hotbar keeps by job itself); a
  character's first job takes the config as it was. Lua mods' data stays in mod-data (per character, as before);
  which mods run can be kept per character and / or per job from each mod's page (docs/lua-mods.md 3.4.1).
  Live: WAR made from the config, WHM and RDM from WAR, a folded minimap kept to WHM through !changejob both ways.
- The target's cast (2026-10-27, not retail: combat.rs track_cast, World.casts; hud.rs target_cast): 0x028 category 8
  with "ca" starts a spell, 7 with "ca" readies a skill (the first result's value: a mob skill from 0x100, else a
  weapon skill); another arg there is the interrupt; categories 3 / 4 / 11 end it. The target panel shows a spell's name
  over a bar by its base cast time (the record's; others' speed is not sent; gone 2 s after it should have landed) or
  "Readying <skill>" for 10 s at most. Live: Cure on ourselves as WHM 30 (the bar, gone on landing); a Forest Hare's
  Foot Kick logged ready (value 257) and finished 0.4 s later, too quick to catch on a shot.
- The panel with the keys (2026-10-27, modern/mod.rs focus_panel): worked out each frame as the key dispatch routes
  them (the prompt, the chat line, an event's choice or input, the magic / ability lists, /check, the comment editor,
  the chat mode window, the treasure pool, the people lists, the windows the menus open, the main menu or its
  submenu, the action menu); that panel draws in the pack's window_focused frame (blue edge), the rest in their usual frames.
  Live: the main menu, Status, the chat line while typing, the action menu.
- Character select (2026-10-27, modern/lobby.rs over select.rs, which keeps every rule and key; select::Input.modern
  skips the retail draw and mouse): over the 3D scene, a list of every character (the name in its colour, the job and
  level, the race, the area), the cursor's details (World / Race / Job / Level / Area by group 1's labels), the counts,
  a Settings button (chfwin, as Num+ or the right button), chfwin and chswin beside it, the prompt (ptc6yesn), the ok
  window and the rename field below; the mouse as retail's (a row moves the cursor, a click is Enter). Live: the list
  by keys, the login prompt (No), Settings by the mouse.
- The title's buttons (2026-10-27, modern/lobby.rs title_buttons over render/screens.rs TitleState.modern): the pack's
  buttons on retail's rects (loby2win's backing and button sprites skipped), the cursor's lit, the disabled dimmed,
  retail's keys and mouse untouched (hover moves the cursor, a click is Enter). The Update button moved to the login
  form (2026-11-09).
  The labels are sized to the buttons (half their height; user, 2026-10-27: the panel-sized font was far too small). The
  logo, the expansion banners and the help bar stay. Live: hover, Select Character by the mouse, back by Escape.
- The login screen (2026-10-27, modern/lobby.rs login_form / ok_popup / wait_panel / help_bar over client/mod.rs
  login_window, which keeps the keys): the form at 1.5x in the pack's input, checkbox and button art; the wait and
  error popups on retail's netwait / ok rects with their text above the form's size (user: as big as the original's
  or bigger) and OK centred on retail's row, lifted clear of the form; the help bar on retail's lobyhelp strip with
  the version above it at loby2win's spot (title screen too). Character select's wait is the same panel. Update
  (2026-11-09, while something is out of date) at the right end of Remember password's row, in the lobby button art.
- Panel Opacity (2026-10-27, Settings > Client, config modern_opacity, default 85 %): every window background the layer
  paints (the frame gradient, the other border styles' fill, the edge windows, the auto-translate list); borders and
  text stay opaque.
- The creator (2026-10-27, modern/lobby.rs creator_screen over client/creator.rs and client/mod.rs, which keep every
  step's rules and keys): one panel where the select list sits, the step's help line and "n/7"; the step's choices as
  rows (the race by its group 6 name, Face / Hair as the stats line's words, the size, job and nation rows'
  captions); on the name step the name field, Random (Num+) and OK (Enter), then the world list (worldsel) and the
  pass (chmkpass) with its OK; up to the name the race, its STR..CHR for the job and the Face / Hair / Size line; on
  the nation step the nation's title and description (they were centred on the screen). The confirm (ptc6yesn) and
  the ok windows as the select screen's prompts; the race title and the script stats are not drawn, the nation
  emblem stays. The mouse as retail's (a row moves the choice, a click is Enter). Live: race by hover and click, the
  steps by keys, Random, a typed name, the world and the pass by the mouse, LSB's name check, the nation, the
  confirm's No, Escape out to the title.
- The Battle tab (2026-10-27, not retail; the BattleMod / rChat way of keeping battle text apart): battle messages
  (combat.rs print) are tagged BATTLE_MODE; the chat's Battle tab shows them alone, and the chat settings' "Battle in
  All: Shown / Hidden" (config modern_battle_in_all, Shown by default) takes them out of All. Retail's Chat
  Filters (Config, 0x32 / 0x33) still decide which battle messages print at all. The settings popup opens above the
  gear when the chat window sits in the screen's lower half (it opened off-screen before). Live: Cure and a say line,
  the Battle tab with the cast lines alone, Hidden leaving All to the chat.
- From rChat (pChat in ESO; 2026-10-27): unread counts on the chat tabs (Say ... Shout; not All or Battle): the lines a
  tab shows since it, or All showing them, was last on screen, "9+" past nine; a line from another player naming you
  (a whole word, any case; retail's incoming modes only, never the echoes of our lines) gets a gold wash and a gold bar at its
  left. Live: two /say lines on the Battle tab counted on Say, cleared by opening it. The log across logins: the
  session's last 200 lines saved at logout (the data dir's chat/<name>.json, or beside an FFXI_CONFIG copy) and shown
  dimmed under "------ last session ------" at the next login (modern UI only; restored lines are not saved again).
  Find in the full log: a letter or digit typed while it is open filters it to the lines holding the text (any case;
  the field and the count at its top), Backspace edits, Escape clears the find, a second Escape closes the log. The
  full log now draws over the edge windows. Live: "sand" left the 6 lines with it; history over two logins.
- A click on a panel that does not move it leaves its place alone (egui starts a window's drag on the press: a click
  on the location panel while the chat folded saved it undocked, and it stopped riding the chat). Live: a click keeps
  the dock, a real drag moves and saves it.
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
- Edge windows and the menu (user, 2026-10-27: the main menu, the minimap and an open Wide Scan swapped places every
  frame; then "the menu should just draw on top of edge windows"): the edge windows (Timers, minimap, Wide Scan) no
  longer step aside for anything; the menu column (command, its submenu, the command window) is kept on top of them
  (move_to_top each frame). Nothing docks on an edge window (their left moves as they fold, and a dock on Wide Scan's
  tab made the menu jump 270 px when it opened), and a dock saved on one gives way to the panel's default place. A dock on
  a panel placed from the right counts from its right side (ModernDock.right). Retail's strip right of its minimap
  is not kept clear. Live: the user's layout copied in, frames alike, the menu over the minimap.
- Row look (user, 2026-10-27: the framed rows "too busy", but not plain; Gilt picked from five rendered by
  `modern-shot --rows`): every list and menu row goes through rows.rs (face for a row face, menu_row for the main
  menu): bare rows, the cursor between two gold rules fading at the ends with a gold diamond and gold label, a faint
  gold wash on hover, thin chevrons for submenus. The framed art stays as Style::Framed. The text fields that had the
  selected row's art (the rename field, the event input) use the input art. The action menu (combat.rs, on button
  art before) draws its rows the same way, the submenu mark a chevron that still takes the Right click; Gilt's
  diamond sits in the row's left margin, clear of every list's icon; list labels are gold under the cursor.
- The Num+ status cursor (user report 2026-10-27: no box on the picked buff): the modern status icons box the cursor's
  icon in retail's orange-red (anc_equi), and Up / Down move by the panel's 10-icon rows (retail's 9 while classic).
  Live: Num+ twice boxes Protect, Right moves it to Shell, the top bar names it.
- Condensed battle lines (2026-10-27, not retail; BattleMod's condensing): with the chat settings' "Condense: On"
  (config modern_condense, On by default), a 0x028 action's record lines that differ only in their number merge into
  the first one as it grows ("Lnxcsfix hits the Wild Rabbit for 18, 14 points of damage."), lines with no number into
  a count ("misses the Wild Rabbit. (x2)"). The action is the packet (Action::seq, numbered in on_action), so a melee
  round's later swings still join it; a line whose text is not its template with the number in is left alone
  (combat.rs condense / template_of). The retail log wraps again when a line changes (ChatLog::edits). The chat
  settings popup now holds the chat window open (it folded away with the popup under the mouse). Live: MNK rounds of
  two hits as one line, Off giving one line a hit. Then (2026-10-27, BattleMod's sumdamage, condensecrits and
  cancelmulti): a merged line gives the sum with the hits after it ("for 33 points of damage (18 + 15)."), a critical
  hit joins its round's hit line marked "!" (alone it stays retail's two lines), and a system or battle line equal to
  the one before counts on it ("The Wild Rabbit is out of range. (x2)"; players' lines never fold). With the split
  rounds the target is held while its round has swings left (combat::held), so its death and "defeats" no longer
  print before the killing swing (seen live before the fix). An action on several targets (BattleMod's
  condensetargets): each target's line that differs only in the name and number joins the first in the message
  table's plural ("Nine, Raven, and Koru take 50, 48, and 45 points of damage."; Who.plural), each name with its
  article; a possessive goes on every name ("Lnxcsfix's and Aurelie's attacks are enhanced."); a value the line
  names (a status id) rather than prints is kept as it is. Live: Warcry with a bot in the party.
- The target panel's name in retail's name colour (FUN_1008db40 as the plates: unclaimed monster pale yellow, our
  party's claim red, another's magenta).
- Cursor on movement (ours, not retail): Settings > Misc. > Keys Hide the Cursor, off, keeps the mouse cursor shown
  while you walk with the keys. On, the default, hides it as retail does.
- The first letter typed into a list's hidden search no longer also triggers its game hotkey.
- Settings window search (ours, not retail): typing on a topic page searches every page's settings by name, help
  and heading; a Retail only button (Ctrl+R) hides our settings; a details column shows the setting under the
  cursor. Mods and Mod Settings show only their tabs. This finishes what the 2026-11-04 Settings window entry
  left to come.
- Settings window (ours, not retail): the modern Config window is now "Settings", with every retail Config setting
  and every setting of ours (the old Settings and Extras pages) on one of fourteen pages, grouped as Game, Graphics,
  Sound, Input and System, each page in small headed groups. Chat Filters, Effects, Font Colors, Mods and the Lua
  mods' settings open as pages from a button on their topic's page.
- Typing in the Settings window's search no longer triggers game hotkeys (ours).
- Mouse Camera Speed (ours, not retail): Settings' Mouse row (Input > Mouse, Camera & Keys, and the classic Settings
  Controls page) scales every mouse look drag, 25% to 200%, 100% the old speed. The controller's camera is not touched,
  so it stays retail's.
- Ours: a Game Master window (Command Menu > Game Master), with config `gm_window` set in config.json by hand (off;
  no Settings row) and a private server whose GM tools answer the 0x1F0 request with a GM level above 0. Groups, a search by name, 9
  commands a page (Up / Down, Left / Right), a form per command, runs over 0x1F1 with the server's printed reply in
  the window. Its fields and search are typing, so they trigger no game hotkeys. Not retail (retail has no GM menu).
- Pickers: while a field is being typed in, up to 6 matches show under it (id or name, names starting with the text
  first); a click fills the id. Zones, items, spells, key items and titles come from the game's name tables; jobs,
  weather, skills, logs, nations and true/false are fixed lists; player fields offer you and your party; entity
  fields offer the named things in the zone. A typed id shows its name under the field.
- The action registry (2026-11-02, ours; plan D): client/registry.rs gives, for a spell or an ability, its valid
  targets (the DAT bits by the prompt's kinds), its range (spell +0x45, table 0x103644a8), the current target's flat
  distance when the action may take it, and why it can't be used now (retail's group 7 refusal line from
  magic_check / ability_check, else the effect that stops it: asleep, silenced, amnesia ...). The magic and ability
  lists show it under the cost box ("Range 20 · Self, Party · Target 12.3"), the reason in red, and "Target out of
  range" past range + 0.5 (the prompt's red arrow). Range only warns: the row greys by retail's checks alone.
- Alliance rows offscreen (2026-11-02): `modern-shot --alliance` draws both alliance windows with sample rows built as
  party_rows builds them (no MP bar, the alliance leader's blue mark, a member out of the zone, a trust without TP).
  Seen right there; the shot also learns the GPU's texture limit first now (the 2184-wide status atlas failed under
  egui's assumed 2048).
- "Not yet: alliance rows live" still stands: it needs a second party of bots.
- Auto-translate in fields (2026-11-02, ours): Tab completes the word before the caret from the auto-translate
  dictionary in the macro editor's title and lines and in the comment editor (search comment, bazaar message), as on
  the chat line: the same list (Tab / Space / arrows / Page keys move, Enter puts the pick in, a category opens its
  phrases, Escape or Backspace backs out), drawn at the field, with the Modern UI on (the classic UI keeps retail's
  keys there). A phrase goes in as one unit within the field's bytes (6 of them); typing closes the list. The comment
  lines show phrases written out.
- The top bar's busy strip also runs while a PlayOnline message is being sent (retail's bit 0x10, set by msgline's
  commit: a message, a reply, a friend request, an accept or a decline; not by /befriend's search or Ignore;
  2026-11-02).
- Command palette (2026-11-02, ours; listsearch.rs, modern/lists.rs): Ctrl+Space in the field searches every spell,
  ability, usable item and action menu command (Attack, Disengage, Switch Target, Check, Call for Help, Heal); recent
  picks show on the empty field; a pick goes through the menus as the keys would, and only the commands the action
  menu offers now are listed. Ctrl+Space is a placeholder: which keys open the palette and the search is still open.
- Title rows (2026-11-02, ours; modern/mod.rs): a note no longer runs under the title. The Commands window's target
  name breaks in two lines as retail's target window breaks a name (FUN_101c9fa0), the second cut with an ellipsis;
  other notes are cut; a long title is cut and leaves the note at least 40% of the row. `modern-shot --titles`.
- Right-click menus (2026-11-02, ours; the pack's "right click a context menu"): one look for every menu
  (ctxmenu.rs popup, the chat window's Copy / Paste menus now use it too): an optional heading (the name), rows lit
  gold on hover, grey when they can't be picked now, closed by a pick, a press outside or Escape. An Items window row:
  Use, Drop, Sort, Move to... (the action menu's direct list: Use); they set the row's cursor and press iuse's or
  itmsort2's Enter, so Drop and Sort still ask. Move to... lists the bags open away from the moogle that take the
  item by the storage view's colours (Satchel, Sack, Case, the wardrobes; gear only into wardrobes, nothing equipped
  or in the bazaar) and sends 0x029 with the stack. A party or alliance row: Target, Check, Tell (the chat line on
  "/tell name "), Focus Target (our party's slots 1..5, as retail's party menu). An actor: a right click on the
  world that did not drag (client/mod.rs, World::actor_menu) opens its menu in place of retail's right click (which
  clears the target); on nothing retail's right click stays. A monster: Target, Attack, Check; a player: Target,
  Check, Tell, Invite to Party, Trade; an NPC or a door: Talk, Target, Trade. They run the text commands (/check,
  /attack, /pcmd add with the server id or name) and trade::start_on, so the refusals are retail's.
  `modern-shot --menus` draws three of them.
- Layouts per display (2026-11-02, ours; plan B): config `modern_places` keeps a layout per character and display
  ("Name @2560x1440 60%": the window's pixels and the UI scale) beside the character's latest one (the plain name).
  A display not seen yet starts from the latest; a change saves both; a display shown for 2 s keeps that layout as
  its layout, so arranging another display doesn't move its panels, and a window dragged through sizes stores nothing.
  UI Setup's Reset clears every display's. No other panel resizes.
- Favourites (2026-11-02, ours; the plan's "favourites" and "a small optional favourites strip"; client/favs.rs,
  modern/favs.rs): spells, abilities (not traits) and items are starred by the star at their row's right end in the
  magic and ability lists, the Items window and the search's hits, or with Ctrl+F on the cursor's row or the
  highlighted hit. Kept per character in config `modern_favourites` (the search's keys, "m:" / "a:" / "i:"), 24 at
  most. In the list search a favourite ranks above every other match and says "favourite". The favourites strip
  (panel "favourites", movable, over the chat window's right end by default) shows a tile per favourite: its icon, an
  item's count, greyed as its list greys it, its name as the tooltip. A click runs it as a macro line would: /ma,
  /ja, /ws or /pet, /item, with <me> when only we are a valid target and <st> (the target prompt) otherwise. /favs
  [on | off] shows or hides it (config `modern_fav_strip`); it hides while nothing is starred. `modern-shot --favs`.
  Not yet: macros in favourites (the open question "How macros and equipment sets show in search and favourites"
  stays open). The strip and /favs are the Modern UI's only.
- Focus and last target (2026-11-02, ours): a panel on the target panel with the focus target (retail's
  /focustarget, a party member) and the last target (the one before the current), each with HP; a click targets it.
  In modern, `<lt>` names / targets the last target and `<ft>` is `<focust>`.
- Help Desk reply mark (2026-11-02, retail; hudtop.rs, modern/topbar.rs): while a Help Desk reply waits, the top
  bar's mail page shows retail's reply mark (netstat state 2, windowps 36 and keytops3 118, count 1) and the network
  page isn't held; the modern top bar shows "Help Desk reply". Engine +0xe's count never counts in this build
  (MODLOG "netstat: the second count").
- Pending actions (2026-11-02, ours; the plan's "state changes are server-confirmed" and its open question on latency
  and invalid targets; client/pending.rs, modern/pending.rs): a spell, weapon skill, job ability or item sent to the
  server (0x01A from /ma, /ws, /ja, /pet; 0x037) is pending until it answers. A chip over the cast bar (panel
  "pending", movable) shows "Cure III -> Raven  waiting..." once the wait passes 150 ms (a quick answer never flashes
  it). Our 0x028 of its kind confirms it (the chip goes; the cast bar and the effect take over). A 0x029 of ours before
  that, within 2 s of the ask, is the refusal: the chip turns red with the message's text ("Raven is out of
  range.") for 4 s. No answer in 5 s says "The server has not answered." The client's refusals (group 7 lines
  before anything is sent) start nothing.
- Open question, still open: "How latency, failed transfers and invalid targets are shown." The chip is one candidate
  for actions only (not transfers); it does not settle it.
- The classic quantity and gil windows (itemctrl / moneyctr) take the mouse as retail's do: a click on a child is
  the key retail re-sends for it, - / + repeat while held (winhits::ctr). The modern panels keep their buttons.
- Remaining retail windows (2026-11-02, ours): Play Time ticks on the Status page (days, hours, minutes, seconds and
  retail's line; Enter or "Show in the log" still prints it). link11, naming a new linkshell, takes the window: the
  shell icon in the chosen colour, the name as a field (a click types), Red / Green / Blue as sliders 0..15, OK and
  Cancel; retail's keys still drive it. Retail's link6 header no longer draws over the modern member list. Region
  Info's maps (Conquest, Besieged, Campaign, Colonization) keep retail's map and markers; their info windows are one
  panel (modern/regioninfo.rs) at the edge away from the cursor: the region's owner, tally and nation gauges, the
  nation ranking, Al Zahbi's and each army's level and orders, a stronghold's forces, mirrors and prisoners, an
  area's development and bivouacs, the coalition ranks and the legend's value.
- Suggestion tests (2026-11-02, plan G): game states recorded as JSON cases (VANA_SUGGEST_RECORD=<folder>) and
  replayed against the rules by `cargo test suggest` (client/suggest_cases/, a case for every rule).
- UI modes (2026-11-02, ours; the plan's "UI states" as adaptive presets; modern/modes.rs): the mode follows the game:
  Combat while engaged and 4 s after (a pull's pause keeps it), Management while a window or menu the command menu
  opens has the keys, else Exploration. It never takes the keyboard or moves anything: each mode only hides HUD panels
  (status icons, party, alliances, target, location, cast bar, minimap, Timers, Training, Skillchains; windows the
  player opens never hide). A hidden panel is not drawn, so what docks on it falls to what it sits on. A pinned panel
  shows in every mode. Panels fade in and out over 0.25 s unless Fades is off. Defaults: Combat hides the location
  panel (the user's rule of 2026-11-01, now this preset) and Training; Exploration and Management hide Skillchains.
  `/uimodes` opens the editor (Shown / Hidden per mode, Pin, Fades, Reset, Close; Escape closes); config
  `modern_modes`. `modern-shot --modes`. The editor is mouse-only for now. The edge windows (Timers, Training,
  Skillchains, the minimap) fade as the other panels.
  Every loaded mod's windows and HUD groups are rows too (2026-11-07, the user: "all of the windows created by mods"):
  each window by its panel key (`mod:<mod>:<window>`, or the built-in panel it stands in for), each HUD window as
  `hud:<mod>:<window>` ("<Mod> (on screen)" for the default one); shown in every mode until set otherwise, faded as
  the panels are. The list scrolls past 60 % of the screen's height.
- Open question, still open: "Which windows hide in combat and which stay pinned?" The defaults above are one
  candidate, every one changeable in the editor; they do not settle it.
- The usable Items list (action menu > Items) puts a small bag name (Temporary, Inventory, Mog Wardrobe, Mog Wardrobe
  2..8) above each bag's rows, and above the top visible row, once it lists rows from more than one bag (ours; retail
  has no headers). The order and keys stay as retail's.
- Not yet: alliance rows live.
