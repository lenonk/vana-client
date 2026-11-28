-- XIV Buff Bar: draws the player's own status effects (buffs and debuffs alike,
-- the game's 0x037 list) with native status icons, countdowns, a blink in the
-- last 3 s, hover help, drag to move and a click to cancel.
--
-- Change-driven: elements are rebuilt only when the set of statuses changes;
-- between changes a 0.5 s tick refreshes the countdown labels, the tips and the
-- blink.

local M = {}

-- XIV-UI passes the buff settings proxy at entry; global settings is the suite after bootstrap.
local bound_settings

function M.bind_settings(s) bound_settings = s end

local function cfg()
  return bound_settings or settings
end

local MAX = 36              -- the game draws up to 36 (4 rows of 9)
local slots = {}            -- i -> {id, name, desc, art, square, cancellable, has_img, has_ico, has_txt}
local cur_ids = ""          -- signature of the statuses currently drawn
local lock = false          -- dragging locked (//xbb lock)
local blink = false
local drag = nil
local bg_on, bg_w, bg_h = false, 0, 0   -- the full-bar drag area
local untimed_cache = {}
local failed = false       -- the bar stopped on an error; native icons are back; it retries
local fail_ticks = 0
local logged = false       -- log the first error once
local created = {}         -- element id -> true once hud.image/icon/text created it
local bound = {}           -- element id -> true once hud.on bound its handler
local applied_x, applied_y -- last place() in UI units (reposition when offsets change)

-- Create an element on first use and update it with hud.set afterwards, the way the
-- other XIV mods do. (hud.on replaces a same-id handler, so binding once is not needed
-- for correctness; it just avoids rebinding on every rebuild.)
local function put(kind, id, spec)
  if created[id] then
    local ok, err = pcall(hud.set, id, spec)
    if not ok then
      created[id] = nil
      if hud.remove then pcall(hud.remove, id) end
      ok, err = pcall(hud[kind], id, spec)
      if not ok then error(err) end
      created[id] = true
    end
  else
    local ok, err = pcall(hud[kind], id, spec)
    if not ok then error(err) end
    created[id] = true
  end
end
local function put_text(id, spec)
  put("text", id, spec)
end
local function bind(id, fn)
  if not bound[id] and pcall(hud.on, id, fn) then bound[id] = true end
end
-- One element failing (a bad PNG, say) must not take the whole bar down: run it
-- protected and log the first error, never swallow it silently.
local function try(fn, ...)
  local ok, err = pcall(fn, ...)
  if not ok and not logged then
    logged = true
    if log and log.error then log.error("XIV Buff Bar: " .. tostring(err)) end
  end
  return ok
end

local function on() return cfg().get("on") == true end
local function size() return cfg().get("size") or 32 end
local function cols() return math.max(1, math.floor(cfg().get("columns") or 9)) end
local function gap() return cfg().get("gap") or 4 end
local function show_timers() return cfg().get("timers") ~= false end
local function blink_on() return cfg().get("blink") ~= false end
local function cancel_on() return cfg().get("cancel_click") ~= false end

-- ui_scale; native icon size and step (square slots), y step includes room for the label.
local function geom()
  local u = hud.screen().ui_scale
  local h = size()
  local sw = h
  local g = gap()
  local k = h / 32
  return u, h, sw, sw + g, h + g + math.floor(9 * k + 0.5), math.max(0.5, 0.8 * k)
end

local DEFAULT_X, DEFAULT_Y = 8, 80

-- Bar position: drag saves absolute UI units in account storage (no mod setting sliders).
local function read_saved_pos()
  local d = type(storage) == "table" and storage.account and storage.account.xbb
  if type(d) ~= "table" then return DEFAULT_X, DEFAULT_Y end
  if d.x ~= nil or d.y ~= nil then
    return tonumber(d.x) or DEFAULT_X, tonumber(d.y) or DEFAULT_Y
  end
  -- Legacy drag delta (before offsets were removed from settings).
  return DEFAULT_X + (tonumber(d.dx) or 0), DEFAULT_Y + (tonumber(d.dy) or 0)
end

local function save_pos(x, y)
  if type(storage) ~= "table" or type(storage.account) ~= "table" then return end
  storage.account.xbb = { x = math.floor(x + 0.5), y = math.floor(y + 0.5) }
  if storage.save then pcall(storage.save) end
end

-- Keep the bar on screen; full width/height use current layout (slots may be empty during clamp).
local function clamp_pos(x, y)
  local ok, scr = pcall(hud.screen)
  if not ok or type(scr) ~= "table" then return x, y end
  local u = tonumber(scr.ui_scale) or 1
  local sw = (tonumber(scr.width) or 0) / u
  local sh = (tonumber(scr.height) or 0) / u
  if sw < 64 or sh < 64 then return x, y end
  local _, _, _, stepx, stepy = geom()
  local cl = cols()
  local used = 0
  for i = 1, MAX do
    if slots[i] and slots[i].id then used = i end
  end
  if used == 0 then used = 1 end
  local bw = math.min(used, cl) * stepx
  local bh = math.ceil(used / cl) * stepy
  local margin = 8
  x = math.max(margin - bw, math.min(x, sw - margin))
  y = math.max(margin, math.min(y, sh - math.max(margin, bh)))
  return x, y
end

local function stored_pos()
  return clamp_pos(read_saved_pos())
end
-- Where the bar is right now: the in-progress drag position while dragging (so a status
-- change mid-drag does not snap it back), otherwise the saved place.
local function cur_pos()
  if drag and drag.cx then return drag.cx, drag.cy end
  return stored_pos()
end

-- The countdown: hours, minutes, seconds; shown down to 1 s.
local function time_label(rem)
  if type(rem) ~= "number" then return nil end
  local r = math.floor(rem + 0.5)
  if r <= 0 then return nil end
  if r >= 3600 then return math.floor(r / 3600) .. "h" end
  if r >= 60 then return math.floor(r / 60) .. "m" end
  return tostring(r)
end

local function is_untimed(id)
  if untimed_cache[id] == nil then
    local ok, info = pcall(res.status_info, id)
    untimed_cache[id] = (ok and info and info.untimed) and true or false
  end
  return untimed_cache[id]
end

local function buff_list()
  local out = {}
  local ok, list = pcall(game.buffs)
  if ok and type(list) == "table" then
    for _, r in ipairs(list) do
      if type(r) == "table" and type(r.id) == "number" and r.id ~= 0 then out[#out + 1] = r end
    end
  end
  return out
end

local function signature(list)
  local t = {}
  for i = 1, math.min(#list, MAX) do t[i] = tostring(list[i].id) end
  return table.concat(t, ",")
end

local function text_spec(label, bx, by, u, h, sw, tsize)
  return { x = bx + sw * u / 2, y = by + h * u, text = label, size = tsize, align = "center",
    font = "game", colour = { 216, 214, 208, 255 }, outline = { 0, 0, 0, 220 }, layer = 32, visible = true }
end

local function slot_xy(i, x, y, cols, stepx, stepy, u)
  local cx = (i - 1) % cols
  local cy = math.floor((i - 1) / cols)
  return (x + cx * stepx) * u, (y + cy * stepy) * u
end

local function tip_of(s, rem)
  local timed = type(rem) == "number" and not is_untimed(s.id)
  return { title = s.name or ("Status " .. tostring(s.id)),
    text = (s.desc or "") .. (timed and ("\n" .. math.max(0, math.floor(rem + 0.5)) .. "s left") or "") }
end

-- Reposition existing elements (drag).
function M.place(x, y)
  local u, h, sw, stepx, stepy = geom()
  local cl = cols()
  for i = 1, MAX do
    local s = slots[i]
    if s then
      local bx, by = slot_xy(i, x, y, cl, stepx, stepy, u)
      if s.has_img then
        if s.square then
          local pad = math.max(1, math.floor(h * 0.06))
          local side = sw + pad * 2
          pcall(hud.set, "bbp" .. i, { x = bx - pad * u, y = by + (h - side) / 2 * u, visible = true })
        else
          pcall(hud.set, "bbp" .. i, { x = bx, y = by, visible = true })
        end
      end
      if s.has_ico then pcall(hud.set, "bbi" .. i, { x = bx, y = by + (h - sw) / 2 * u, visible = true }) end
      if s.has_hit then pcall(hud.set, "bbh" .. i, { x = bx, y = by, visible = true }) end
      if s.has_txt then pcall(hud.set, "bbt" .. i, { x = bx + sw * u / 2, y = by + h * u, visible = true }) end
    end
  end
  if bg_on then pcall(hud.set, "bbbg", { x = x * u - 3, y = y * u - 3, visible = true }) end
end

-- Drag core shared by the slots and the full-bar area.
local function drag_core(e)
  if e.kind == "drag_start" then
    if lock then return true end
    local x, y = stored_pos()
    -- dx / dy are from the start of the drag, so keep the start and offset from it.
    drag = { x = x, y = y }
    return true
  elseif e.kind == "drag" or e.kind == "drag_end" then
    if not drag then return false end
    local u = hud.screen().ui_scale
    local nx = drag.x + (e.dx or 0) / u
    local ny = drag.y + (e.dy or 0) / u
    drag.cx, drag.cy = nx, ny
    M.place(nx, ny)
    if e.kind == "drag_end" then
      save_pos(clamp_pos(nx, ny))
      drag = nil
    end
    return true
  end
  return false
end

local function handler(i)
  return function(_, e)
    if drag_core(e) then return true end
    if e.kind == "click" then
      -- A click ends the press: clear any drag state so a lost drag_end can
      -- never wedge cancelling. Only a left click cancels.
      drag = nil
      local s = slots[i]
      if e.button ~= "right" and cancel_on() and s and s.cancellable and s.id then
        local ok, info = pcall(res.status_info, s.id)
        if ok and info and info.cancellable then pcall(actions.cancel, s.id) end
      end
      return true
    end
    return false
  end
end

-- The full-bar area: dragging anywhere on the bar moves it (no click action).
local function bg_handler(_, e) return drag_core(e) end

local function build(list, x, y)
  x, y = clamp_pos(x, y)
  applied_x, applied_y = x, y
  local u, h, sw, stepx, stepy, tsize = geom()
  local cl = cols()
  -- A full-bar drag area, so a press between icons or on the label strip still moves the bar.
  local used = math.min(#list, MAX)
  if used > 0 and try(put, "rect", "bbbg", { x = x * u - 3, y = y * u - 3,
      w = math.min(used, cl) * stepx * u + 6, h = math.ceil(used / cl) * stepy * u + 6,
      colour = { 0, 0, 0, 1 }, layer = 28, visible = true }) then
    bg_on, bg_w, bg_h = true, math.min(used, cl) * stepx, math.ceil(used / cl) * stepy
    bind("bbbg", bg_handler)
  else
    bg_on = false
    if created["bbbg"] then pcall(hud.set, "bbbg", { visible = false }) end
  end
  for i = 1, MAX do
    local s = slots[i]
    if not s then s = {}; slots[i] = s end
    local img, ico, txt = "bbp" .. i, "bbi" .. i, "bbt" .. i
    local row = list[i]
    if not row then
      if s.has_img then pcall(hud.set, img, { visible = false }) end
      if s.has_ico then pcall(hud.set, ico, { visible = false }) end
      if s.has_hit then pcall(hud.set, "bbh" .. i, { visible = false }) end
      if s.has_txt then pcall(hud.set, txt, { visible = false }) end
      s.id, s.name, s.desc, s.cancellable, s.art, s.tip_text = nil, nil, nil, nil, false, nil
    else
      local bx, by = slot_xy(i, x, y, cl, stepx, stepy, u)
      local id = row.id
      local ok_name, status_name = pcall(res.status, id)
      local name = (ok_name and status_name) or ("Status " .. tostring(id))
      local ok, info = pcall(res.status_info, id)
      info = ok and info or nil
      local timed = type(row.remaining) == "number" and not (info and info.untimed)
      local label = timed and show_timers() and time_label(row.remaining) or nil
      local tip = { title = name, text = (info and info.description or "") ..
        (timed and ("\n" .. math.max(0, math.floor((row.remaining or 0) + 0.5)) .. "s left") or "") }
      -- A transparent hit rect takes the press: the art images do not take one in this client
      -- (the other XIV mods also drag from a rect grip). Dragging it moves the bar; a left click
      -- inside the same press cancels a cancellable effect.
      if try(put, "rect", "bbh" .. i, { x = bx, y = by, w = sw * u, h = h * u,
          colour = { 0, 0, 0, 1 }, layer = 29, visible = true, tip = tip }) then
        s.has_hit = true
        bind("bbh" .. i, handler(i))
      end
      if s.has_img then pcall(hud.set, img, { visible = false }); s.has_img = false end
      if try(put, "icon", ico, { x = bx, y = by, size = sw * u, kind = "status",
          id = id, layer = 30, visible = true, tip = tip }) then
        s.has_ico = true
      end
      if label then
        if try(put_text, txt, text_spec(label, bx, by, u, h, sw, tsize)) then s.has_txt = true end
      elseif s.has_txt then
        pcall(hud.set, txt, { visible = false })
      end
      s.id, s.name, s.desc, s.art, s.tip_text = id, name, (info and info.description or ""), true, tip.text
      s.cancellable = (info and info.cancellable) and true or false
    end
  end
end

-- Only the countdown labels and the blink change between status changes.
local function refresh_labels(list)
  blink = not blink
  local x, y = cur_pos()
  local u, h, sw, stepx, stepy, tsize = geom()
  local cl = cols()
  for i = 1, math.min(#list, MAX) do
    local s = slots[i]
    if s and s.art and s.id then
      local rem = list[i].remaining
      local timed = type(rem) == "number" and not is_untimed(s.id)
      local label = timed and show_timers() and time_label(rem) or nil
      if label and not s.has_txt then
        -- The effect had no duration when the bar was built (not reported yet); give it a label now.
        local bx, by = slot_xy(i, x, y, cl, stepx, stepy, u)
        if try(put_text, "bbt" .. i, text_spec(label, bx, by, u, h, sw, tsize)) then s.has_txt = true end
      elseif s.has_txt then
        pcall(hud.set, "bbt" .. i, { text = label or "", visible = label ~= nil })
      end
      -- Blink in the last 3 s. The countdown stays visible down to 1 s.
      local a = 255
      if blink_on() and timed and rem and rem <= 3 then a = blink and 235 or 110 end
      -- Refresh the tip ("N s left") only when its text changed.
      local tip = tip_of(s, rem)
      local tip_changed = tip.text ~= s.tip_text
      if tip_changed then s.tip_text = tip.text end
      if s.has_img then
        pcall(hud.set, "bbp" .. i, tip_changed and { colour = { 255, 255, 255, a }, tip = tip } or { colour = { 255, 255, 255, a } })
      elseif s.has_ico and tip_changed then
        pcall(hud.set, "bbi" .. i, { tip = tip })
      end
      if tip_changed and s.has_hit then pcall(hud.set, "bbh" .. i, { tip = tip }) end
    end
  end
end

local function clear_all()
  -- Never hud.clear() in XIV-UI: it removes every panel's elements in this mod VM
  -- (player bars, target, focus, …) while other modules still hold stale ids.
  for id in pairs(created) do pcall(hud.set, id, { visible = false }) end
  created, bound = {}, {}
  slots, cur_ids, drag = {}, "", nil
  bg_on, bg_w, bg_h = false, 0, 0
end

function M.tick()
  if failed then
    -- Stopped on an error: native icons are back. Retry about every 5 s (10 ticks).
    fail_ticks = fail_ticks + 1
    if fail_ticks % 10 ~= 0 or not on() then return end
    clear_all()
  end
  local ok, err = pcall(function()
    if not on() then
      if cur_ids ~= "" or next(slots) ~= nil then clear_all() end
      return
    end
    local list = buff_list()
    local s = signature(list)
    if s ~= cur_ids then
      cur_ids = s
      if next(slots) == nil and s == "" then return end
      build(list, cur_pos())
    else
      local x, y = cur_pos()
      if x ~= applied_x or y ~= applied_y then
        applied_x, applied_y = x, y
        if next(slots) ~= nil then M.place(x, y) end
      end
      refresh_labels(list)
    end
  end)
  if ok then
    if failed then
      -- The retry worked: take the bar back and hide the native icons again.
      failed, fail_ticks, logged = false, 0, false
      pcall(ui.native.hide, "buffs", true)
    end
  else
    if not logged then
      logged = true
      if log and log.error then log.error("XIV Buff Bar: " .. tostring(err)) end
    end
    if not failed then
      -- Stop drawing and give the game's own icons back: with the bar dead and the native
      -- ones still hidden the player would have no status display at all.
      failed, fail_ticks = true, 0
      pcall(clear_all)
      pcall(ui.native.hide, "buffs", false)
      local detail = tostring(err or "")
      local msg = "XIV Buff Bar hit an error (see log); native icons restored, retrying."
      if detail ~= "" then msg = msg .. " [" .. detail .. "]" end
      if chat and chat.add then pcall(chat.add, msg) end
    end
  end
end

function M.rebuild() cur_ids = "-"; M.tick() end
function M.reset() failed, fail_ticks, logged = false, 0, false; clear_all(); M.tick() end
function M.resetpos()
  failed, fail_ticks, logged = false, 0, false
  save_pos(DEFAULT_X, DEFAULT_Y)
  cur_ids = "-"
  clear_all()
  M.tick()
end
function M.failed() return failed end
function M.clear() clear_all() end
function M.count() return #buff_list() end
function M.toggle_lock() lock = not lock; return lock end
function M.locked() return lock end

return M
