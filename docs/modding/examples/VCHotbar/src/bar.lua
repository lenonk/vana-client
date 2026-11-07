-- The hotbars on screen: drawn with the HUD, clicked or fired by hotkey.
--
-- Slots whose macro comes from the main job's set get a gold border; slots the
-- sub job's set supplied (overriding main) get a blue one.

local data = require("src.data")
local runner = require("src.runner")
local jobs = require("src.jobs")
local keys = require("src.keys")

local M = {}

-- Set by init.lua: open_manager(), edit_slot(bar, slot, from)
M.hooks = {}

local hidden = false     -- toggled by //hb show|hide
local built_sig = nil    -- what the current drawing was made for

local EMPTY_BG = { 12, 12, 20, 150 }
local FILLED_BG = { 30, 30, 52, 225 }
local EMPTY_BORDER = { 70, 70, 80, 140 }
local BORDER = { main = { 170, 145, 80, 235 }, sub = { 90, 150, 220, 240 } }

local function opts()
  return {
    bars = math.min(settings.get("bars"), data.MAX_BARS),
    slots = math.min(settings.get("slots"), data.MAX_SLOTS),
    size = settings.get("size"),
    gap = settings.get("gap"),
    ox = settings.get("offset_x"),
    oy = settings.get("offset_y"),
    show_keys = settings.get("show_keys"),
  }
end

local function signature()
  local main, sub = jobs.current()
  local scr = hud.screen()
  return table.concat({ tostring(main), tostring(sub), scr.width, scr.height }, ":")
end

-- Runs the macro in a slot for the current jobs.
function M.fire(bar, slot)
  local main, sub = jobs.current()
  if not main then
    return
  end
  local macro = data.resolve(main, sub, bar, slot)
  if macro then
    runner.run_checked(macro)
  end
end

local function slot_menu(bar, slot)
  local main, sub = jobs.current()
  local _, from = data.resolve(main, sub, bar, slot)
  ui.menu({ "Edit / assign macro", "Clear slot", "Open manager" }, function(i)
    if i == 1 then
      M.hooks.edit_slot(bar, slot, from)
    elseif i == 2 then
      if main then
        data.clear(main, from == "sub" and sub or nil, bar, slot)
        M.rebuild()
        if M.hooks.changed then
          M.hooks.changed()
        end
      end
    elseif i == 3 then
      M.hooks.open_manager()
    end
  end)
end

local function on_click(bar, slot, button)
  if button == "right" then
    slot_menu(bar, slot)
  else
    M.fire(bar, slot)
  end
end

function M.rebuild()
  hud.clear()
  built_sig = signature()
  if hidden or not settings.get("visible") then
    return
  end
  local main, sub = jobs.current()
  if not main then
    built_sig = nil -- jobs not known yet; the tick tries again
    return
  end

  local o = opts()
  local scr = hud.screen()
  local s = scr.ui_scale
  local eff = data.effective(main, sub)

  local total_w = o.slots * o.size + (o.slots - 1) * o.gap
  local x0 = math.floor((scr.width - total_w) / 2) + o.ox
  local y_bottom = scr.height - o.oy - o.size

  -- Text scales with the slot, so a bigger slot gets bigger lettering.
  local tsize = math.max(0.5, math.min(1.2, 0.7 * o.size / 44))
  local line_h = 16 * tsize * s

  for b = 1, o.bars do
    -- Bar 1 is the top row; the last bar sits at the bottom offset.
    local y = y_bottom - (o.bars - b) * (o.size + o.gap)
    for i = 1, o.slots do
      local x = x0 + (i - 1) * (o.size + o.gap)
      local id = ("s%d_%d"):format(b, i)
      local cell = eff[b] and eff[b][i]
      local macro = cell and cell.macro

      hud.rect(id .. "_f", {
        x = x - 1, y = y - 1, w = o.size + 2, h = o.size + 2,
        colour = macro and BORDER[cell.from] or EMPTY_BORDER, layer = 0,
      })
      hud.rect(id, {
        x = x, y = y, w = o.size, h = o.size,
        colour = macro and FILLED_BG or EMPTY_BG, layer = 1,
      })
      hud.on(id, function(_, e)
        on_click(b, i, e.button)
      end)

      if macro then
        local label = data.display_label(macro)
        local n = select(2, label:gsub("\n", "")) + 1
        hud.text(id .. "_t", {
          x = x + o.size / 2, y = y + (o.size - n * line_h) / 2,
          text = label, size = tsize, align = "center",
          colour = { 255, 255, 255 }, outline = { 0, 0, 0 }, layer = 2,
        })
      end
      if o.show_keys then
        hud.text(id .. "_k", {
          x = x + 3, y = y + 1, text = keys.hint(b, i), size = tsize * 0.8,
          colour = { 200, 200, 210, 210 }, outline = { 0, 0, 0 }, layer = 3,
        })
      end
    end
  end

  -- Job label above the top bar; clicking it opens the manager.
  local top = y_bottom - (o.bars - 1) * (o.size + o.gap)
  hud.text("hdr", {
    x = x0, y = top - 16 * 0.75 * s - 6,
    text = main .. "/" .. (sub or "-"), size = 0.75,
    colour = { 230, 215, 170 }, bg = { 0, 0, 0, 140 }, layer = 2,
  })
  hud.on("hdr", function()
    M.hooks.open_manager()
  end)
end

function M.rebind()
  local o = opts()
  keys.bind_all(o.bars, o.slots, M.fire)
end

-- Called once a second: redraws when the jobs or the window size changed.
function M.tick()
  if signature() ~= built_sig then
    M.rebuild()
  end
end

function M.set_hidden(value)
  hidden = value
  M.rebuild()
end

function M.is_hidden()
  return hidden
end

return M
