-- The palette: the main job's library as HUD tiles. Drag one onto a slot to assign it; click to edit. The panel moves
-- by dragging (place kept per character).

local data = require("src.data")
local actions = require("src.actions")
local ghost = require("src.ghost")

local M = {}

-- Set by init.lua: edit(id), drag_ended().
M.hooks = {}

-- The tile being dragged ({id, act, label}, here or in the manager); `last` its id, kept for a drop after drag end.
M.dragging = nil
M.last = nil

function M.start_drag(id, act, label)
  M.dragging = { id = id, act = act, label = label }
  M.last = id
end

function M.end_drag()
  M.dragging = nil
  if M.hooks.drag_ended then M.hooks.drag_ended() end
end

local COLS, ROWS = 8, 2
local open = false
local page = 1
local drawn = {}   -- HUD element ids
local main = nil
local moving = nil -- panel origin at drag start

local TILE_BG = { 10, 17, 24, 235 }
local TILE_DIM = { 10, 17, 24, 90 }
local TILE_EDGE = { 184, 139, 73, 205 }
local PANEL_BG = { 6, 10, 16, 200 }
local PANEL_EDGE = { 184, 139, 73, 120 }
local TITLE = { 232, 210, 156 }
local INK = { 236, 230, 214 }
local OUTLINE = { 0, 0, 0, 230 }

-- The library id of a dragged HUD element, or nil.
function M.dragged_id(element_id)
  return element_id and tostring(element_id):match("^p_(.+)$")
end

function M.is_open() return open end

local function clear()
  for _, id in ipairs(drawn) do
    pcall(hud.remove, id)
  end
  drawn = {}
end

local function add(kind, id, spec)
  hud[kind](id, spec)
  drawn[#drawn + 1] = id
end

local function sizes()
  local u = hud.screen().ui_scale
  local tile = math.floor(settings.get("size") * 0.8 * u)
  local gap = math.floor(4 * u)
  local pad = math.floor(8 * u)
  local head = math.floor(20 * u)
  local w = COLS * tile + (COLS - 1) * gap + 2 * pad
  local h = head + ROWS * tile + (ROWS - 1) * gap + 2 * pad
  return u, tile, gap, pad, head, w, h
end

-- Top left: where it was left, clamped on screen.
local function place(w, h)
  local scr = hud.screen()
  local at = type(storage.character.hb_palette) == "table" and storage.character.hb_palette or {}
  local x = tonumber(at.x) or (scr.width - w) / 2
  local y = tonumber(at.y) or scr.height * 0.55
  return math.max(0, math.min(x, scr.width - w)), math.max(0, math.min(y, scr.height - h))
end

local function text_button(id, spec, fn)
  add("text", id, spec)
  hud.on(id, function(_, e)
    if e.kind == "click" then fn() end
  end)
end

function M.draw()
  clear()
  if not (open and main) then return end
  local u, tile, gap, pad, head, w, h = sizes()
  local x0, y0 = place(w, h)
  local items = data.library(main)
  local per = COLS * ROWS
  local pages = math.max(1, math.ceil(#items / per))
  page = math.min(page, pages)

  add("rect", "pal_f", { x = x0 - 1, y = y0 - 1, w = w + 2, h = h + 2, round = 8 * u, colour = PANEL_EDGE,
    line = 1.2 * u, layer = 0 })
  add("rect", "pal_bg", { x = x0, y = y0, w = w, h = h, round = 7 * u, colour = PANEL_BG, layer = 0 })
  hud.on("pal_bg", function(_, e)
    if e.kind == "drag_start" then
      moving = { x = x0, y = y0 }
    elseif e.kind == "drag" and moving then
      storage.character.hb_palette = { x = moving.x + e.dx, y = moving.y + e.dy }
      M.draw()
    elseif e.kind == "drag_end" then
      moving = nil
      storage.save()
    end
  end)
  local title = #items == 0 and ("%s macros: none yet (//hb, Library, New)"):format(main)
    or ("%s macros: drag one onto a slot"):format(main)
  add("text", "pal_title", { x = x0 + pad, y = y0 + 4 * u, text = title, size = 0.75, colour = TITLE,
    outline = OUTLINE, layer = 2 })
  local right = x0 + w - pad
  text_button("pal_close", { x = right, y = y0 + 3 * u, text = "x", size = 0.85, align = "right", colour = INK,
    outline = OUTLINE, layer = 2, tip = "Hide the palette" }, M.toggle)
  if pages > 1 then
    text_button("pal_next", { x = right - 22 * u, y = y0 + 3 * u, text = ">", size = 0.85, align = "right",
      colour = INK, outline = OUTLINE, layer = 2 }, function()
      page = page % pages + 1
      M.draw()
    end)
    add("text", "pal_page", { x = right - 38 * u, y = y0 + 4 * u, text = ("%d/%d"):format(page, pages), size = 0.75,
      align = "right", colour = INK, outline = OUTLINE, layer = 2 })
    text_button("pal_prev", { x = right - 76 * u, y = y0 + 3 * u, text = "<", size = 0.85, align = "right",
      colour = INK, outline = OUTLINE, layer = 2 }, function()
      page = (page - 2) % pages + 1
      M.draw()
    end)
  end

  local k = tile / (52 * u)
  local first = (page - 1) * per
  for n = 1, math.min(per, #items - first) do
    local it = items[first + n]
    local col, row = (n - 1) % COLS, (n - 1) // COLS
    local x = x0 + pad + col * (tile + gap)
    local y = y0 + head + pad + row * (tile + gap)
    local id = "p_" .. it.id
    local name = it.entry.name or ""
    local label = data.display_label(it.entry)
    local macro = data.macro_of(main, it.id)
    local act = macro and actions.of(macro)
    add("rect", id, { x = x, y = y, w = tile, h = tile, round = 6 * u, colour = TILE_BG, layer = 1,
      tip = { title = name, text = (macro and data.tip_text(macro) or "") .. "\n\nDrag onto a slot; click to edit." } })
    add("rect", id .. "_f", { x = x, y = y, w = tile, h = tile, round = 6 * u, colour = TILE_EDGE, line = 1.2 * u,
      layer = 2 })
    hud.on(id, function(_, e)
      if e.kind == "click" and e.button == "left" then
        M.hooks.edit(it.id)
      else
        ghost.follow(e, id, TILE_BG, TILE_DIM, function()
          return math.floor(settings.get("size") * u), act, label
        end)
        if e.kind == "drag_start" then
          M.start_drag(it.id, act, label)
        elseif e.kind == "drag_end" then
          M.end_drag()
        end
      end
    end)
    if act then
      local isz = math.floor(tile * 0.62)
      add("icon", id .. "_i", { x = x + (tile - isz) / 2, y = y + 3 * u, size = isz, kind = act.kind, id = act.id,
        layer = 2 })
    end
    add("text", id .. "_t", { x = x + tile / 2, y = act and (y + tile - 14 * k * u) or (y + tile / 2 - 6 * u),
      text = label:match("^[^\n]*"), size = 0.65 * k, align = "center", colour = INK, outline = OUTLINE, layer = 4 })
  end
end

function M.set_job(job)
  if job ~= main then page = 1 end
  main = job
  M.draw()
end

function M.toggle()
  open = not open
  M.draw()
end

return M
