-- Shared by the bars and the crossbars: slot drawing, canvas drag and drop, the drag ghost, group list placement.

local data = require("src.data")
local actions = require("src.actions")
local palette = require("src.palette")
local chooser = require("src.chooser")

local M = {}

M.KEY = { 10, 17, 24, 255 }
M.KEY_HOVER = { 20, 33, 44, 255 }
M.EMPTY_KEY = { 10, 17, 24, 150 }
M.EMPTY_EDGE = { 184, 139, 73, 70 }
M.TEXT = { 236, 230, 214, 255 }
M.MUTED = { 196, 188, 170, 230 }
M.GOLD = { 228, 192, 119, 255 }
local FLASH = { 255, 236, 190, 120 }
local SHADE = { 0, 0, 0, 165 }
local UNUSABLE = { 6, 8, 14, 165 }
local GHOST = { 255, 255, 255, 140 }
local GHOST_BG = { 20, 24, 38, 120 }
local DROP = { 255, 214, 120, 200 }
local DRAGGED = { 0, 0, 0, 120 }

local LABEL = 0.78
local LABEL_LINE = 13
local ROUND = 7

-- hover: key under the pointer; flash: key -> until; drag: {cell, x, y}; tile_at: a palette tile over us {x, y}.
function M.state()
  return { hover = nil, flash = {}, drag = nil, tile_at = nil }
end

function M.reset(st)
  st.hover, st.drag, st.tile_at = nil, nil, nil
end

-- Cuts a line to fit a slot `w` wide.
function M.fit(text, w)
  local n = math.max(1, math.floor((w - 8) / (6.5 * LABEL)))
  if utf8.len(text) and utf8.len(text) > n then
    return text:sub(1, utf8.offset(text, n) - 1) .. "…"
  end
  return text
end

-- {left, frac, usable}, or nil without an action.
function M.shade(macro, recasts, me)
  local act = actions.of(macro)
  if not act then return nil end
  local left, frac = actions.recast(act, recasts)
  return { left = left, frac = frac, usable = actions.usable(act, me) }
end

function M.flash(st, key, draw)
  st.flash[key] = os.clock() + 0.12
  draw()
  timer.after(0.13, draw)
end

-- c: x, y, s, macro, act, edge, hover, shade, dim, up (group arrow points down), hint, fallback (tip title),
-- before / after(add), st, key, source (being dragged), hit_key(x, y).
function M.draw_slot(add, c)
  local x, y, s, macro, act, sh, st = c.x, c.y, c.s, c.macro, c.act, c.shade, c.st
  add({ type = "rect", x = x, y = y, w = s, h = s, round = ROUND,
    colour = c.hover and M.KEY_HOVER or (macro and M.KEY or M.EMPTY_KEY) })
  add({ type = "rect", x = x, y = y, w = s, h = s, round = ROUND, line = 1.5, colour = c.edge })
  if c.before then c.before(add) end
  if act then
    local isz = math.floor(s * 0.58)
    add({ type = "icon", x = x + (s - isz) / 2, y = y + 4, size = isz, kind = act.kind, id = act.id,
      colour = c.dim and { 150, 150, 150, 255 } or nil })
  end
  if macro and macro.group then
    local ax = x + s - 9
    local base, tip = y + 10, y + 6
    if c.up then base, tip = tip, base end
    add({ type = "tri", x1 = ax - 4, y1 = base, x2 = ax + 4, y2 = base, x3 = ax, y3 = tip, colour = M.GOLD })
  end
  if sh and sh.left > 0 then
    add({ type = "rect", x = x, y = y, w = s, h = math.max(2, s * sh.frac), round = ROUND, colour = SHADE })
  elseif sh and not sh.usable then
    add({ type = "rect", x = x, y = y, w = s, h = s, round = ROUND, colour = UNUSABLE })
  end
  if macro then
    local label = data.display_label(macro)
    if act then
      add({ type = "text", x = x + s / 2, y = y + s - LABEL_LINE - 2, text = M.fit(label:match("^[^\n]*"), s),
        size = LABEL, align = "center", colour = M.TEXT })
    else
      local lines = {}
      for l in label:gmatch("[^\n]+") do lines[#lines + 1] = l end
      local top = y + (s - #lines * LABEL_LINE) / 2
      for n, l in ipairs(lines) do
        add({ type = "text", x = x + s / 2, y = top + (n - 1) * LABEL_LINE, text = M.fit(l, s), size = LABEL,
          align = "center", colour = M.TEXT })
      end
    end
  end
  if sh and sh.left > 0 then
    local secs = math.ceil(sh.left)
    local t = secs >= 60 and ("%d:%02d"):format(secs // 60, secs % 60) or tostring(secs)
    add({ type = "text", x = x + s / 2, y = y + s / 2 - 11, text = t, size = 1.45, font = "bold",
      align = "center", colour = { 255, 236, 180, 255 } })
  end
  if c.hint then
    add({ type = "text", x = x + 4, y = y + 2, text = c.hint, size = 0.9, colour = M.MUTED })
  end
  if c.after then c.after(add) end
  if st.flash[c.key] and st.flash[c.key] > os.clock() then
    add({ type = "rect", x = x, y = y, w = s, h = s, round = ROUND, colour = FLASH })
  end
  local drop = false
  if st.drag and st.drag.x then
    if c.source then
      add({ type = "rect", x = x, y = y, w = s, h = s, round = ROUND, colour = DRAGGED })
    else
      drop = c.hit_key(st.drag.x, st.drag.y) == c.key
    end
  elseif st.tile_at then
    drop = c.hit_key(st.tile_at.x, st.tile_at.y) == c.key
  end
  if drop then
    add({ type = "rect", x = x - 1, y = y - 1, w = s + 2, h = s + 2, round = ROUND + 1, line = 2, colour = DROP })
  end
  if macro then
    local name = data.trim(macro.name)
    add({ type = "tip", x = x, y = y, w = s, h = s, title = name ~= "" and name or c.fallback,
      text = data.tip_text(macro) })
  end
end

-- The dragged slot or palette tile under the pointer.
function M.draw_ghost(add, st, s, macro_of)
  local g, act, label
  if st.drag and st.drag.x then
    local macro = macro_of(st.drag.cell)
    g, act, label = st.drag, macro and actions.of(macro), macro and data.display_label(macro)
  elseif st.tile_at and palette.dragging then
    g, act, label = st.tile_at, palette.dragging.act, palette.dragging.label
  end
  if not g then return end
  local gx, gy = g.x - s / 2, g.y - s / 2
  add({ type = "rect", x = gx, y = gy, w = s, h = s, round = ROUND, colour = GHOST_BG })
  if act then
    local isz = math.floor(s * 0.58)
    add({ type = "icon", x = gx + (s - isz) / 2, y = gy + 4, size = isz, kind = act.kind, id = act.id, colour = GHOST })
  elseif label then
    add({ type = "text", x = gx + s / 2, y = gy + s / 2 - LABEL_LINE / 2, text = M.fit(label:match("^[^\n]*"), s),
      size = LABEL, align = "center", colour = GHOST })
  end
end

-- A group list's place over the slot at (x, y) (under it when docked on top), from the canvas bounds `at`.
function M.list_place(at, x, y, s, top_dock)
  local u = hud.screen().ui_scale
  local cx = at.x + (x + s / 2) * at.scale - chooser.WIDTH * u / 2
  if top_dock then
    return { x = cx, y = at.y + (y + s) * at.scale + 6 * u }
  end
  return { x = cx, y = at.y + y * at.scale - 6 * u, above = true }
end

-- A slot canvas's events. h: st, hit(x, y), key_of, macro_of, same(a, b), draw, drop(cell, id), fire, menu,
-- swap(from, to), other(thing) for clicks off slots. Returns whether it handled the event.
function M.handle(e, h)
  local st = h.st
  if e.kind == "drop" then
    st.tile_at = nil
    local c = h.hit(e.x, e.y)
    local id = palette.dragged_id(e.from) or (e.from == "tiles" and palette.last)
    if type(c) == "table" and id then
      h.drop(c, id)
    else
      h.draw()
    end
  elseif e.kind == "hover" or e.kind == "leave" then
    local now = e.kind == "hover" and h.key_of(h.hit(e.x, e.y)) or nil
    if now ~= st.hover then
      st.hover = now
      h.draw()
    end
  elseif e.kind == "drag_over" or e.kind == "drag_out" then
    st.tile_at = e.kind == "drag_over" and palette.dragging and { x = e.x, y = e.y } or nil
    h.draw()
  elseif e.kind == "click" then
    local c = h.hit(e.x, e.y)
    if type(c) == "table" then
      if e.button == "right" then
        h.menu(c)
      else
        M.flash(st, h.key_of(c), h.draw)
        h.fire(c)
      end
    elseif c and h.other then
      h.other(c)
    end
  elseif e.kind == "drag_start" then
    local c = h.hit(e.x, e.y)
    st.drag = type(c) == "table" and h.macro_of(c) and { cell = c, x = e.x, y = e.y } or nil
    if st.drag then h.draw() end
  elseif e.kind == "drag" and st.drag then
    st.drag.x, st.drag.y = e.x, e.y
    h.draw()
  elseif e.kind == "drag_end" and st.drag then
    local to = h.hit(e.x, e.y)
    local from = st.drag.cell
    st.drag = nil
    h.draw()
    if type(to) == "table" and not h.same(from, to) then h.swap(from, to) end
  else
    return false
  end
  return true
end

return M
