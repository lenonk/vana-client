-- Gearshift's set builder: a better piece we keep in the Mog Safe is not offered as worth getting; it is named with
-- the bag it is in, and the set takes the best piece we can wear from.

local HEAD, BLM, ALL = 1 << 4, 1 << 4, 0x1fe
local ITEMS = {
  [2001] = { name = "Plain Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+1" },
  [2002] = { name = "Wise Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+9" },
  [2003] = { name = "Shop Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+5" },
}
local BAGS = { [0] = { { bag = 0, slot = 1, id = 2001, count = 1, locked = 0 } },
  [1] = { { bag = 1, slot = 1, id = 2002, count = 1, locked = 0 } } }
game = {
  player = function() return { name = "Tester", id = 100, index = 1, main_job = 4, main_level = 75, sub_job = 0,
    status = 0, pos = { x = 0, y = 0, z = 0 } } end,
  entity = function() return { race = 1 } end,
  entities = function() return {} end,
  equipment = function() return {} end,
  items = function(bag) return BAGS[bag] or {} end,
  time = function() return { weekday = 0 } end, weather = function() return 0 end, buffs = function() return {} end,
}
res = setmetatable({
  item = function(id) return ITEMS[id] end,
  job = function(id) return ({ "WAR", "MNK", "WHM", "BLM" })[id] end,
}, { __index = function() return function() return nil end end })
storage = { character = { builder = { jobs = {}, modes = {} } } }
settings = { get = function(k) if k == "cap" then return "Auto" end return true end }
packets.inject_out = function() end

local run = require("src.build.run")
run.RANGES = { { 2001, 2004 } }
run.refresh()
for _ = 1, 80 do run.frame() end
local head = run.state.built.Nuke and run.state.built.Nuke.slots[4]
assert(head and head.pick and ITEMS[head.pick.id].name == "Plain Hat", "the set took a hat out of reach")
assert(head.worth and ITEMS[head.worth.id].name == "Wise Hat", tostring(head.worth and ITEMS[head.worth.id].name))
assert(head.worth_bag == 1, "the Mog Safe hat offered as worth getting: " .. tostring(head.worth_bag))
