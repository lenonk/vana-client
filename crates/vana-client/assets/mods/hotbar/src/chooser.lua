-- A popup list over a slot: a group's members (open), or any rows to pick from (pick_list). Up / Down and Enter, a
-- click, the bar's number keys, or the pad's d-pad and A pick; Escape or B closes.

local data = require("src.data")
local actions = require("src.actions")
local runner = require("src.runner")

local M = {}

-- Set by bar.lua and init.lua: picked() after a member ran.
M.hooks = {}

local WIN = "group"
local WIDTH = 240
M.WIDTH = WIDTH
local DIM = { 160, 152, 136, 255 }
local SUGGESTED = { 255, 214, 120, 255 }

local open_macro = nil -- the group shown
local open_bar = nil   -- the bar that opened it
local open_pick = nil  -- pick_list's callback(k)
local open_count = 0

local function clock(secs)
  secs = math.ceil(secs)
  return secs >= 60 and ("%d:%02d"):format(secs // 60, secs % 60) or tostring(secs)
end

local function rows_of(macro, suggested)
  local recasts = game.recasts()
  local rows = {}
  for k, m in ipairs(macro.group) do
    local a = actions.of(m)
    local left = a and actions.recast(a, recasts) or 0
    rows[k] = { data.name_of(m), left > 0 and clock(left) or "", tostring(k), icon = a and { kind = a.kind, id = a.id } or nil,
      colour = suggested and suggested(m) and SUGGESTED or (a and not actions.usable(a, game.player()) and DIM) or nil }
  end
  return rows
end

function M.is_open()
  return (open_macro ~= nil or open_pick ~= nil) and ui.is_open(WIN)
end

local function close()
  open_macro, open_bar, open_pick = nil, nil, nil
  if ui.is_open(WIN) then ui.close(WIN) end
end
M.close = close

function M.bar()
  return M.is_open() and open_bar or nil
end

-- Picks row k and closes.
function M.pick(k)
  if open_pick then
    local f = open_pick
    if k < 1 or k > open_count then return end
    close()
    f(k)
    return
  end
  local macro = open_macro
  local m = macro and macro.group[k]
  if not m then return end
  close()
  data.set_last_member(macro, k)
  runner.run_checked(m)
  if M.hooks.picked then M.hooks.picked() end
end

-- Moves the cursor by `delta` rows, wrapping.
function M.step(delta)
  if not M.is_open() then return end
  local n = open_pick and open_count or #open_macro.group
  local at = ui.get(WIN, "m").at or 1
  ui.update(WIN, "m", { at = (at - 1 + delta) % n + 1 })
end

function M.accept()
  if not M.is_open() then return end
  M.pick(ui.get(WIN, "m").at or 1)
end

-- `at`: {x, y, above} in window pixels, nil for the pointer; `suggested(member)` rows show gold.
function M.open(macro, at, suggested, bar)
  if not macro.group or #macro.group == 0 then return end
  local rows = rows_of(macro, suggested)
  ui.window(WIN, {
    title = data.trim(macro.name) ~= "" and macro.name or "Group",
    width = WIDTH,
    popup = true,
    animate = true,
    help = "Up / Down, then Enter or A. Escape or B closes.",
    elements = {
      { type = "list", id = "m", rows = rows, height = #rows, at = data.last_member(macro),
        columns = { { title = "" }, { title = "", width = 46, align = "right" },
          { title = "", width = 20, align = "right" } } },
    },
  })
  open_macro, open_bar, open_pick = macro, bar, nil
  ui.open(WIN, at)
end

-- rows: { name, "", "", icon? }; on_pick(k); the cursor starts on `start`.
function M.pick_list(title, rows, at, on_pick, start)
  if #rows == 0 then return end
  ui.window(WIN, {
    title = title,
    width = WIDTH,
    popup = true,
    animate = true,
    help = "Up / Down, then Enter or A. Escape or B closes.",
    elements = {
      { type = "list", id = "m", rows = rows, height = math.min(#rows, 10), at = start or 1,
        columns = { { title = "" }, { title = "", width = 46, align = "right" },
          { title = "", width = 20, align = "right" } } },
    },
  })
  open_macro, open_bar, open_pick, open_count = nil, nil, on_pick, #rows
  ui.open(WIN, at)
end

ui.on(WIN, function(_, e)
  if e.kind == "select" then
    M.pick(e.index)
  elseif e.kind == "close" then
    open_macro, open_bar, open_pick = nil, nil, nil
  end
end)

return M
