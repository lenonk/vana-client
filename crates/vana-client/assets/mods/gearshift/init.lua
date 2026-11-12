-- Gear swapping from players' files: a GearSwap job file (gearswap/ in this mod's data folder, named as GearSwap
-- names them), a LuAshitacast profile (ashitacast/<Name>_<Id>/<JOB>.lua or ashitacast/<Name>_<JOB>.lua) or an
-- Ashitacast XML profile (the same names, .xml), loaded for the main job, all run around the action packets we send:
-- the gear before an action, the action, the gear after it, in one batch. Or a Spellcast XML file (spellcast/), run
-- from the action's command line on Spellcast's clock.

local core = require("src.core")
local gs = require("src.gs")
local lac = require("src.lac")
local acxml = require("src.acxml")
local spellcast = require("src.spellcast")
local builder = require("src.build.run")
local builder_window = require("src.build.window")
builder_window.install()
-- The set builder's background work, one coroutine for the session.
timer.after(0, function() builder.worker() end)

-- A file the player wrote runs instead of the set builder.
local function file_loaded()
  return gs.loaded() or lac.loaded() or acxml.loaded() or spellcast.loaded()
end

local job_now

local function load_for_job()
  gs.unload()
  lac.unload()
  acxml.unload()
  spellcast.unload()
  if not settings.get("on") then return end
  local f = gs.find()
  if f then gs.load(f) return end
  f = lac.find()
  if f then lac.load(f) return end
  f = acxml.find()
  -- A job change keeps the XML profile's variables, as Ashitacast does.
  if f then acxml.load(f, true) return end
  f = spellcast.find()
  if f then spellcast.load(f) end
end

local function main_job()
  local p = game.player()
  return p and p.main_job
end

local started = false
local function start()
  started = true
  job_now = main_job()
  load_for_job()
end

-- The action packets we send: held, and sent again by the front end with the gear around them.
local function on_out(p)
  if p.injected then return end
  local act = core.parse_action(p)
  if not act then return end
  if gs.loaded() then return gs.action(act) end
  if lac.loaded() then return lac.action(act) end
  if acxml.loaded() then return acxml.action(act) end
  if not spellcast.loaded() then return builder.action(act) end
end
packets.on_out(0x01A, on_out)
packets.on_out(0x037, on_out)

filters.typed(function(line)
  gs.typed(line)
  local r = acxml.typed(line)
  if r ~= nil then return r end
  return spellcast.typed(line)
end)

events.on("action", function(a)
  gs.on_action(a)
  lac.on_action(a)
  acxml.on_action(a)
  if not file_loaded() then builder.on_action(a) end
end)
events.on("battle_message", function(actor, target, message) gs.battle_message(actor, target, message) end)
events.on("status_change", function(new, old)
  gs.status_change(new, old)
  spellcast.status_change(new)
  gs.user_event("status change", new, old)
end)
events.on("buff_gain", function(id)
  gs.buff_change(id, true)
  gs.user_event("gain buff", id)
end)
events.on("buff_lose", function(id)
  gs.buff_change(id, false)
  gs.user_event("lose buff", id)
end)
events.on("job_change", function(main, _, sub)
  local sub_changed = gs.sub ~= nil and gs.sub ~= sub
  if main ~= job_now and main ~= 0 then
    started, job_now = true, main
    load_for_job()
  elseif sub_changed and spellcast.loaded() then
    -- Spellcast looks for its file again on a sub job change too (Name_MAIN_SUB.xml).
    load_for_job()
  end
  if gs.loaded() and sub_changed then gs.sub_job_change(sub, gs.sub) end
  gs.sub = sub
end)
events.on("zone_change", function(zone, previous) gs.user_event("zone change", zone, previous) end)
events.on("target_change", function(id) gs.user_event("target change", id) end)
events.on("settings_changed", start)
-- The first frame our main job is known loads its file (the mod can start before the server has said).
local texts = require("src.texts")
events.on("frame", function()
  core.decode_augments()
  texts.frame()
  if not started and main_job() and main_job() ~= 0 then
    gs.sub = game.player().sub_job
    start()
  end
end)
timer.every(0.4, function()
  lac.tick()
  acxml.tick()
  if not file_loaded() then builder.tick() end
end)
timer.every(3.0, function() builder.refresh() end)
timer.every(0.5, function() builder_window.redraw() end)
events.on("item_change", function() builder.refresh() end)

-- For other mods (Bag Manager): the item names the loaded file's sets hold, and which file.
mods.provide("gear_names", function()
  if gs.loaded() then return { file = gs.file, names = gs.gear_names() } end
  if lac.loaded() then return { file = lac.file, names = lac.gear_names() } end
  if acxml.loaded() then return { file = acxml.file, names = acxml.gear_names() } end
  if spellcast.loaded() then return { file = spellcast.file, names = spellcast.gear_names() } end
  return nil
end)

commands.register("gs", function(...) gs.command({ ... }) end)
commands.register("lac", function(...) lac.command({ ... }) end)
commands.register("ac", function(...) acxml.command({ ... }) end)
-- /sc is the game's (search comment) until a Spellcast file is loaded; then the typed filter takes it.
commands.register("spellcast", function(...) spellcast.command({ ... }) end)
commands.register("gearshift", function(cmd, arg)
  cmd = (cmd or ""):lower()
  if cmd == "mode" then
    chat.add("Gearshift: " .. builder.toggle((arg or "off"):lower()))
    return
  end
  if cmd == "" then
    builder_window.toggle()
    return
  end
  if cmd == "reload" then start() end
  chat.add("Gearshift: " .. (gs.file or lac.file or acxml.file or spellcast.file
    or "no file for this job (gearswap/, ashitacast/ or spellcast/ in its data folder): the set builder's sets"))
end)

