-- Gearshift's set builder on a job with no TP set (BLM): engaged, it stays in its Idle set, and a set that fails to
-- build leaves the worker going.

local HEAD, BLM, ALL = 1 << 4, 1 << 4, 0x1fe
local ITEMS = {
  [2001] = { name = "Wise Hat", slots = HEAD, jobs = BLM, races = ALL, level = 1, description = "DEF:5 INT+3" },
}
game = {
  player = function() return { name = "Tester", id = 100, index = 1, main_job = 4, main_level = 75, sub_job = 0,
    status = 1, pos = { x = 0, y = 0, z = 0 } } end,
  entity = function() return { race = 1 } end,
  entities = function() return {} end,
  equipment = function() return {} end,
  items = function(bag)
    if bag ~= 0 then return {} end
    return { { bag = 0, slot = 1, id = 2001, count = 1, locked = 0 } }
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

local run = require("src.build.run")
run.RANGES = { { 2001, 2002 } }
run.refresh()
for _ = 1, 80 do run.frame() end
assert(run.state.job == "BLM", tostring(run.state.job))

-- Engaged with no TP set: the Idle set, never no set.
assert(run.default_set() == "Idle", tostring(run.default_set()))
run.set_active(true)
run.rebuild()
run.tick()
for i = 1, #run.state.queue do assert(run.state.queue[i] ~= nil, "a nil set queued") end
run.want_first(nil)
assert(run.state.queue[1] ~= nil, "want_first queued no set")

-- A set whose build fails: the worker's step goes on to the next.
local said, add, build = {}, chat.add, run.build
chat.add = function(t) said[#said + 1] = t end
run.build = function() error("boom") end
run.state.queue = { "Idle", "Nuke" }
assert(run.step() == true and #run.state.queue == 1, "the step stopped at the failed set")
run.build = build
chat.add = add
assert(said[1] and said[1]:find("Idle set could not be built"), tostring(said[1]))
assert(run.step() == true and run.state.built.Nuke, "the next set not built")
