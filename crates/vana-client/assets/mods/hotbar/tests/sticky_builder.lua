-- Hotbars: the Builder's Sticky choice saves a macro to every job's library, shows it set when editing one, and
-- set back takes it to the job's library.

res = { spell = function(id) if id == 1 then return { name = "Cure", mp = 8, kind = 1, targets = 0x1F } end end,
  ability = function() end, job = function(id) return ({ [5] = "RDM", [3] = "WHM" })[id] end }
game = { player = function() return { main_job = 5, sub_job = 3 } end,
  spells = function() return { 1 } end, abilities = function() return {} end,
  find_items = function() return {} end }
chat = { add = function() end }
local on_window = ui.on
ui.on = function(id, f) if id == "manager" then H = f end; on_window(id, f) end
local data = require("src.data")
data.init({ read = function() return nil end, write = function() end, decode = json.decode, encode = json.encode })
local manager = require("src.manager")
manager.hooks = { changed = function() end, rebind = function() end }
manager.open()

-- New, Cure, Sticky: every job, Save.
H("manager", { kind = "press", id = "lbtn", index = 1 })
assert(ui.get("manager", "sticky").at == 1)
H("manager", { kind = "select", id = "found", index = 1 })
H("manager", { kind = "set", id = "sticky", index = 2 })
H("manager", { kind = "press", id = "bbtn", index = 1 })
local lib = data.library("WHM")
assert(#lib == 1 and lib[1].sticky and lib[1].entry.name == "Cure")
local id = lib[1].id

-- Edit shows it sticky; This job only takes it to RDM's library.
manager.edit_entry(id)
assert(ui.get("manager", "sticky").at == 2)
H("manager", { kind = "set", id = "sticky", index = 1 })
H("manager", { kind = "press", id = "bbtn", index = 1 })
assert(#data.library("WHM") == 0 and #data.library("RDM") == 1 and not data.library("RDM")[1].sticky)
