-- Hotkeys for the bars: a modifier per bar plus one key row shared by all bars.
-- Defaults: bar 1 = 1..0, bar 2 = Ctrl+1..0, bar 3 = Alt+1..0.
-- The choices live in storage.account.vch_keys (string keys only: saved tables
-- come back with text keys).

local M = {}

-- label -> key prefix, in the order the editor offers them.
M.MOD_LABELS = { "None", "Ctrl", "Alt", "Shift", "Ctrl+Alt", "Ctrl+Shift", "Alt+Shift" }
local MOD_PREFIX = {
  ["None"] = "",
  ["Ctrl"] = "ctrl+",
  ["Alt"] = "alt+",
  ["Shift"] = "shift+",
  ["Ctrl+Alt"] = "ctrl+alt+",
  ["Ctrl+Shift"] = "ctrl+shift+",
  ["Alt+Shift"] = "alt+shift+",
}
local MOD_SHORT = {
  ["None"] = "",
  ["Ctrl"] = "C",
  ["Alt"] = "A",
  ["Shift"] = "S",
  ["Ctrl+Alt"] = "CA",
  ["Ctrl+Shift"] = "CS",
  ["Alt+Shift"] = "AS",
}
local DEFAULT_MODS = { "None", "Ctrl", "Alt" }

M.ROW_LABELS = { "Number row (1-0, -, =)", "Function keys (F1-F12)", "Numpad (1-0, -, +)" }
local ROWS = {
  {
    keys = { "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=" },
    hints = { "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=" },
  },
  {
    keys = { "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12" },
    hints = { "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12" },
  },
  {
    keys = { "numpad1", "numpad2", "numpad3", "numpad4", "numpad5", "numpad6", "numpad7", "numpad8", "numpad9",
      "numpad0", "numpad-", "numpad+" },
    hints = { "1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "+" },
  },
}

local bound = {}
local conflicts = {}

local function store()
  local s = storage.account
  if type(s.vch_keys) ~= "table" then
    s.vch_keys = {}
  end
  return s.vch_keys
end

local function index_of(list, value)
  for i, v in ipairs(list) do
    if v == value then
      return i
    end
  end
  return nil
end
M.index_of = index_of

function M.get_mod(bar)
  local v = store()["bar" .. bar]
  if v and MOD_PREFIX[v] then
    return v
  end
  return DEFAULT_MODS[bar] or "None"
end

function M.set_mod(bar, label)
  if MOD_PREFIX[label] then
    store()["bar" .. bar] = label
  end
end

function M.get_row()
  local v = store().row
  if v and index_of(M.ROW_LABELS, v) then
    return v
  end
  return M.ROW_LABELS[1]
end

function M.set_row(label)
  if index_of(M.ROW_LABELS, label) then
    store().row = label
  end
end

function M.reset()
  storage.account.vch_keys = nil
end

local function row()
  return ROWS[index_of(M.ROW_LABELS, M.get_row())]
end

-- The key combination for a slot, e.g. "ctrl+1".
function M.combo(bar, slot)
  return MOD_PREFIX[M.get_mod(bar)] .. row().keys[slot]
end

-- Short text for the slot's corner, e.g. "C1" or "AF3".
function M.hint(bar, slot)
  return MOD_SHORT[M.get_mod(bar)] .. row().hints[slot]
end

-- Drops the old binds and binds every slot to fire(bar, slot). A combination two
-- slots share is bound for the first only; the clash is listed by M.conflicts().
function M.bind_all(bars, slots, fire)
  for _, combo in ipairs(bound) do
    pcall(binds.unset, combo)
  end
  bound = {}
  conflicts = {}
  local seen = {}
  for b = 1, bars do
    for s = 1, slots do
      local combo = M.combo(b, s)
      if seen[combo] then
        conflicts[#conflicts + 1] = ("%s: bar %d slot %d is taken by bar %d slot %d"):format(
          combo, b, s, seen[combo][1], seen[combo][2])
      else
        seen[combo] = { b, s }
        local ok, err = pcall(binds.set, combo, function()
          fire(b, s)
        end)
        if ok then
          bound[#bound + 1] = combo
        else
          log.warn(("could not bind %s: %s"):format(combo, tostring(err)))
        end
      end
    end
  end
end

function M.conflicts()
  return conflicts
end

return M
