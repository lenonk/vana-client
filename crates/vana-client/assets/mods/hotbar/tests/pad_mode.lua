-- Hotbars' gamepad mode: the crossbars keep library macros per main job (by reference, like the bars' slots), a
-- button press runs one (a group runs the member picked last), and the mode switches between the crossbars and the
-- keyboard's bars.

local abilities = { [869] = { name = "Rune Enchantment", kind = 1, sub = 21, targets = 1 },
  [870] = { name = "Ignis", kind = 21, targets = 1, recast = 5 },
  [877] = { name = "Tenebrae", kind = 21, targets = 1, recast = 5 } }
res = { spell = function() end, ability = function(id) return abilities[id] end,
  job = function(id) return ({ [22] = "RUN" })[id] end }
game = { player = function() return { main_job = 22, sub_job = 0 } end,
  spells = function() return {} end, abilities = function() return { 869, 870, 877 } end,
  find_items = function() return {} end,
  recasts = function() return { spells = {}, abilities = {}, totals = { spells = {}, abilities = {} } } end }
chat = { add = function() end }
SENT = {}
commands.run = function(line) SENT[#SENT + 1] = line end

local data = require("src.data")
data.init({ read = function() return nil end, write = function() end, decode = json.decode,
  encode = json.encode })

-- The slots: a library entry by reference, swapped, cleared, counted among its uses and gone with it.
local id = data.save_entry("RUN", nil, { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" },
  lines = { "/p Ignis up" } })
assert(data.pad_resolve("RUN", "lt", "up") == nil)
data.pad_assign("RUN", "lt", "up", id)
local m = data.pad_resolve("RUN", "lt", "up")
assert(m.ref == id and m.lines[1] == '/ja "Ignis" <me>' and m.lines[2] == "/p Ignis up")
assert(data.uses("RUN", id) == 1)
data.pad_swap("RUN", "lt", "up", "rt", "a")
assert(data.pad_resolve("RUN", "lt", "up") == nil and data.pad_resolve("RUN", "rt", "a").ref == id)
assert(data.get_raw("RUN", nil, 1, 1) == nil, "the bars are not touched")
data.pad_clear("RUN", "rt", "a")
assert(data.pad_resolve("RUN", "rt", "a") == nil and data.uses("RUN", id) == 0)
data.pad_assign("RUN", "rt", "x", id)
data.delete_entry("RUN", id)
assert(data.pad_resolve("RUN", "rt", "x") == nil and data.uses("RUN", id) == 0)

-- The pad filter pad.lua registers, the controller it reads and the timers it defers to, which the test drives.
local FILTERS = {}
input.on_pad = function(f) FILTERS[#FILTERS + 1] = f return #FILTERS end
local BUTTONS = {}
local CONNECTED = true
input.pad = function() return { connected = CONNECTED, buttons = BUTTONS } end
local LIST = nil
ui.native = ui.native or {}
ui.native.list = function() return LIST end
local QUEUED = {}
timer.after = function(_, f) QUEUED[#QUEUED + 1] = f end
local OVER = {}
local real_get = settings.get
settings.get = function(k) if OVER[k] ~= nil then return OVER[k] end return real_get(k) end

-- A button press: the macro's lines run in order.
local pad = require("src.pad")
assert(#FILTERS == 1, "pad.lua registers one pad filter")
local one = data.save_entry("RUN", nil, { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" },
  lines = { "/p Ignis up" } })
data.pad_assign("RUN", "lt", "b", one)
pad.fire("lt", "b")
assert(SENT[1] == '/ja "Ignis" <me>' and SENT[2] == "/p Ignis up", tostring(SENT[1]))
SENT = {}
pad.fire("rt", "y")
assert(#SENT == 0, "an empty slot runs nothing")

-- A group: a button press opens its list; the d-pad moves the cursor and Accept runs the row (the one picked last
-- under the cursor first).
local grp = data.save_entry("RUN", nil, { name = "Runes", group = {
  { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" } },
  { name = "Tenebrae", action = { kind = "ja", name = "Tenebrae", target = "me" } } } })
data.pad_assign("RUN", "rt", "up", grp)
local chooser = require("src.chooser")
SENT = {}
pad.fire("rt", "up")
assert(chooser.is_open() and #SENT == 0, "the list opens, nothing runs yet")
chooser.step(1)
chooser.step(1)   -- wraps back to the first row
chooser.step(-1)  -- and up to the last
chooser.accept()
assert(not chooser.is_open() and SENT[1] == '/ja "Tenebrae" <me>', tostring(SENT[1]))
assert(data.last_member(data.macro_of("RUN", grp)) == 2)
SENT = {}
pad.fire("rt", "up")
chooser.accept()   -- the one picked last is under the cursor first
assert(SENT[1] == '/ja "Tenebrae" <me>', tostring(SENT[1]))
SENT = {}
pad.fire("rt", "up")
chooser.close()
assert(not chooser.is_open() and #SENT == 0)

-- The mode: the bars by default, switched and parsed.
local mode = require("src.mode")
assert(mode.get() == "hotbars" and not mode.is_pad())
assert(mode.parse("Pad") == "gamepad" and mode.parse("keyboard") == "hotbars" and mode.parse("nope") == nil)
mode.set("gamepad")
assert(mode.is_pad())
assert(mode.toggle() == "hotbars" and not mode.is_pad())
assert(mode.toggle() == "gamepad")
-- Changing the Mode setting hands the choice back to it (the default: the hotbars).
mode.use_setting()
assert(mode.get() == "hotbars")
mode.set("gamepad")

-- In gamepad mode the bars' keys and window stand down.
local bar = require("src.bar")
bar.rebuild()
assert(not ui.is_open("bars"))


-- The crossbars' window: nothing shows while Hotbars is off, and rebuilding or the recast tick raise nothing.
pad.rebuild()
pad.recasts()
pad.tick()
assert(not ui.is_open("pads"))

-- The bumpers' and stick clicks' slots hold macros as the others do, per trigger side.
data.init({ read = function() return nil end, write = function() end, decode = json.decode, encode = json.encode })
local bid = data.save_entry("RUN", nil, { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" },
  lines = { "/p Ignis up" } })
for _, b in ipairs({ "lb", "rb", "l3", "r3" }) do
  data.pad_assign("RUN", "lt", b, bid)
  assert(data.pad_resolve("RUN", "lt", b).ref == bid)
  assert(data.pad_resolve("RUN", "rt", b) == nil)
end
SENT = {}
pad.fire("lt", "l3")
assert(#SENT == 2 and SENT[2] == "/p Ignis up", #SENT)

-- Both triggers held: a third set, apart from the other two.
data.init({ read = function() return nil end, write = function() end, decode = json.decode, encode = json.encode })
local lid = data.save_entry("RUN", nil, { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" },
  lines = { "/p both" } })
data.pad_assign("RUN", "lr", "a", lid)
assert(data.pad_resolve("RUN", "lr", "a").ref == lid)
assert(data.pad_resolve("RUN", "lt", "a") == nil and data.pad_resolve("RUN", "rt", "a") == nil)
SENT = {}
pad.fire("lr", "a")
assert(SENT[2] == "/p both", tostring(SENT[2]))

-- The pad filter. Holds `held` buttons, sends one event and returns what the filter answered; what it deferred runs
-- when `flush` does (the timers the filter and the window's rebuilds use).
local function flush()
  for _ = 1, 20 do
    local q = QUEUED
    QUEUED = {}
    if #q == 0 then return end
    for _, f in ipairs(q) do f() end
  end
end
local function press(button, down, opts)
  BUTTONS[button] = down
  local e = { button = button, down = down, blocked = opts and opts.blocked or false }
  local took = FILTERS[1](e)
  flush()
  return took
end
local function release_all()
  for k in pairs(BUTTONS) do BUTTONS[k] = nil end
  chooser.close()
  SENT = {}
  QUEUED = {}
end

OVER.on, OVER.visible = true, true
data.init({ read = function() return nil end, write = function() end, decode = json.decode, encode = json.encode })
local fid = data.save_entry("RUN", nil, { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" },
  lines = { "/p Ignis up" } })
local other = data.save_entry("RUN", nil, { name = "Other", lines = { "/p other" } })
local both = data.save_entry("RUN", nil, { name = "Both", lines = { "/p both" } })
local runes = data.save_entry("RUN", nil, { name = "Runes", group = {
  { name = "Ignis", action = { kind = "ja", name = "Ignis", target = "me" } },
  { name = "Tenebrae", action = { kind = "ja", name = "Tenebrae", target = "me" } } } })
data.pad_assign("RUN", "lt", "b", fid)
data.pad_assign("RUN", "lt", "x", other)
data.pad_assign("RUN", "lr", "a", both)
data.pad_assign("RUN", "rt", "up", runes)

SENT = {}
-- A trigger is taken on its press only (the host keeps it taken until it comes up); a slot's button likewise.
assert(press("lt", true) == true, "LT is taken")
assert(press("b", true) == true and SENT[2] == "/p Ignis up", "LT + B runs the slot")
assert(press("b", false) == nil, "its release is not answered")
assert(press("lt", false) == nil, "nor is the trigger's")
release_all()

-- A button with no trigger held is the game's.
assert(press("b", true) == nil and #SENT == 0, "B alone is the game's")
press("b", false)
release_all()

-- A slot with nothing in it is taken but runs nothing.
BUTTONS.rt = true
assert(press("y", true) == true and #SENT == 0)
release_all()

-- Both triggers: the third set.
BUTTONS.lt, BUTTONS.rt = true, true
assert(press("a", true) == true and SENT[1] == "/p both", tostring(SENT[1]))
release_all()

-- A press another mod took is left alone.
BUTTONS.lt = true
assert(press("lt", true, { blocked = true }) == nil)
assert(press("b", true, { blocked = true }) == nil and #SENT == 0, "a blocked press runs nothing")
release_all()

-- A game list (items, magic) has the keys: the pad is the game's, for its paging and bag switch.
LIST = { window = "items" }
assert(press("lt", true) == nil, "the trigger is the game's")
assert(press("b", true) == nil and #SENT == 0, "and so is the button")
LIST = nil
release_all()

-- Without a controller, or in keyboard mode, or with Hotbars off, nothing is taken.
CONNECTED = false
assert(press("lt", true) == nil)
CONNECTED = true
release_all()
OVER.on = false
assert(press("lt", true) == nil)
OVER.on = true
mode.set("hotbars")
assert(press("lt", true) == nil)
mode.set("gamepad")
release_all()

-- An open group list: the d-pad moves the cursor, A runs the row, B closes it; another slot closes it and fires.
BUTTONS.rt = true
assert(press("up", true) == true)
assert(chooser.is_open() and #SENT == 0, "the slot opens the list")
BUTTONS.up = false
assert(press("down", true) == true and press("down", false) == nil)
assert(press("a", true) == true and not chooser.is_open())
assert(SENT[1] == '/ja "Tenebrae" <me>' or SENT[1] == '/ja "Ignis" <me>', tostring(SENT[1]))
SENT = {}
BUTTONS.up = false
press("up", true)
assert(chooser.is_open())
assert(press("b", true) == true and not chooser.is_open() and #SENT == 0, "B closes the list")
release_all()
BUTTONS.rt = true
press("up", true)
assert(chooser.is_open())
BUTTONS.rt = nil
BUTTONS.lt = true
assert(press("x", true) == true and not chooser.is_open() and SENT[1] == "/p other", "another slot fires as usual")
release_all()

-- Assigning with the controller: a trigger and Back start it; a slot then opens the library's list, A puts the row
-- on the slot (a filled slot's list starts on its macro, under a Clear row); Back or B ends it.
assert(press("back", true) == nil and not pad.is_assigning(), "Back alone is the game's")
BUTTONS.lt = true
assert(press("back", true) == true and pad.is_assigning())
press("back", false)
assert(press("y", true) == true and chooser.is_open() and #SENT == 0, "a slot opens the library")
press("y", false)
assert(press("down", true) == true)
press("down", false)
assert(press("a", true) == true and not chooser.is_open())
press("a", false)
assert(data.pad_resolve("RUN", "lt", "y").ref == other, "the second entry went on the slot")
press("y", true)
press("y", false)
assert(press("up", true) == true)
press("up", false)
press("up", true)
press("up", false)
press("a", true)
press("a", false)
assert(data.pad_resolve("RUN", "lt", "y") == nil, "the Clear row empties it")
assert(press("b", true) == true and chooser.is_open(), "B with a trigger is a slot")
press("b", false)
assert(press("b", true) == true and not chooser.is_open() and pad.is_assigning(), "B closes the list")
press("b", false)
BUTTONS.lt = nil
assert(press("b", true) == true and not pad.is_assigning(), "B with no trigger ends it")
press("b", false)
BUTTONS.lt = true
press("back", true)
press("back", false)
BUTTONS.lt = nil
assert(pad.is_assigning() and press("back", true) == true and not pad.is_assigning(), "Back ends it")
release_all()

-- A trigger and Start open the manager; Start alone is the game's.
local managed = 0
local open_manager = pad.hooks.open_manager
pad.hooks.open_manager = function() managed = managed + 1 end
assert(press("start", true) == nil and managed == 0)
BUTTONS.rt = true
assert(press("start", true) == true and managed == 1)
pad.hooks.open_manager = open_manager
release_all()

-- Detect Gamepad: a controller coming or going picks the mode; a hand-made choice stands until the next change.
mode.use_setting()
mode.forget()
assert(mode.detect(false, true) == nil, "off: nothing changes")
mode.forget()
assert(mode.detect(true, true) == mode.GAMEPAD and mode.is_pad(), "a controller at the start")
assert(mode.detect(true, true) == nil)
mode.set("hotbars")
assert(mode.detect(true, true) == nil and not mode.is_pad(), "a hand-made choice stands")
assert(mode.detect(true, false) == nil, "already the hotbars")
assert(mode.detect(true, true) == mode.GAMEPAD, "it comes again")
assert(mode.detect(true, false) == mode.HOTBARS and not mode.is_pad(), "it goes")
mode.forget()
assert(mode.detect(true, false) == nil, "no controller at the start: the setting stands")
mode.use_setting()
mode.set("gamepad")
