-- VCHotbar: hotbars with multi-line macros that load by main job, then sub job.
-- See src/data.lua for the macros.json layout. //hb opens the manager.

local data = require("src.data")
local bar = require("src.bar")
local manager = require("src.manager")

local ok, err = data.init({
  read = fs.read,
  write = fs.write,
  decode = json.decode,
  encode = function(value)
    return json.encode(value, { pretty = true })
  end,
  log = log.warn,
})
if not ok then
  chat.add("VCHotbar: macros.json could not be read (" .. tostring(err) .. "); a copy was kept as macros.json.bad.")
end

bar.hooks.open_manager = manager.open
bar.hooks.edit_slot = manager.edit_slot
bar.hooks.changed = manager.refresh
manager.hooks.changed = bar.rebuild
manager.hooks.rebind = bar.rebind

-- Hides the game's own macro palette (drawing only; the client restores it when the mod
-- unloads). Without the ui.native permission the call raises, so say so once and carry on.
local warned_stock = false
local function apply_stock_palette()
  local hide = settings.get("hide_stock")
  local ok, err = pcall(ui.native.hide, "palette", hide)
  if not ok and hide and not warned_stock then
    warned_stock = true
    log.warn("VCHotbar: could not hide the stock macro bar: " .. tostring(err))
    chat.add("VCHotbar can't hide the stock macro bar yet: type /lua grant VCHotbar ui.native, "
      .. "then /lua reload VCHotbar.")
  end
end

apply_stock_palette()
bar.rebind()
bar.rebuild()

-- The jobs can arrive after the mod loads, and a job change swaps the macros.
events.on("job_change", function()
  bar.rebuild()
  manager.refresh()
end)
events.on("settings_changed", function()
  apply_stock_palette()
  bar.rebind()
  bar.rebuild()
  manager.refresh()
end)
timer.every(1, bar.tick)

local stock = require("src.stock")

local USAGE = "//hb [show | hide | run <bar> <slot> | reload | wipestock | restorestock]  (no argument opens the manager)"

commands.register("hb", function(cmd, a, b)
  cmd = (cmd or ""):lower()
  if cmd == "" then
    manager.toggle()
  elseif cmd == "show" then
    bar.set_hidden(false)
  elseif cmd == "hide" then
    bar.set_hidden(true)
  elseif cmd == "run" then
    local bn, sn = tonumber(a), tonumber(b)
    if bn and sn then
      bar.fire(bn, sn)
    else
      chat.add("VCHotbar: " .. USAGE)
    end
  elseif cmd == "wipestock" then
    stock.start_wipe()
  elseif cmd == "restorestock" then
    stock.restore()
  elseif cmd == "reload" then
    local loaded, why = data.load()
    if not loaded then
      chat.add("VCHotbar: macros.json could not be read (" .. tostring(why) .. ").")
    end
    bar.rebuild()
    manager.refresh()
  else
    chat.add("VCHotbar: " .. USAGE)
  end
end)
