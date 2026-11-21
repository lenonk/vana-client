-- RDM Buffs: RDM 99 / NIN 49 with Haste up and Refresh wearing off lists Refresh II (10 s left), Phalanx II and
-- Utsusemi: Ni (on recast); Cast All casts the first two in order and skips the one on recast; /buffs list says so.

local SET = {
  window = true, early = 20, composure = true, haste = true, refresh = true, phalanx = true, temper = true,
  gain = "Off", enspell = "Off", protect = true, regen = true, stoneskin = true, aquaveil = true, shadows = true,
  spikes = "Off", reraise = true, sublimation = true,
}
local SPELLS = {
  [57] = { name = "Haste", levels = { [5] = 48 }, mp = 40 },
  [109] = { name = "Refresh", levels = { [5] = 41 }, mp = 40 },
  [473] = { name = "Refresh II", levels = { [5] = 82 }, mp = 60 },
  [106] = { name = "Phalanx", levels = { [5] = 33 }, mp = 21 },
  [107] = { name = "Phalanx II", levels = { [5] = 75 }, mp = 42 },
  [53] = { name = "Blink", levels = { [5] = 23 }, mp = 20 },
  [338] = { name = "Utsusemi: Ichi", levels = { [13] = 12 }, mp = 0 },
  [339] = { name = "Utsusemi: Ni", levels = { [13] = 37 }, mp = 0 },
}

local tick, rows, cmd, handler
local casts, lines = {}, {}
settings = { get = function(k) return SET[k] end }
timer = { every = function(_, f) tick = f end }
events = { on = function() end }
res = { spell = function(id) return SPELLS[id] end, ability = function() end }
game = {
  player = function() return { main_job = 5, main_level = 99, sub_job = 13, sub_level = 49, mp = 900 } end,
  buffs = function() return { { id = 33, remaining = 200 }, { id = 43, remaining = 10 } } end,
  spells = function()
    local out = {}
    for id in pairs(SPELLS) do out[#out + 1] = id end
    return out
  end,
  abilities = function() return {} end,
  recasts = function() return { abilities = {}, spells = { [339] = 30 } } end,
  cast = function() return nil end,
  job_points = function() return {} end,
}
actions = { cast = function(id) casts[#casts + 1] = id end, ability = function() end }
wait_until = function() end
chat = { add = function(t) lines[#lines + 1] = t end }
commands = { register = function(_, f) cmd = f end }
ui = {
  window = function(_, spec)
    rows = spec.elements[1].rows
  end,
  open = function() end,
  close = function() end,
  on = function(_, f) handler = f end,
}
load(TEST.source("init.lua"), "@init.lua")()

tick()
local got = {}
for _, r in ipairs(rows) do got[#got + 1] = r.cells[1] .. "|" .. r.cells[2] end
assert(table.concat(got, ", ") == "Refresh II|10s left, Phalanx II|, Utsusemi: Ni|recast 30s", table.concat(got, ", "))
assert(rows[3].colour ~= nil and rows[1].colour == nil)

handler(nil, { kind = "press", id = "go", index = 1 })
assert(#casts == 2 and casts[1] == 473 and casts[2] == 107, table.concat(casts, " "))

cmd("list")
assert(#lines == 3 and lines[3] == "RDM Buffs: Utsusemi: Ni (recast 30s)", lines[3])
