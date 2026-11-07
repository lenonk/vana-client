-- Changes to the modern interface's own panels: the target window a quarter larger, the location panel replaced by
-- one compact line where it stood, your target's HP in large figures on the target window's corner, and warmer
-- colours.
-- /tweaks turns it all off and on again; unloading the mod gives everything back too.

local on = false

local function refresh()
  if not on then return end
  local zone = res.zone(game.zone() or 0) or ""
  local t = game.time()
  -- Where the location panel stood (anchored to it, it stays there while hidden).
  hud.set("where", { text = ("%s  %02d:%02d"):format(zone, t.hour, t.minute) })
  local target = game.target()
  hud.set("target", { text = target and ("%d%%"):format(target.hpp or 0) or "", visible = target ~= nil })
end

local function apply()
  on = true
  -- It stands on the party window at the right edge: grow it up and left.
  ui.native.place("target", { scale = 1.25, origin = "bottom_right" })
  ui.native.hide("where")
  -- The location panel stands on the chat window, so count from its bottom edge.
  hud.text("where", { anchor = "where", corner = "bottom_left", x = 8, y = -30, size = 1.1, bg = { 0, 0, 0, 170 }, tip = "The location panel, compact (panel-tweaks)" })
  -- Over its top right corner, just outside it (HUD elements draw under the panels).
  hud.text("target", { anchor = "target", corner = "top_right", x = -90, y = -40, size = 2, align = "right", outline = { 0, 0, 0 } })
  ui.skin.colour("gold", { 255, 196, 120 })
  ui.skin.frame("window_normal", { top = { 36, 28, 22, 235 }, bottom = { 14, 11, 9, 235 } })
  refresh()
end

local function undo()
  on = false
  ui.native.place("target", nil)
  ui.native.hide("where", false)
  ui.skin.reset()
  hud.clear()
end

commands.register("tweaks", function()
  if on then undo() else apply() end
  chat.add(on and "Panel tweaks on." or "Panel tweaks off.")
end)

timer.every(0.5, refresh)
if game.player() then apply() end
events.on("login", apply)
