-- Gearshift's set builder: the best in slot, held, is the slot's pick, though another held piece carries more of its
-- stats (the share would take that one).

local HEAD, BLM, ALL = 1 << 4, 1 << 4, 0x1fe
local ITEMS = {
  [2001] = { name = "Big Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+9" },
  [2002] = { name = "Ref Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+4" },
}
game = {
  player = function() return { name = "Tester", id = 100, index = 1, main_job = 4, main_level = 75, sub_job = 0,
    status = 0, pos = { x = 0, y = 0, z = 0 } } end,
  entity = function() return { race = 1 } end,
  entities = function() return {} end,
  equipment = function() return {} end,
  items = function(bag)
    if bag ~= 12 then return {} end
    return { { bag = 12, slot = 1, id = 2001, count = 1, locked = 0 }, { bag = 12, slot = 2, id = 2002, count = 1, locked = 0 } }
  end,
  time = function() return { weekday = 0 } end, weather = function() return 0 end, buffs = function() return {} end,
}
res = setmetatable({
  item = function(id) return ITEMS[id] end,
  job = function(id) return ({ "WAR", "MNK", "WHM", "BLM" })[id] end,
}, { __index = function() return function() return nil end end })
storage = { character = { builder = { jobs = {}, modes = {} } } }
settings = { get = function(k) if k == "cap" then return "Auto" end return true end }
packets.inject_out = function() end
-- The curated list names the lesser hat for the Nuke set.
require("src.build.bis").lookup = function(_, _, set) return set == "Nuke" and { head = "Ref Hat" } or nil end

local run = require("src.build.run")
run.RANGES = { { 2001, 2003 } }
run.refresh()
for _ = 1, 80 do run.frame() end
local head = run.state.built.Nuke and run.state.built.Nuke.slots[4]
assert(head and head.ref and ITEMS[head.ref.id].name == "Ref Hat", "the curated hat is not the reference")
assert(head.pick and ITEMS[head.pick.id].name == "Ref Hat", "held best in slot passed over for " ..
  tostring(head.pick and ITEMS[head.pick.id].name))
