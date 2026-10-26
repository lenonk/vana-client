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
- Not yet: keyboard focus between panels, the target's cast and claim colours, a way to reset places, alliance rows live.
