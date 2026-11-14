-- Hotbars: multi-line macro bars loaded by main / sub job, with a macro library, builder and palette. Two modes: the
-- keyboard's hotbars (src/bar.lua) or the gamepad's crossbars (src/pad.lua). //hb opens the manager.

local data = require("src.data")
local bar = require("src.bar")
local manager = require("src.manager")
local stock = require("src.stock")
local palette = require("src.palette")
local jobs = require("src.jobs")
local chooser = require("src.chooser")
local mode = require("src.mode")
local pad = require("src.pad")

local ok, err = data.init({
  read = fs.read,
  write = fs.write,
  decode = json.decode,
  encode = function(value) return json.encode(value, { pretty = true }) end,
  log = log.warn,
})
local function read_failed(why)
  chat.add("Hotbars: " .. data.file() .. " could not be read (" .. tostring(why) .. "); a copy was kept as "
    .. data.file() .. ".bad.")
end
if not ok then read_failed(err) end

local function lists_changed()
  manager.refresh()
  palette.set_job(jobs.current())
end

local function changed()
  bar.rebuild()
  pad.invalidate()
  lists_changed()
end

local function toggle_palette()
  palette.set_job(jobs.current())
  palette.toggle()
end

bar.hooks.changed = lists_changed
bar.hooks.open_manager = manager.open
bar.hooks.palette = toggle_palette
pad.hooks.changed = lists_changed
pad.hooks.open_manager = manager.open
pad.hooks.palette = toggle_palette
pad.hooks.editing = function() return palette.is_open() or manager.is_open() end
manager.hooks.changed = function()
  bar.rebuild()
  pad.invalidate()
  palette.set_job(jobs.current())
end
manager.hooks.rebind = bar.rebind
manager.hooks.palette = toggle_palette
palette.hooks.edit = manager.edit_entry
palette.hooks.drag_ended = function()
  bar.tile_drag_ended()
  pad.tile_drag_ended()
end
local picked = chooser.hooks.picked
chooser.hooks.picked = function()
  if picked then picked() end
  pad.redraw()
end

local function apply_stock_palette()
  ui.native.hide("palette", settings.get("on") and settings.get("hide_stock"))
end

local function refresh()
  apply_stock_palette()
  bar.rebind()
  changed()
end

local detect
events.on("job_change", changed)
events.on("settings_changed", function(key)
  if key == "mode" then mode.use_setting() end
  if key == "pad_detect" then mode.forget() end
  refresh()
  if key == "pad_detect" then detect() end
end)
timer.every(1, bar.tick)
timer.every(0.25, bar.recasts)
timer.every(0.25, pad.recasts)
timer.every(0.25, pad.sync)
timer.every(1, pad.tick)

-- `to` nil: the other mode.
local function set_mode(to)
  if to then mode.set(to) else mode.toggle() end
  refresh()
  if mode.is_pad() then
    chat.add("Hotbars: gamepad crossbars"
      .. (input.pad().connected and "." or " (no controller is connected: they answer one once it is)."))
  else
    chat.add("Hotbars: keyboard hotbars.")
  end
end

-- Detect Gamepad: a controller connecting or disconnecting switches the mode. An unfocused window reads as
-- disconnected; that is ignored.
detect = function()
  local connected = input.pad().connected
  local ok, win = pcall(client.window)
  if not connected and ok and win and not win.focused then return end
  local to = mode.detect(settings.get("pad_detect"), connected)
  if not to then return end
  refresh()
  chat.add(to == mode.GAMEPAD and "Hotbars: controller found, gamepad crossbars."
    or "Hotbars: controller gone, keyboard hotbars.")
end
timer.every(0.5, detect)

-- The world's macro file, once game.info works.
local function use_world()
  local ok, info = pcall(game.info)
  if not ok or type(info) ~= "table" then return end
  local switched, loaded, why = data.set_world(info.server)
  if not switched then return end
  if loaded then changed() else read_failed(why) end
end

refresh()
use_world()
timer.every(1, use_world)

local function usage()
  chat.add("Hotbars:")
  for _, line in ipairs(commands.usage("hb")) do chat.add("  " .. line) end
end

commands.register("hb", function(cmd, a, b)
  cmd = (cmd or ""):lower()
  if cmd == "" then
    manager.toggle()
  elseif cmd == "show" then
    bar.set_hidden(false)
    pad.rebuild()
  elseif cmd == "hide" then
    bar.set_hidden(true)
    pad.rebuild()
  elseif cmd == "more" then
    bar.grow(1)
  elseif cmd == "fewer" then
    bar.grow(-1)
  elseif cmd == "palette" then
    toggle_palette()
  elseif cmd == "assign" then
    if mode.is_pad() and pad.is_active() then
      pad.set_assigning(not pad.is_assigning())
    else
      chat.add("Hotbars: assigning needs gamepad mode and a controller.")
    end
  elseif cmd == "mode" then
    if (a or "") == "" then
      set_mode(nil)
    elseif mode.parse(a) then
      set_mode(mode.parse(a))
    else
      usage()
    end
  elseif cmd == "run" then
    local bn, sn = tonumber(a), tonumber(b)
    if bn and sn then bar.fire(bn, sn) else usage() end
  elseif cmd == "wipestock" then
    stock.start_wipe()
  elseif cmd == "restorestock" then
    stock.restore()
  elseif cmd == "reload" then
    local loaded, why = data.load()
    if not loaded then read_failed(why) end
    changed()
  else
    usage()
  end
end)
