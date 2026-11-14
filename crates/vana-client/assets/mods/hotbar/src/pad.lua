-- Gamepad crossbars: three sets (LT, RT, both held) of twelve slots (d-pad, face buttons, L1 / R1, L3 / R3) in an edge
-- window. With Auto-hide it shows only the set held and folds to an "Open Manager" button otherwise.
--
-- Controller: trigger + slot button fires; trigger + Back toggles assigning (a slot press opens the library to pick
-- its macro); trigger + Start opens the manager. Game lists (magic, items...) keep the pad.

local data = require("src.data")
local runner = require("src.runner")
local jobs = require("src.jobs")
local actions = require("src.actions")
local chooser = require("src.chooser")
local menu = require("src.menu")
local mode = require("src.mode")
local bar = require("src.bar")
local slotview = require("src.slotview")

local M = {}

-- Set by init.lua: changed(), open_manager(), palette(), editing() (manager or palette open).
M.hooks = {}

local WIN = "pads"

local SIDES, BUTTONS = data.PAD_SIDES, data.PAD_BUTTONS
local LABELS = { up = "Up", right = "Right", down = "Down", left = "Left", y = "Y", b = "B", a = "A", x = "X",
  lb = "L1", rb = "R1", l3 = "L3", r3 = "R3" }
local SIDE_NAMES = { lt = "LT", rt = "RT", lr = "LT + RT" }
local SLOT = {}
for _, b in ipairs(BUTTONS) do SLOT[b] = true end
local IS_TRIGGER = { lt = true, rt = true }

local EDGE = { 184, 139, 73, 205 }
local EDGE_HELD = { 228, 192, 119, 255 }
local OFF_EDGE = { 80, 84, 96, 120 }
local KEY, KEY_HOVER, TEXT, GOLD = slotview.KEY, slotview.KEY_HOVER, slotview.TEXT, slotview.GOLD
local DIMMED = { 140, 140, 150, 255 }

local PAD = 8
local HEAD = 16                -- set names over the diamonds
local IDLE_W, IDLE_H = 120, 24 -- the folded window's button

local st = slotview.state()
local geo = nil          -- current layout: size, slots ({x, y, side, button}), sides, width, height
local layouts = {}       -- "view:size" -> layout
local cells = {}         -- "side:button" -> macro
local shades = {}        -- "side:button" -> recast shade
local main = nil
local built_sig = nil
local built_view = nil   -- "all", "lt", "rt", "lr" or "none"
local built_at = nil     -- os.clock() of the last window define
local window_key = nil
local drawn_held = nil
local top_dock = false
local idle_hover = false
local assigning = false

local function key_of(c) return type(c) == "table" and (c.side .. ":" .. c.button) or c end
local function where(side, button) return SIDE_NAMES[side] .. " " .. LABELS[button] end

local function changed()
  M.rebuild()
  if M.hooks.changed then M.hooks.changed() end
end

-- ---------------------------------------------------------------------------
-- State
-- ---------------------------------------------------------------------------

function M.is_active()
  return settings.get("on") and mode.is_pad() and input.pad().connected and true or false
end

function M.is_assigning() return assigning end

-- A game list has focus: the pad is the game's.
local function game_menu()
  local list = ui.native and ui.native.list
  if not list or chooser.is_open() then return false end
  local ok, open = pcall(list)
  return ok and open ~= nil
end

-- "lt", "rt" or "lr" for the triggers held, else nil.
local function held()
  local p = input.pad()
  if not p.connected or game_menu() then return nil end
  local l, r = p.buttons.lt, p.buttons.rt
  if l and r then return "lr" end
  if l then return "lt" end
  if r then return "rt" end
  return nil
end

-- "all", the set held, or "none" (folded). Every set shows while assigning or editing.
local function view()
  if settings.get("pad_autohide") and M.is_active() and not assigning
      and not (M.hooks.editing and M.hooks.editing()) then
    return held() or "none"
  end
  return "all"
end

-- ---------------------------------------------------------------------------
-- Slots
-- ---------------------------------------------------------------------------

local function slot_of(side, button)
  for _, s in ipairs(geo and geo.slots or {}) do
    if s.side == side and s.button == button then return s end
  end
end

-- Calls open(place) with where a list opens over a slot. A rebuilt window's canvas bounds lag a frame: wait for them.
local function open_at(side, button, tries, open)
  local at = ui.bounds and ui.bounds(WIN, "c")
  local s = slot_of(side, button)
  local fresh = at and s and math.abs(at.w - geo.width * at.scale) < 2 and math.abs(at.h - geo.height * at.scale) < 2
  if not fresh and built_at and os.clock() - built_at < 0.3 and tries < 8 then
    timer.after(0.03, function() open_at(side, button, tries + 1, open) end)
    return
  end
  open(fresh and slotview.list_place(at, s.x, s.y, geo.size, top_dock) or nil)
end

-- Runs a slot; a group opens its list.
function M.fire(side, button)
  local mj = jobs.current()
  local macro = mj and data.pad_resolve(mj, side, button)
  if not macro then return end
  if macro.group then
    open_at(side, button, 0, function(place) chooser.open(macro, place, nil, nil) end)
  else
    runner.run_checked(macro)
  end
end

-- Opens the library over a slot; the row picked goes on it (a "Clear" row first when it is filled).
function M.assign(side, button)
  local mj = jobs.current()
  if not mj then return end
  local lib = data.library(mj)
  if #lib == 0 then
    chat.add("Hotbars: the library is empty. //hb opens the manager to build macros.")
    return
  end
  local cur = data.pad_get_raw(mj, side, button)
  local rows, ids, start = {}, {}, 1
  if cur then
    rows[1] = { "Clear this slot", "", "", colour = DIMMED }
    ids[1] = false
  end
  for _, item in ipairs(lib) do
    local macro = data.macro_of(mj, item.id)
    if macro then
      local act = actions.of(macro)
      rows[#rows + 1] = { data.name_of(macro), "", "", icon = act and { kind = act.kind, id = act.id } or nil }
      ids[#rows] = item.id
      if cur and cur.ref == item.id then start = #rows end
    end
  end
  open_at(side, button, 0, function(place)
    chooser.pick_list(where(side, button), rows, place, function(k)
      if ids[k] then data.pad_assign(mj, side, button, ids[k]) else data.pad_clear(mj, side, button) end
      changed()
    end, start)
  end)
end

function M.set_assigning(on)
  on = on and M.is_active() or false
  if on == assigning then return end
  assigning = on
  if not on then chooser.close() end
  chat.add(on and "Hotbars: assigning. Hold a trigger and press a slot to pick its macro. Back or B ends it."
    or "Hotbars: assigning ended.")
  M.rebuild()
end

local function open_menu(side, button)
  local mj = jobs.current()
  local macro = mj and data.pad_resolve(mj, side, button)
  local items = {}
  if macro then
    items[#items + 1] = { "Clear the slot", function()
      data.pad_clear(mj, side, button)
      changed()
    end }
  end
  items[#items + 1] = { "Open the palette", function() M.hooks.palette() end }
  items[#items + 1] = { "Open the manager", function() M.hooks.open_manager() end }
  menu.open("Crossbar slot", macro, where(side, button), items)
end

-- ---------------------------------------------------------------------------
-- Controller
-- ---------------------------------------------------------------------------

-- Runs fn next tick (the filter must answer at once) and takes the press; releases pass.
local function take(e, fn)
  if e.down and fn then timer.after(0, fn) end
  return e.down or nil
end

local LIST_KEYS = {
  up = function() chooser.step(-1) end,
  down = function() chooser.step(1) end,
  a = function() chooser.accept() end,
  b = function() chooser.close() end,
}

input.on_pad(function(e)
  if not mode.is_pad() then return nil end
  if IS_TRIGGER[e.button] then timer.after(0, M.sync) end
  if e.blocked or not M.is_active() then return nil end
  local b = e.button
  if chooser.is_open() then
    if LIST_KEYS[b] then return take(e, LIST_KEYS[b]) end
    if e.down and held() and SLOT[b] then chooser.close() end
  end
  if game_menu() then return nil end
  if IS_TRIGGER[b] then return take(e) end
  local side = held()
  if b == "back" and (assigning or side) then
    return take(e, function() M.set_assigning(not assigning) end)
  end
  if b == "start" and side then return take(e, M.hooks.open_manager) end
  if b == "b" and assigning and not side then
    return take(e, function() M.set_assigning(false) end)
  end
  if not side or not SLOT[b] then return nil end
  local pick = assigning
  return take(e, function()
    if pick then M.assign(side, b) else M.fire(side, b) end
  end)
end)

-- ---------------------------------------------------------------------------
-- Drawing
-- ---------------------------------------------------------------------------

local function hit(x, y)
  if not geo then return nil end
  for _, sl in ipairs(geo.slots) do
    if x >= sl.x and x < sl.x + geo.size and y >= sl.y and y < sl.y + geo.size then return sl end
  end
end

local function macro_of(c) return cells[key_of(c)] end

local function draw()
  if not geo then return end
  local d = {}
  local function add(c) d[#d + 1] = c end
  local s = geo.size
  local active = held()
  drawn_held = active
  for _, side in ipairs(geo.sides) do
    local lit = active == nil or active == side
    add({ type = "text", x = geo.side_x[side] + geo.side_w / 2, y = geo.side_y[side], text = SIDE_NAMES[side],
      size = 0.9, align = "center", colour = lit and GOLD or DIMMED })
  end
  for _, sl in ipairs(geo.slots) do
    local key = key_of(sl)
    local macro = cells[key]
    local lit = active == nil or active == sl.side
    slotview.draw_slot(add, {
      x = sl.x, y = sl.y, s = s, macro = macro, act = macro and actions.of(macro), shade = shades[key],
      up = top_dock, hover = st.hover == key, dim = not lit,
      edge = active == sl.side and EDGE_HELD or (not lit and OFF_EDGE) or (macro and EDGE or slotview.EMPTY_EDGE),
      hint = LABELS[sl.button], fallback = LABELS[sl.button],
      st = st, key = key, source = st.drag and st.drag.cell == sl,
      hit_key = function(px, py) return key_of(hit(px, py)) end,
    })
  end
  slotview.draw_ghost(add, st, s, macro_of)
  ui.update(WIN, "c", { h = geo.height, draw = d })
end

local function draw_idle()
  ui.update(WIN, "c", { draw = {
    { type = "rect", x = 0, y = 0, w = IDLE_W, h = IDLE_H, round = 7, colour = idle_hover and KEY_HOVER or KEY },
    { type = "rect", x = 0, y = 0, w = IDLE_W, h = IDLE_H, round = 7, line = 1.5,
      colour = idle_hover and EDGE_HELD or EDGE },
    { type = "text", x = IDLE_W / 2, y = 5, text = "Open Manager", size = 0.9, align = "center",
      colour = idle_hover and GOLD or TEXT },
  } })
end

local function refresh_cells()
  cells = {}
  for _, sl in ipairs(geo.slots) do cells[key_of(sl)] = data.pad_resolve(main, sl.side, sl.button) end
end

-- Every quarter second.
function M.recasts()
  if not geo then return end
  local r = game.recasts()
  local me = game.player()
  shades = {}
  for key, macro in pairs(cells) do shades[key] = slotview.shade(macro, r, me) end
  draw()
end

-- After a group member was picked.
function M.redraw()
  if not geo or not main then return end
  refresh_cells()
  draw()
end

function M.tile_drag_ended()
  if st.tile_at then
    st.tile_at = nil
    draw()
  end
end

-- ---------------------------------------------------------------------------
-- Window
-- ---------------------------------------------------------------------------

local function on_event(_, e)
  if e.kind == "dock" then
    top_dock = e.side == "top"
    draw()
    return
  end
  if e.id ~= "c" then return end
  if not geo then
    if e.kind == "click" and e.button ~= "right" then
      if M.hooks.open_manager then M.hooks.open_manager() end
    elseif e.kind == "hover" or e.kind == "leave" then
      local now = e.kind == "hover"
      if now ~= idle_hover then
        idle_hover = now
        draw_idle()
      end
    end
    return
  end
  slotview.handle(e, {
    st = st, hit = hit, key_of = key_of, draw = draw, macro_of = macro_of,
    same = function(a, b) return a.side == b.side and a.button == b.button end,
    drop = function(c, id)
      if not main or not data.get_entry(main, id) then
        draw()
        return
      end
      data.pad_assign(main, c.side, c.button, id)
      slotview.flash(st, key_of(c), draw)
      changed()
    end,
    fire = function(c) M.fire(c.side, c.button) end,
    menu = function(c) open_menu(c.side, c.button) end,
    swap = function(from, to)
      if not main then return end
      data.pad_swap(main, from.side, from.button, to.side, to.button)
      changed()
    end,
  })
end

local function shown_now()
  return settings.get("on") and mode.is_pad() and settings.get("visible") and not bar.is_hidden()
end

local function signature()
  return table.concat({ tostring(jobs.current()), settings.get("pad_size"), tostring(shown_now()), view() }, ":")
end

-- Two sets to a row (LT + RT centred under them); each set: name, two diamonds, L1 / L3 left, R1 / R3 right.
local function layout(v, size)
  local key = v .. ":" .. size
  if layouts[key] then return layouts[key] end
  local gap = math.max(2, math.floor(size * 0.15))
  local arm = size + gap
  local dw = 2 * arm + size     -- a diamond's width
  local wing = size + 2 * gap   -- the shoulder column and its gap
  local side_w = 2 * wing + 2 * dw + 2 * gap
  local between = 6 * gap
  local spots = { up = { 1, 0 }, right = { 2, 1 }, down = { 1, 2 }, left = { 0, 1 } }
  local top = PAD + HEAD
  local side_h = HEAD + 2 * arm + size
  local sides = v == "all" and SIDES or { v }
  local cols, rows = math.min(#sides, 2), math.ceil(#sides / 2)
  local g = { size = size, slots = {}, side_x = {}, side_y = {}, side_w = side_w, sides = sides,
    width = 2 * PAD + cols * side_w + (cols - 1) * between, height = rows * (side_h + PAD) + PAD }
  local function add_slot(side, b, x, y)
    g.slots[#g.slots + 1] = { x = x, y = y, side = side, button = b }
  end
  for si, side in ipairs(sides) do
    local sx = PAD + ((si - 1) % 2) * (side_w + between)
    if si == #sides and si % 2 == 1 and cols == 2 then sx = PAD + (side_w + between) / 2 end
    local oy = math.floor((si - 1) / 2) * (side_h + PAD)
    g.side_x[side], g.side_y[side] = sx, PAD + oy
    for di = 1, 2 do
      local dx = sx + wing + (di - 1) * (dw + 2 * gap)
      for k = 1, 4 do
        local pos = spots[BUTTONS[k]]
        add_slot(side, BUTTONS[(di - 1) * 4 + k], dx + pos[1] * arm, oy + top + pos[2] * arm)
      end
    end
    local wy = oy + top + arm / 2
    add_slot(side, "lb", sx, wy)
    add_slot(side, "l3", sx, wy + arm)
    add_slot(side, "rb", sx + side_w - size, wy)
    add_slot(side, "r3", sx + side_w - size, wy + arm)
  end
  layouts[key] = g
  return g
end

-- Defines and opens the window when `key` changed. `width` adds the edge frame's margins (34).
local function define(key, w, h, width)
  if key == window_key and ui.is_open(WIN) then return end
  window_key = key
  built_at = os.clock()
  ui.window(WIN, {
    title = "Crossbars",
    heading = assigning and (main .. " - assigning") or main,
    edge = true,
    grow = "up",
    width = width,
    elements = { { type = "canvas", id = "c", w = w, h = h } },
  })
  ui.on(WIN, on_event)
  ui.open(WIN)
end

function M.rebuild()
  built_sig = signature()
  built_view = view()
  if not shown_now() then
    geo, window_key = nil, nil
    ui.close(WIN)
    return
  end
  main = jobs.current()
  if not main then
    built_sig, geo = nil, nil
    return
  end
  local size = settings.get("pad_size")
  local key = table.concat({ built_view, size, main, assigning and "assign" or "" }, ":")
  if built_view == "none" then
    geo = nil
    slotview.reset(st)
    define(key, IDLE_W, IDLE_H, IDLE_W + 2 * PAD + 34)
    draw_idle()
    return
  end
  idle_hover = false
  geo = layout(built_view, size)
  refresh_cells()
  define(key, geo.width, geo.height, geo.width + 34)
  M.recasts()
end

M.invalidate = M.rebuild

-- Every quarter second and on trigger changes.
function M.sync()
  if not mode.is_pad() then return end
  if view() ~= built_view then
    M.rebuild()
  elseif geo and held() ~= drawn_held then
    draw()
  end
end

-- Every second.
function M.tick()
  if signature() ~= built_sig then M.rebuild() end
end

return M
