-- What follows the pointer while a slot or a palette tile is dragged: a see-through copy of it (its icon or its
-- label). It has no handler, so the slot under it still gets the drop.

local M = {}

local LAYER = 20
local BG = { 22, 26, 40, 170 }
local EDGE = { 240, 220, 150, 220 }

local size = 0
local active = false

local function place(x, y)
  return x - size / 2, y - size / 2
end

-- A drop rebuilds the bars (hud.clear), so an element may be gone by the time its drag ends.
local function set(id, fields)
  pcall(hud.set, id, fields)
end

-- Starts it at the pointer (x, y): `act` is the macro's first action ({kind, id}) or nil, `label` its text.
function M.start(x, y, s, act, label)
  local u = hud.screen().ui_scale
  size = s
  active = true
  local gx, gy = place(x, y)
  hud.rect("ghost", { x = gx, y = gy, w = size, h = size, round = 6 * u, colour = BG, layer = LAYER })
  hud.rect("ghost_f", { x = gx - 1, y = gy - 1, w = size + 2, h = size + 2, round = 7 * u, colour = EDGE,
    line = 1.5 * u, layer = LAYER })
  if act then
    local isz = math.floor(size * 0.7)
    hud.icon("ghost_i", { x = gx + (size - isz) / 2, y = gy + (size - isz) / 2, size = isz, kind = act.kind,
      id = act.id, layer = LAYER + 1 })
  else
    pcall(hud.remove, "ghost_i")
  end
  hud.text("ghost_t", { x = gx + size / 2, y = gy + size / 2 - 7 * u, text = act and "" or (label or ""),
    size = 0.75 * size / (52 * u), align = "center", colour = { 255, 255, 255 }, outline = { 0, 0, 0, 230 },
    layer = LAYER + 2 })
end

function M.move(x, y)
  if not active then return end
  local gx, gy = place(x, y)
  local isz = math.floor(size * 0.7)
  set("ghost", { x = gx, y = gy })
  set("ghost_f", { x = gx - 1, y = gy - 1 })
  set("ghost_i", { x = gx + (size - isz) / 2, y = gy + (size - isz) / 2 })
  set("ghost_t", { x = gx + size / 2, y = gy + size / 2 - 7 * hud.screen().ui_scale })
end

function M.stop()
  if not active then return end
  active = false
  for _, id in ipairs({ "ghost", "ghost_f", "ghost_i", "ghost_t" }) do
    pcall(hud.remove, id)
  end
end

-- Handles the drag events of an element: `look()` gives (size, act, label) when the drag starts; the element `id`
-- is dimmed (to `dim`, back to `colour`) while it is dragged.
function M.follow(e, id, colour, dim, look)
  if e.kind == "drag_start" then
    local s, act, label = look()
    M.start(e.x, e.y, s, act, label)
    set(id, { colour = dim })
  elseif e.kind == "drag" then
    -- Over one of our windows (drawn over the HUD) it hides: the window draws its copy.
    if e.window then
      M.move(-10000, -10000)
    else
      M.move(e.x, e.y)
    end
  elseif e.kind == "drag_end" then
    M.stop()
    set(id, { colour = colour })
  end
end

return M
