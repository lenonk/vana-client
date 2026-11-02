//! The mod API's reference (docs/lua-mods.md 4): one entry per global function and event. `lua-stubs` writes LuaLS
//! stubs from it, and a test keeps it equal to what api.rs binds.

pub struct Func {
    /// The namespace table, or "" for a global function.
    pub ns: &'static str,
    pub name: &'static str,
    pub doc: &'static str,
    /// (name, LuaLS type, doc); a name ending in '?' is optional.
    pub params: &'static [(&'static str, &'static str, &'static str)],
    /// The LuaLS return type ("" for none).
    pub ret: &'static str,
}

pub struct Event {
    pub name: &'static str,
    pub doc: &'static str,
    pub args: &'static [(&'static str, &'static str)],
}

pub const FUNCS: &[Func] = &[
    Func { ns: "", name: "print", doc: "Writes its arguments to the chat log (dim) and vana-client.log.", params: &[("...", "any", "values, tab-separated")], ret: "" },
    Func { ns: "", name: "require", doc: "Loads a Lua file from the mod's folder once (a.b is a/b.lua or a/b/init.lua) and returns what it returned.", params: &[("name", "string", "module name")], ret: "any" },
    Func { ns: "", name: "sleep", doc: "Pauses the running handler for `seconds` (other handlers and the game go on).", params: &[("seconds?", "number", "default 0: the next frame")], ret: "" },
    Func { ns: "", name: "sleep_frames", doc: "Pauses the running handler for `n` frames.", params: &[("n?", "integer", "default 1")], ret: "" },
    Func { ns: "", name: "wait_until", doc: "Pauses the running handler until `test()` returns true (checked once a frame), or `timeout` seconds pass.", params: &[("test", "fun(): boolean", "the condition"), ("timeout?", "number", "seconds; none waits for ever")], ret: "" },
    Func { ns: "", name: "clock", doc: "Seconds since the mod was loaded (monotonic).", params: &[], ret: "number" },
    Func { ns: "log", name: "info", doc: "Writes a line to vana-client.log.", params: &[("text", "string", "")], ret: "" },
    Func { ns: "log", name: "warn", doc: "Writes a warning to vana-client.log.", params: &[("text", "string", "")], ret: "" },
    Func { ns: "log", name: "error", doc: "Writes an error to vana-client.log.", params: &[("text", "string", "")], ret: "" },
    Func { ns: "log", name: "debug", doc: "Writes a debug line to vana-client.log (shown with debug logging on).", params: &[("text", "string", "")], ret: "" },
    Func { ns: "events", name: "on", doc: "Calls `handler` on every `event` (see the events list), as a coroutine that may sleep. Higher priority runs first.", params: &[("event", "vana.Event|string", "event name"), ("handler", "function", "its arguments are the event's"), ("options?", "{priority: integer?}", "")], ret: "integer" },
    Func { ns: "events", name: "off", doc: "Removes a handler added by events.on.", params: &[("id", "integer", "what events.on returned")], ret: "" },
    Func { ns: "commands", name: "register", doc: "Adds a command: typed as //name or /name, it calls `handler` with the words after it.", params: &[("name", "string", "without the slash"), ("handler", "fun(...: string)", "")], ret: "" },
    Func { ns: "commands", name: "run", doc: "Runs a line as if the player had typed it (game commands, our commands, other mods' commands). Any line but a //mod command is a game action: it needs actions (while handling the player's input) or automation.", params: &[("line", "string", "e.g. '/ma \"Cure\" <t>'")], ret: "" },
    Func { ns: "filters", name: "chat", doc: "Runs `filter` on every chat line before it is kept: return nil to keep it, false to drop it, or new text (and optionally a new mode) to replace it. Runs inline: it cannot sleep or read game state.", params: &[("filter", "fun(text: string, mode: integer?, battle: boolean): (string|false|nil, integer?)", "")], ret: "integer" },
    Func { ns: "filters", name: "typed", doc: "Runs `filter` on every line typed or run (macros, mods) before the command parser: nil keeps it, false drops it, a string replaces it. Runs inline.", params: &[("filter", "fun(line: string): string|false|nil", "")], ret: "integer" },
    Func { ns: "filters", name: "off", doc: "Removes a filter.", params: &[("id", "integer", "what filters.chat / filters.typed returned")], ret: "" },
    Func { ns: "ui", name: "window", doc: "Creates or replaces a window: title, width, help, and elements in order. Element types: text {id, text, dim}; buttons {id, label, items, marked} (marked makes them options); slider {id, label, steps, at, shown}; list {id, rows, at, height}; gauge {id, label, value 0..1, text, kind hp|mp|tp|cast|experience|recast}; separator. Indexes are 1-based. Window options: hud (never takes the keys), frame = false (no frame), x / y (its place in the classic UI). It draws in the active UI's look (classic Config page or modern panel).", params: &[("id", "string", ""), ("spec", "vana.WindowSpec", "")], ret: "" },
    Func { ns: "ui", name: "update", doc: "Changes one element of a window (only the fields given): e.g. a list's rows, a text's text, a slider's at and shown.", params: &[("id", "string", "window"), ("element", "string", "element id"), ("fields", "table", "")], ret: "" },
    Func { ns: "ui", name: "open", doc: "Shows a window and gives it the keys (arrows, Enter, Escape); the mouse works in both UIs.", params: &[("id", "string", "")], ret: "" },
    Func { ns: "ui", name: "show", doc: "Shows a window without giving it the keys (a HUD: hud = true windows never take them; a click still acts).", params: &[("id", "string", "")], ret: "" },
    Func { ns: "ui", name: "close", doc: "Hides a window.", params: &[("id", "string", "")], ret: "" },
    Func { ns: "ui", name: "is_open", doc: "Whether a window shows.", params: &[("id", "string", "")], ret: "boolean" },
    Func { ns: "ui", name: "on", doc: "The window's handler: called with the window id and an event {kind = 'press' | 'set' | 'select' | 'close', id = element id, index = 1-based}.", params: &[("id", "string", ""), ("handler", "fun(window: string, event: {kind: string, id: string?, index: integer?})", "")], ret: "" },
    Func { ns: "packets", name: "on_in", doc: "Hooks received packets (one id, or every one when nil) before the client reads them. The handler gets {id, data, original, injected, blocked, outgoing} (data and original are the whole sub-packet, header included) and returns nil to leave it, false to block it, or new bytes. Hooks chain across mods: a blocked packet still reaches later ones. Needs packets.read; changing or blocking needs packets.write.", params: &[("id", "integer?", "packet id"), ("handler", "fun(p: vana.Packet): (string|false|nil)", "")], ret: "integer" },
    Func { ns: "packets", name: "on_out", doc: "Hooks packets about to be sent, after the client built them; as on_in.", params: &[("id", "integer?", "packet id"), ("handler", "fun(p: vana.Packet): (string|false|nil)", "")], ret: "integer" },
    Func { ns: "packets", name: "off", doc: "Removes a packet hook.", params: &[("id", "integer", "what on_in / on_out returned")], ret: "" },
    Func { ns: "packets", name: "inject_in", doc: "Hands the client a packet as if the server had sent it (it passes the hooks, injected = true). Needs packets.write.", params: &[("id_or_data", "integer|string", "an id, or a whole sub-packet"), ("body?", "string", "the body after the 4-byte header")], ret: "" },
    Func { ns: "packets", name: "inject_out", doc: "Sends a packet to the server (it passes the hooks, injected = true). Needs packets.write.", params: &[("id_or_data", "integer|string", "an id, or a whole sub-packet"), ("body?", "string", "the body after the 4-byte header")], ret: "" },
    Func { ns: "packets", name: "build", doc: "A whole sub-packet from an id and its body (the header's size made to fit; at most 508 bytes in all).", params: &[("id", "integer", ""), ("body", "string", "")], ret: "string" },
    Func { ns: "packets", name: "last", doc: "The last packet of an id seen each way, or nil. Needs packets.read.", params: &[("direction", "'in'|'out'", ""), ("id", "integer", "")], ret: "string?" },
    Func { ns: "chat", name: "add", doc: "Adds a line to the chat log. With a mode it takes that mode's colour, filters and indent, like a game line; {colour = mode} takes only the mode's colour.", params: &[("text", "string", ""), ("how?", "integer|{mode: integer?, colour: integer?}", "the game's chat mode (0..255), or a table")], ret: "" },
    Func { ns: "chat", name: "line", doc: "The chat line: whether it is open, and its text.", params: &[], ret: "{open: boolean, text: string}" },
    Func { ns: "chat", name: "set_line", doc: "Opens the chat line with `text` (the player still presses Enter), or closes it with open = false.", params: &[("text?", "string", ""), ("open?", "boolean", "default true")], ret: "" },
    Func { ns: "input", name: "on_key", doc: "Sees every key press, repeat and release before the game and the UI do: {key, down, repeated, ctrl, alt, shift, typing, blocked}. Return true to take it from the game (needs input.capture; its repeats and release go with it). Filters chain across mods: a taken key still reaches later ones with blocked = true. It is the player's input: actions may act here.", params: &[("handler", "fun(e: vana.KeyEvent): boolean?", "")], ret: "integer" },
    Func { ns: "input", name: "on_mouse", doc: "Sees the mouse before the game and the UI do: {kind = 'down' | 'up' | 'move' | 'wheel', button (left, right, middle, back, forward), x, y (window pixels), delta (wheel notches, up positive), blocked}. Return true to take it (needs input.capture).", params: &[("handler", "fun(e: vana.MouseEvent): boolean?", "")], ret: "integer" },
    Func { ns: "input", name: "off", doc: "Removes a key or mouse filter.", params: &[("id", "integer", "what on_key / on_mouse returned")], ret: "" },
    Func { ns: "input", name: "held", doc: "Whether a key is held (a name as on_key gives it, or shift / ctrl / alt / win for either side).", params: &[("key", "string", "")], ret: "boolean" },
    Func { ns: "input", name: "modifiers", doc: "The modifier keys held.", params: &[], ret: "{ctrl: boolean, alt: boolean, shift: boolean, win: boolean}" },
    Func { ns: "input", name: "mouse", doc: "The pointer (window pixels) and its buttons.", params: &[], ret: "{x: number, y: number, left: boolean, right: boolean, middle: boolean}" },
    Func { ns: "input", name: "typing", doc: "Whether a text field (the chat line, a search, the map's...) has the keys.", params: &[], ret: "boolean" },
    Func { ns: "input", name: "press", doc: "Presses and releases a key, with modifiers (\"enter\", \"ctrl+f1\", \"^1\"), as the keyboard would this frame. Needs input.capture.", params: &[("key", "string", "")], ret: "" },
    Func { ns: "input", name: "send", doc: "Holds a key down or lets it go. Needs input.capture.", params: &[("key", "string", ""), ("down", "boolean", "")], ret: "" },
    Func { ns: "binds", name: "set", doc: "Binds a key with exact modifiers (\"f9\", \"ctrl+1\", Windower's \"^!f1\": ^ Ctrl, ! Alt, ~ Shift, @ Win) to a line run as typed or a function. The key is taken from the game. A line other than a //mod command is a game action (needs actions). Options: up (fire on release), typing (fire while a text field is open too).", params: &[("key", "string", ""), ("action", "string|function", ""), ("options?", "{up: boolean?, typing: boolean?}", "")], ret: "" },
    Func { ns: "binds", name: "unset", doc: "Removes the mod's binds of a key.", params: &[("key", "string", "")], ret: "" },
    Func { ns: "binds", name: "list", doc: "The mod's binds.", params: &[], ret: "{key: string, line: string?, up: boolean, typing: boolean}[]" },
    Func { ns: "timer", name: "after", doc: "Calls `handler` once after `seconds`.", params: &[("seconds", "number", ""), ("handler", "function", "")], ret: "integer" },
    Func { ns: "timer", name: "every", doc: "Calls `handler` every `seconds` until cancelled.", params: &[("seconds", "number", "at least 0.01"), ("handler", "function", "")], ret: "integer" },
    Func { ns: "timer", name: "cancel", doc: "Stops a timer.", params: &[("id", "integer", "what timer.after / timer.every returned")], ret: "" },
    Func { ns: "game", name: "player", doc: "Our character: name, id, index, zone, pos {x, y, z}, heading, status, hp / mp / tp and their percents, maxima, jobs and levels, target id.", params: &[], ret: "vana.Player" },
    Func { ns: "game", name: "zone", doc: "The zone id, or nil while zoning.", params: &[], ret: "integer?" },
    Func { ns: "game", name: "entity", doc: "An entity in range by server id or index, or nil.", params: &[("key", "integer", "server id or index")], ret: "vana.Entity?" },
    Func { ns: "game", name: "entities", doc: "Every entity in range, by index.", params: &[("filter?", "{distance: number?, kinds: ('player'|'npc'|'mob')[]?}", "")], ret: "vana.Entity[]" },
    Func { ns: "game", name: "target", doc: "The current target, or nil.", params: &[], ret: "vana.Entity?" },
    Func { ns: "game", name: "party", doc: "The party and alliance members (slot 0..5 our party, 6..17 the others).", params: &[], ret: "vana.Member[]" },
    Func { ns: "game", name: "items", doc: "A bag's items in the list's order (0 inventory, 1 safe, 2 storage, 3 temporary, 4 locker, 5 satchel, 6 sack, 7 case, 8.. wardrobes).", params: &[("bag", "integer", "")], ret: "vana.Item[]" },
    Func { ns: "game", name: "bag_size", doc: "How many slots a bag has (nil before the server said).", params: &[("bag", "integer", "")], ret: "integer?" },
    Func { ns: "game", name: "equipment", doc: "What is equipped, by equipment slot (0 main .. 15 back): {bag, slot, id}.", params: &[], ret: "table<integer, {bag: integer, slot: integer, id: integer}>" },
    Func { ns: "game", name: "buffs", doc: "Our status effects: id, and seconds remaining when timed.", params: &[], ret: "{id: integer, remaining: number?}[]" },
    Func { ns: "game", name: "recasts", doc: "Seconds left on recasts: spells by spell id, abilities by recast id (only those still counting).", params: &[], ret: "{spells: table<integer, number>, abilities: table<integer, number>}" },
    Func { ns: "game", name: "time", doc: "The Vana'diel clock: earth (seconds since the Vana'diel epoch), year, month, day, hour, minute, weekday (0 Firesday), moon_phase, moon (percent).", params: &[], ret: "vana.Time" },
    Func { ns: "game", name: "weather", doc: "The weather id.", params: &[], ret: "integer" },
    Func { ns: "game", name: "key_items", doc: "The key item ids we hold.", params: &[], ret: "integer[]" },
    Func { ns: "game", name: "treasure", doc: "The treasure pool: {slot (0..9), item, count, dropper, lotted, passed, lot, high_lot, high_name} per item in it.", params: &[], ret: "table[]" },
    Func { ns: "game", name: "event", doc: "The event (cutscene, NPC talk) under way, or nil: {menu = {question, options, cursor}} while it shows a menu (cursor from 1).", params: &[], ret: "table?" },
    Func { ns: "game", name: "entity_by_tag", doc: "The entity a target tag names (t, st, bt, pet, me, ht, scan, p0..p5, a10..a25, with or without <>), a player's name or a server id; nil when none.", params: &[("tag", "string", "")], ret: "vana.Entity?" },
    Func { ns: "game", name: "skills", doc: "Our skills by id: combat (1..31) and magic (32..47) {level, capped}; crafts (48..) {rank, level, capped}.", params: &[], ret: "{combat: table<integer, table>, magic: table<integer, table>, crafts: table<integer, table>}" },
    Func { ns: "game", name: "jobs", doc: "Each unlocked job's level, and the master levels, by job id.", params: &[], ret: "{levels: table<integer, integer>, master: table<integer, integer>}" },
    Func { ns: "game", name: "stats", doc: "Base and bonus stats (STR..CHR), attack, defense, resistances, item and superior level, title, rank and points, nation, home point, experience and the next level's, master level and exemplar points, limit and merit points; nil before the server sent them.", params: &[], ret: "table?" },
    Func { ns: "game", name: "merits", doc: "Each merit we have by index: {level, next_cost}.", params: &[], ret: "table<integer, {level: integer, next_cost: integer}>" },
    Func { ns: "game", name: "job_points", doc: "Capacity points, job points and job points spent, by job id.", params: &[], ret: "table<integer, {capacity: integer, points: integer, spent: integer}>" },
    Func { ns: "game", name: "gil", doc: "Our gil.", params: &[], ret: "integer" },
    Func { ns: "game", name: "spells", doc: "The spell ids we have learned.", params: &[], ret: "integer[]" },
    Func { ns: "game", name: "abilities", doc: "The ability ids (res.ability) we can use now: job abilities, weapon skills, pet commands with a pet.", params: &[], ret: "integer[]" },
    Func { ns: "game", name: "job_data", doc: "The job setups of main and sub: job, raw (the server's bytes), and for BLU blue_spells (ids), for PUP head, frame and attachments (item ids).", params: &[], ret: "{main: table?, sub: table?}" },
    Func { ns: "game", name: "info", doc: "The session: zone, server, mog_house, chat_open, menu_open, event, in_combat, locked (target lock), autorun, follow (server id), pet_index, map_grid (as <pos>), modern_ui.", params: &[], ret: "table" },
    Func { ns: "game", name: "camera", doc: "The camera: eye and look points, and their distance.", params: &[], ret: "{eye: {x: number, y: number, z: number}, look: {x: number, y: number, z: number}, distance: number}" },
    Func { ns: "game", name: "npc_names", doc: "The zone's NPC list by server id, spawned or not (Wide Scan's names).", params: &[], ret: "table<integer, string>" },
    Func { ns: "actions", name: "cast", doc: "Casts a spell by id, with the magic menu's checks (recast, MP, learned, status) and its refusal lines.", params: &[("spell", "integer", "spell id"), ("target", "integer|string?", "a server id, a tag (<t>, <me>, <bt>, <st>...) or a name; none: the current target")], ret: "" },
    Func { ns: "actions", name: "ability", doc: "Uses a job ability, weapon skill or pet command by record id (res.ability), with the ability menu's checks.", params: &[("id", "integer", "ability id"), ("target", "integer|string?", "as actions.cast")], ret: "" },
    Func { ns: "actions", name: "weaponskill", doc: "As actions.ability (weapon skills are ability records).", params: &[("id", "integer", "ability id"), ("target", "integer|string?", "as actions.cast")], ret: "" },
    Func { ns: "actions", name: "ranged", doc: "A ranged attack (/range).", params: &[("target", "integer|string?", "as actions.cast")], ret: "" },
    Func { ns: "actions", name: "use_item", doc: "Uses an item by id, as /item finds it (temporary items, inventory, wardrobes).", params: &[("item", "integer", "item id"), ("target", "integer|string?", "as actions.cast")], ret: "" },
    Func { ns: "actions", name: "use_slot", doc: "Uses the item in a bag slot, as the item menu does.", params: &[("bag", "integer", ""), ("index", "integer", ""), ("target", "integer?", "a server id; none: ourselves")], ret: "" },
    Func { ns: "actions", name: "attack", doc: "Engages a target, or switches to it while engaged (/attack).", params: &[("target", "integer|string?", "none: the current target")], ret: "" },
    Func { ns: "actions", name: "disengage", doc: "Disengages (/attack off).", params: &[], ret: "" },
    Func { ns: "actions", name: "assist", doc: "Assists a player (/assist).", params: &[("target", "integer|string", "")], ret: "" },
    Func { ns: "actions", name: "target", doc: "Sets the current target, or clears it with nil.", params: &[("id", "integer?", "server id")], ret: "" },
    Func { ns: "actions", name: "lock", doc: "Locks on to the current target, or lets go.", params: &[("on", "boolean", "")], ret: "" },
    Func { ns: "actions", name: "heal", doc: "Rests (/heal): true on, false off, none toggles.", params: &[("on", "boolean?", "")], ret: "" },
    Func { ns: "actions", name: "equip", doc: "Wears pieces as the equipment window does: (slot, item) or ({slot = item, ...}). A slot is 0..15 or a name (main, sub, range, ammo, head, body, hands, legs, feet, neck, waist, left_ear, right_ear, left_ring, right_ring, back); an item is an id, {bag = , index = }, or false to take it off. Pieces already worn stay.", params: &[("slot_or_set", "integer|string|table", ""), ("item", "integer|table|false|nil", "")], ret: "" },
    Func { ns: "actions", name: "unequip", doc: "Takes off what a slot wears.", params: &[("slot", "integer|string", "")], ret: "" },
    Func { ns: "actions", name: "equip_set", doc: "Wears an equip set (/equipset).", params: &[("n", "integer", "1..200")], ret: "" },
    Func { ns: "actions", name: "move", doc: "Moves an item to another bag (its first free slot).", params: &[("bag", "integer", ""), ("index", "integer", ""), ("to", "integer", "bag"), ("count", "integer?", "none: the whole stack")], ret: "" },
    Func { ns: "actions", name: "sort", doc: "Sorts a bag and merges its stacks.", params: &[("bag", "integer", "")], ret: "" },
    Func { ns: "actions", name: "drop", doc: "Drops an inventory slot's whole stack (no confirmation).", params: &[("index", "integer", "inventory slot")], ret: "" },
    Func { ns: "actions", name: "lot", doc: "Lots on a treasure pool item.", params: &[("slot", "integer", "0..9, as game.treasure gives it")], ret: "" },
    Func { ns: "actions", name: "pass", doc: "Passes on a treasure pool item.", params: &[("slot", "integer", "0..9")], ret: "" },
    Func { ns: "actions", name: "cancel", doc: "Cancels a status effect we have, when it can be cancelled.", params: &[("status", "integer", "status id")], ret: "" },
    Func { ns: "actions", name: "choose", doc: "Picks an option of the event menu showing (game.event().menu), as the cursor and Enter would.", params: &[("option", "integer", "from 1")], ret: "" },
    Func { ns: "actions", name: "talk", doc: "Talks to an NPC (or checks it), as Enter on it does.", params: &[("id", "integer", "server id")], ret: "" },
    Func { ns: "actions", name: "scan", doc: "Asks for the Wide Scan list.", params: &[], ret: "" },
    Func { ns: "actions", name: "track", doc: "Tracks an entity with Wide Scan.", params: &[("index", "integer", "its index (entity.index)")], ret: "" },
    Func { ns: "actions", name: "untrack", doc: "Stops Wide Scan tracking.", params: &[], ret: "" },
    Func { ns: "actions", name: "run", doc: "Runs on (autorun) toward a heading (entity.heading's units), or the way we face. The player's keys still steer and stop it.", params: &[("heading", "number?", "")], ret: "" },
    Func { ns: "actions", name: "run_to", doc: "Runs to a point and stops within a distance of it.", params: &[("x", "number", ""), ("z", "number", ""), ("within", "number?", "yalms; 1 when none")], ret: "" },
    Func { ns: "actions", name: "follow", doc: "Follows an entity (/follow).", params: &[("id", "integer", "server id")], ret: "" },
    Func { ns: "actions", name: "walk", doc: "Walks (true) or runs (false), as the walk key.", params: &[("on", "boolean", "")], ret: "" },
    Func { ns: "actions", name: "stop", doc: "Stops a run, run_to or follow.", params: &[], ret: "" },
    Func { ns: "actions", name: "turn", doc: "Faces a heading (entity.heading's units).", params: &[("heading", "number", "")], ret: "" },
    Func { ns: "res", name: "item", doc: "An item's record: name, description, kind, flags, stack, type, targets, level, slots, races, jobs (bits), superior_level, damage, delay, skill, max_charges, cast_time, use_delay, reuse_delay.", params: &[("id", "integer", "")], ret: "table?" },
    Func { ns: "res", name: "spell", doc: "A spell's record: name, description, kind (1 white .. 8 trust), element, targets (bits), skill, mp, cast_time and recast (seconds), range, levels (by job).", params: &[("id", "integer", "")], ret: "table?" },
    Func { ns: "res", name: "ability", doc: "An ability's record by its name-table id (weapon skills 0..0x1ff, job abilities 0x200.., pet commands 0x400..): name, description, kind, cost, recast_id, targets, tp, level.", params: &[("id", "integer", "")], ret: "table?" },
    Func { ns: "res", name: "status", doc: "A status effect's name.", params: &[("id", "integer", "")], ret: "string?" },
    Func { ns: "res", name: "zone", doc: "A zone's name.", params: &[("id", "integer", "")], ret: "string?" },
    Func { ns: "res", name: "title", doc: "A title's name.", params: &[("id", "integer", "")], ret: "string?" },
    Func { ns: "res", name: "key_item", doc: "A key item's name and description.", params: &[("id", "integer", "")], ret: "{id: integer, name: string, description: string?}?" },
    Func { ns: "res", name: "job", doc: "A job's abbreviation (1 WAR .. 22 RUN).", params: &[("id", "integer", "")], ret: "string?" },
    Func { ns: "fs", name: "read", doc: "A file's bytes, or nil: the mod's data folder first (what it wrote), then its own files. \"shared/...\" is the area every mod with files.shared can reach.", params: &[("path", "string", "relative, / separated")], ret: "string?" },
    Func { ns: "fs", name: "write", doc: "Writes a file in the mod's data folder (folders made as needed).", params: &[("path", "string", ""), ("data", "string", "")], ret: "" },
    Func { ns: "fs", name: "append", doc: "Adds to the end of a file in the mod's data folder.", params: &[("path", "string", ""), ("data", "string", "")], ret: "" },
    Func { ns: "fs", name: "exists", doc: "Whether a file or folder is there (data folder or the mod's own files).", params: &[("path", "string", "")], ret: "boolean" },
    Func { ns: "fs", name: "list", doc: "The names in a folder, sorted (folders end in /): the data folder's and the mod's own.", params: &[("path?", "string", "none: the top")], ret: "string[]" },
    Func { ns: "fs", name: "mkdir", doc: "Makes a folder (and its parents) in the data folder.", params: &[("path", "string", "")], ret: "" },
    Func { ns: "fs", name: "remove", doc: "Removes a file or a folder with everything in it, from the data folder.", params: &[("path", "string", "")], ret: "" },
    Func { ns: "fs", name: "rename", doc: "Moves a file or folder within the data folder.", params: &[("from", "string", ""), ("to", "string", "")], ret: "" },
    Func { ns: "json", name: "encode", doc: "Plain data as JSON text.", params: &[("value", "any", ""), ("options?", "{pretty: boolean?}", "")], ret: "string" },
    Func { ns: "json", name: "decode", doc: "JSON text as Lua values.", params: &[("text", "string", "")], ret: "any" },
    Func { ns: "mods", name: "list", doc: "Every loaded mod: id, name, version and the functions it provides.", params: &[], ret: "{id: string, name: string, version: string, provides: string[]}[]" },
    Func { ns: "mods", name: "provide", doc: "Offers a function to other mods (mods.call). Values cross as copies of plain data.", params: &[("name", "string", ""), ("func", "function", "")], ret: "" },
    Func { ns: "mods", name: "call", doc: "Calls another mod's provided function now, in its VM, and returns what it returned (copies). It cannot sleep.", params: &[("mod", "string", "its id"), ("name", "string", ""), ("...", "any", "")], ret: "any" },
    Func { ns: "mods", name: "emit", doc: "Raises an event in every other mod: their events.on handlers get the values once the running handler is done (the same mod phase). Prefix the name with your mod's id.", params: &[("event", "string", ""), ("...", "any", "plain data")], ret: "" },
    Func { ns: "text", name: "from_shift_jis", doc: "Shift-JIS bytes (packets, game files) as text.", params: &[("bytes", "string", "")], ret: "string" },
    Func { ns: "text", name: "to_shift_jis", doc: "Text as Shift-JIS bytes.", params: &[("text", "string", "")], ret: "string" },
    Func { ns: "text", name: "plain", doc: "A line with its auto-translate phrases as plain words.", params: &[("text", "string", "")], ret: "string" },
    Func { ns: "client", name: "copy", doc: "Puts text on the clipboard.", params: &[("text", "string", "")], ret: "" },
    Func { ns: "storage", name: "save", doc: "Writes storage.account and storage.character now (they are also written at logout and unload).", params: &[], ret: "" },
    Func { ns: "settings", name: "get", doc: "A setting declared in mod.toml: the user's value, else its default.", params: &[("key", "string", "")], ret: "any" },
];

pub const EVENTS: &[Event] = &[
    Event { name: "load", doc: "The mod's entry file has run.", args: &[] },
    Event { name: "unload", doc: "The mod is about to be unloaded (logout, /lua unload or reload).", args: &[] },
    Event { name: "login", doc: "The character is in the world, once per session.", args: &[("name", "string")] },
    Event { name: "logout", doc: "The character is leaving the world.", args: &[] },
    Event { name: "zone_change", doc: "A new zone has loaded.", args: &[("zone", "integer"), ("previous", "integer?")] },
    Event { name: "settings_changed", doc: "The user changed one of the mod's settings (its Extras page, or /lua set).", args: &[("key", "string"), ("value", "any")] },
    Event { name: "frame", doc: "Every frame (prefer timers and events: this one costs time every frame).", args: &[("dt", "number")] },
    Event { name: "hp_change", doc: "Our HP changed.", args: &[("hp", "integer"), ("previous", "integer")] },
    Event { name: "mp_change", doc: "Our MP changed.", args: &[("mp", "integer"), ("previous", "integer")] },
    Event { name: "tp_change", doc: "Our TP changed.", args: &[("tp", "integer"), ("previous", "integer")] },
    Event { name: "hpp_change", doc: "Our HP percent changed.", args: &[("hpp", "integer"), ("previous", "integer")] },
    Event { name: "mpp_change", doc: "Our MP percent changed.", args: &[("mpp", "integer"), ("previous", "integer")] },
    Event { name: "status_change", doc: "Our status changed (0 idle, 1 engaged, 2 / 3 dead, 33 resting...).", args: &[("status", "integer"), ("previous", "integer")] },
    Event { name: "job_change", doc: "Our main or sub job changed.", args: &[("main", "integer"), ("main_level", "integer"), ("sub", "integer"), ("sub_level", "integer")] },
    Event { name: "level_up", doc: "Our main job's level went up.", args: &[("level", "integer")] },
    Event { name: "level_down", doc: "Our main job's level went down.", args: &[("level", "integer")] },
    Event { name: "buff_gain", doc: "We gained a status effect.", args: &[("id", "integer")] },
    Event { name: "buff_lose", doc: "We lost a status effect.", args: &[("id", "integer")] },
    Event { name: "target_change", doc: "Our target changed (nil: none).", args: &[("id", "integer?"), ("previous", "integer?")] },
    Event { name: "time_change", doc: "A Vana'diel minute passed (minutes since midnight).", args: &[("minute", "integer"), ("previous", "integer")] },
    Event { name: "day_change", doc: "A new Vana'diel day (0 Firesday).", args: &[("weekday", "integer"), ("previous", "integer")] },
    Event { name: "moon_change", doc: "The moon's phase changed.", args: &[("phase", "integer"), ("previous", "integer")] },
    Event { name: "weather_change", doc: "The weather changed.", args: &[("weather", "integer"), ("previous", "integer")] },
    Event { name: "party_change", doc: "The party or alliance changed (members joined, left or moved).", args: &[] },
    Event { name: "item_add", doc: "An item appeared in a bag slot.", args: &[("bag", "integer"), ("slot", "integer"), ("id", "integer"), ("count", "integer")] },
    Event { name: "item_remove", doc: "An item left a bag slot.", args: &[("bag", "integer"), ("slot", "integer"), ("id", "integer"), ("count", "integer")] },
    Event { name: "chat_message", doc: "A chat line from a player (or a GM): its text, sender, kind (0 say, 1 shout, 3 tell, 4 party, 5 linkshell, 0x1a yell, 0x1b linkshell 2, 0x21 unity) and whether a GM sent it.", args: &[("text", "string"), ("sender", "string"), ("kind", "integer"), ("gm", "boolean")] },
    Event { name: "action", doc: "A decoded action packet (s2c 0x028): actor, category, param, info and targets with their results.", args: &[("action", "vana.Action")] },
    Event { name: "battle_message", doc: "A battle message (s2c 0x029 / 0x02D): actor, target, message id and its two parameters.", args: &[("actor", "integer"), ("target", "integer"), ("message", "integer"), ("param1", "integer"), ("param2", "integer")] },
    Event { name: "damage", doc: "A damage number: a battle message that puts one up (main and proc records), whatever the chat filters show.", args: &[("actor", "integer"), ("target", "integer"), ("value", "integer"), ("message", "integer")] },
    Event { name: "item_change", doc: "A bag slot's count changed.", args: &[("bag", "integer"), ("slot", "integer"), ("id", "integer"), ("count", "integer"), ("previous", "integer")] },
];

/// LuaLS stubs (`---@meta`) for the whole API.
pub fn stubs() -> String {
    let mut s = String::from("---@meta\n-- vana-client mod API (generated by `vana-client lua-stubs`; docs/lua-mods.md).\n\n");
    s += "---@alias vana.Event\n";
    for e in EVENTS {
        s += &format!("---| \"{}\" # {}\n", e.name, e.doc);
    }
    s += "\n---@class vana.Player\n---@field name string\n---@field id integer\n---@field index integer\n---@field zone integer?\n---@field pos {x: number, y: number, z: number}\n---@field heading number\n---@field status integer\n---@field hp integer?\n---@field mp integer?\n---@field tp integer?\n---@field hpp integer?\n---@field mpp integer?\n---@field hp_max integer?\n---@field mp_max integer?\n---@field main_job integer?\n---@field main_level integer?\n---@field sub_job integer?\n---@field sub_level integer?\n---@field target integer?\n\n";
    s += "---@class vana.Entity\n---@field id integer\n---@field index integer\n---@field name string\n---@field kind 'player'|'npc'|'mob'\n---@field pos {x: number, y: number, z: number}\n---@field heading number\n---@field distance number\n---@field hpp integer\n---@field status integer\n---@field speed number\n---@field claim integer\n---@field targetable boolean\n---@field hidden boolean\n---@field master integer the master's index (pets), 0 for none\n---@field me boolean? true for our own character\n---@field race integer?\n---@field model integer a monster or NPC model id (0 for a player-style look)\n---@field look {face: integer, head: integer, body: integer, hands: integer, legs: integer, feet: integer, main: integer, sub: integer, range: integer}? a player-style look\n---@field in_party boolean\n---@field in_alliance boolean\n\n";
    s += "---@class vana.Member\n---@field slot integer\n---@field party integer\n---@field id integer\n---@field index integer\n---@field name string\n---@field hp integer\n---@field mp integer\n---@field tp integer\n---@field hpp integer\n---@field mpp integer\n---@field zone integer\n---@field in_zone boolean\n---@field main_job integer\n---@field main_level integer\n---@field sub_job integer\n---@field sub_level integer\n---@field leader boolean\n---@field alliance_leader boolean\n---@field buffs integer[]\n\n";
    s += "---@class vana.Item\n---@field bag integer\n---@field slot integer\n---@field id integer\n---@field count integer\n---@field extdata string? 24 raw bytes\n---@field locked integer\n---@field price integer?\n\n";
    s += "---@class vana.Action\n---@field actor integer\n---@field category integer\n---@field param integer\n---@field info integer\n---@field targets {id: integer, results: {miss: integer, kind: integer, animation: integer, info: integer, scale: integer, value: integer, message: integer, bit: integer, proc: table?, react: table?}[]}[]\n\n";
    s += "---@class vana.WindowSpec\n---@field title string?\n---@field width number?\n---@field help string?\n---@field hud boolean?\n---@field frame boolean?\n---@field x number?\n---@field y number?\n---@field elements table[]?\n\n";
    s += "---@class vana.Packet\n---@field id integer\n---@field data string\n---@field original string\n---@field injected boolean\n---@field blocked boolean\n---@field outgoing boolean\n\n";
    s += "---@class vana.KeyEvent\n---@field key string a, 1, f1, enter, numpad+, lctrl...\n---@field down boolean\n---@field repeated boolean\n---@field ctrl boolean\n---@field alt boolean\n---@field shift boolean\n---@field typing boolean a text field has the keys\n---@field blocked boolean an earlier mod took it\n\n";
    s += "---@class vana.MouseEvent\n---@field kind 'down'|'up'|'move'|'wheel'\n---@field button string?\n---@field x number\n---@field y number\n---@field delta number?\n---@field blocked boolean\n\n";
    s += "---@class vana.Time\n---@field earth integer\n---@field year integer\n---@field month integer\n---@field day integer\n---@field hour integer\n---@field minute integer\n---@field weekday integer\n---@field moon_phase integer\n---@field moon integer\n\n";
    s += "---@class vana.Mod\n---@field id string\n---@field name string\n---@field version string\n---@field api integer\n---@field path string\n\n---@type vana.Mod\n---@diagnostic disable-next-line: missing-fields\nmod = {}\n\n";
    s += "storage = {\n    ---@type table plain data kept for every character of the account\n    account = {},\n    ---@type table plain data kept for this character\n    character = {},\n}\n\n";
    let mut seen = Vec::new();
    for f in FUNCS {
        if !f.ns.is_empty() && !seen.contains(&f.ns) {
            seen.push(f.ns);
            if f.ns != "storage" {
                s += &format!("{} = {{}}\n", f.ns);
            }
        }
    }
    s += "\n";
    for f in FUNCS {
        for line in f.doc.lines() {
            s += &format!("--- {line}\n");
        }
        for (name, ty, doc) in f.params {
            s += &format!("---@param {name} {ty} {doc}\n");
        }
        if !f.ret.is_empty() {
            s += &format!("---@return {}\n", f.ret);
        }
        let args: Vec<&str> = f.params.iter().map(|p| p.0.trim_end_matches('?')).collect();
        let full = if f.ns.is_empty() { f.name.to_string() } else { format!("{}.{}", f.ns, f.name) };
        s += &format!("function {full}({}) end\n\n", args.join(", "));
    }
    s += "-- Events (events.on):\n";
    for e in EVENTS {
        let args: Vec<String> = e.args.iter().map(|(n, t)| format!("{n}: {t}")).collect();
        s += &format!("--   {}({}): {}\n", e.name, args.join(", "), e.doc);
    }
    s
}
