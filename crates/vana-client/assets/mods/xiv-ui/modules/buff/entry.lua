-- XIV Buff Bar: entry. Hides the game's own status icons and runs the bar.
local bar = require("src.bar")
local panel_settings = settings
bar.bind_settings(panel_settings)

local function apply_hide()
  -- While this mod is loaded and on, the game's own status icons stay hidden.
  -- (Not while the bar has stopped on an error: then the native icons must stay visible.)
  pcall(ui.native.hide, "buffs", panel_settings.get("on") == true and not bar.failed())
end

apply_hide()
bar.tick()

-- A slow tick refreshes the countdown labels and the blink; the bar only
-- rebuilds its elements when the set of statuses changes.
timer.every(0.5, function() bar.tick() end)

-- A tick only rebuilds when the set of statuses actually changed, so a burst of events
-- (login, zone) costs one rebuild, not one per event. Zone/login still force a rebuild.
events.on("buff_gain", function() bar.tick() end)
events.on("buff_lose", function() bar.tick() end)
-- The other XIV mods re-assert their native hide on zone changes, so do the same here.
events.on("zone_change", function() apply_hide(); bar.rebuild() end)
events.on("login", function() apply_hide(); bar.rebuild() end)
events.on("settings_changed", function() bar.reset(); apply_hide() end)
events.on("unload", function() bar.clear() end)

commands.register("xbb", function(arg)
  arg = tostring(arg or ""):lower():gsub("^%s+", ""):gsub("%s+$", "")
  if arg == "lock" then
    chat.add("XIV Buff Bar: dragging " .. (bar.toggle_lock() and "locked" or "unlocked"))
  elseif arg == "refresh" then
    bar.reset()
    apply_hide()
    chat.add("XIV Buff Bar: rebuilt.")
  elseif arg == "resetpos" then
    bar.resetpos()
    apply_hide()
    chat.add("XIV Buff Bar: position reset to top-left default (8, 80 UI units). Drag the bar to move it.")
  else
    local n = bar.count()
    chat.add(string.format("XIV Buff Bar: %s, %d effect%s, %s",
      panel_settings.get("on") == true and "on" or "off", n, n == 1 and "" or "s",
      bar.locked() and "locked" or "unlocked"))
  end
end)
