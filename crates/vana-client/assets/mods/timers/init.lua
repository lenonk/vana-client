-- Timers (the timers add-ons): our recasts, the effects on us and our party, and the ones we put on monsters, each in
-- a separate edge window so /uimodes shows and hides each. The classic UI draws them as the one Timers window with
-- three sections. The tracking and LandSandBoat's durations are this mod's (engine.lua, durations.lua); other mods add
-- rows through its service (the `timers` namespace calls it), shown whether the setting is on or not.

local E = require("engine")
local R = require("rows")

-- A client older than the mod's rows (no res.status_info) fills the sections from its tracking.
local OWN_ROWS = res.status_info ~= nil

local WINDOWS = {
  { id = "recasts", title = "Recasts" },
  { id = "buffs", title = "Buffs" },
  { id = "debuffs", title = "Debuffs" },
}

local function show()
  for _, w in ipairs(WINDOWS) do
    ui.window(w.id, {
      edge = true, panel = "timers_" .. w.id, resize = true, title = w.title,
      -- builtin: an older client fills the section from its tracking (it does not read `groups`).
      elements = { { type = "timers", id = "rows", section = w.id, builtin = settings.get("on") == true, groups = {} } },
    })
    ui.open(w.id)
  end
end

local function refresh()
  if not OWN_ROWS then return end
  local secs = R.sections(settings.get("on") == true)
  for k, w in ipairs(WINDOWS) do
    ui.update(w.id, "rows", { groups = secs[k] })
  end
end

show()
events.on("settings_changed", function()
  show()
  refresh()
end)

-- The windows follow at the next refresh (every action in the zone comes here).
events.on("action", function(a) E.on_action(a) end)
events.on("battle_message", function(_, target, message, param)
  E.on_message(target, param, message)
end)
events.on("party_buffs", function() E.on_party_effects() end)
events.on("zone_change", function() E.on_zone() end)

timer.every(0.25, function()
  E.maintain()
  refresh()
end)

-- The `timers` namespace's calls from other mods (with their id first).
-- They show at the next refresh (the caller may be where game state cannot be read).
mods.provide("add", function(mod_id, name, seconds, options) R.add(mod_id, name, seconds, options) end)
mods.provide("remove", function(mod_id, name) R.remove(mod_id, name) end)
mods.provide("clear", function(mod_id) R.clear(mod_id) end)
-- The effects we keep on an entity: {status, name, left, secs}.
mods.provide("effects_on", function(id) return E.effects_on(id) end)
