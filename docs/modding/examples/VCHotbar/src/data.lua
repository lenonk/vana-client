-- Macro storage and job-based resolution.
--
-- macros.json (in the mod's data folder) is nested by the job you play as MAIN:
--
--   {
--     "RDM": {
--       "main": { "<bar>": { "<slot>": <macro> } },        -- loads whenever RDM is main
--       "sub":  { "WHM": { "<bar>": { "<slot>": <macro> } } } -- loads when RDM/WHM, overrides "main"
--     }
--   }
--
-- A macro is { name = "Cure", label = "Cure" (optional), lines = { "/ma \"Cure\" <t>", ... } }.
-- In a "sub" set a slot of `false` blanks a slot the main set filled.
--
-- Bars and slots are stored as text keys ("1", "2", ...) because JSON object keys are text.

local M = {}

M.MAX_BARS = 3
M.MAX_SLOTS = 12

local FILE = "macros.json"

local deps = nil
local db = {}

-- ---------------------------------------------------------------------------
-- Text helpers
-- ---------------------------------------------------------------------------

local function trim(s)
  s = tostring(s or "")
  s = s:gsub("^%s+", "")
  s = s:gsub("%s+$", "")
  return s
end
M.trim = trim

-- "a | b | c" -> { "a", "b", "c" } (empty pieces dropped).
function M.split_lines(text)
  local out = {}
  for piece in tostring(text or ""):gmatch("[^|]+") do
    piece = trim(piece)
    if piece ~= "" then
      out[#out + 1] = piece
    end
  end
  return out
end

function M.join_lines(lines)
  return table.concat(lines or {}, " | ")
end

-- Fit text on a slot: up to 7 characters on one line, otherwise two short lines.
local function wrap(text)
  text = trim(text)
  if #text <= 7 then
    return text
  end
  local a, b = text:match("^(%S+)%s+(.+)$")
  if a then
    return a:sub(1, 7) .. "\n" .. b:sub(1, 7)
  end
  return text:sub(1, 7)
end

function M.display_label(macro)
  if type(macro) ~= "table" then
    return ""
  end
  local label = trim(macro.label)
  if label == "" then
    label = tostring(macro.name or "")
  end
  return wrap(label)
end

-- ---------------------------------------------------------------------------
-- Load and save
-- ---------------------------------------------------------------------------

-- deps = { read, write, decode, encode, log? }
function M.init(d)
  deps = d
  return M.load()
end

function M.load()
  db = {}
  local raw = deps.read(FILE)
  if not raw or raw:match("^%s*$") then
    return true
  end
  local ok, parsed = pcall(deps.decode, raw)
  if not ok or type(parsed) ~= "table" then
    -- Keep what the player wrote so a typo in a hand edit is never lost.
    pcall(deps.write, FILE .. ".bad", raw)
    return false, tostring(parsed)
  end
  db = parsed
  return true
end

function M.save()
  local ok, err = pcall(function()
    deps.write(FILE, deps.encode(db))
  end)
  if not ok and deps.log then
    deps.log("could not save " .. FILE .. ": " .. tostring(err))
  end
  return ok, err
end

-- ---------------------------------------------------------------------------
-- Sets
-- ---------------------------------------------------------------------------

local function key(n)
  return tostring(n)
end

local function is_table(v)
  return type(v) == "table"
end

-- The set for `main` (sub == nil) or for `main` with `sub`. Created when `create`.
local function find_set(main, sub, create)
  if not main then
    return nil
  end
  local job = db[main]
  if not is_table(job) then
    if not create then
      return nil
    end
    job = {}
    db[main] = job
  end
  if not sub then
    if not is_table(job.main) then
      if not create then
        return nil
      end
      job.main = {}
    end
    return job.main
  end
  if not is_table(job.sub) then
    if not create then
      return nil
    end
    job.sub = {}
  end
  if not is_table(job.sub[sub]) then
    if not create then
      return nil
    end
    job.sub[sub] = {}
  end
  return job.sub[sub]
end

local function raw_slot(set, bar, slot)
  if not is_table(set) then
    return nil
  end
  local b = set[key(bar)]
  if not is_table(b) then
    return nil
  end
  return b[key(slot)]
end

-- The macro in exactly one set: a macro table, false (blanked) or nil.
function M.get_raw(main, sub, bar, slot)
  return raw_slot(find_set(main, sub, false), bar, slot)
end

-- What a slot does for this main/sub pair: the macro and where it came from
-- ("sub" overrides "main"), or nil when the slot is empty.
function M.resolve(main, sub, bar, slot)
  if sub then
    local v = raw_slot(find_set(main, sub, false), bar, slot)
    if v == false then
      return nil
    end
    if is_table(v) then
      return v, "sub"
    end
  end
  local v = raw_slot(find_set(main, nil, false), bar, slot)
  if is_table(v) then
    return v, "main"
  end
  return nil
end

-- Every slot for this pair: main first, then sub overwriting it.
-- Returns eff[bar][slot] = { macro = ..., from = "main" | "sub" }.
function M.effective(main, sub)
  local eff = {}
  local function overlay(set, from)
    if not is_table(set) then
      return
    end
    for bk, bar in pairs(set) do
      local b = tonumber(bk)
      if b and is_table(bar) then
        for sk, m in pairs(bar) do
          local s = tonumber(sk)
          if s then
            eff[b] = eff[b] or {}
            if m == false then
              eff[b][s] = nil
            elseif is_table(m) then
              eff[b][s] = { macro = m, from = from }
            end
          end
        end
      end
    end
  end
  overlay(find_set(main, nil, false), "main")
  if sub then
    overlay(find_set(main, sub, false), "sub")
  end
  return eff
end

-- ---------------------------------------------------------------------------
-- Editing
-- ---------------------------------------------------------------------------

function M.set_macro(main, sub, bar, slot, macro)
  local set = find_set(main, sub, true)
  local bk = key(bar)
  if not is_table(set[bk]) then
    set[bk] = {}
  end
  local lines = {}
  for _, l in ipairs(macro.lines or {}) do
    lines[#lines + 1] = tostring(l)
  end
  local m = { name = trim(macro.name), lines = lines }
  local label = trim(macro.label)
  if label ~= "" then
    m.label = label
  end
  set[bk][key(slot)] = m
  M.save()
end

-- Empty a slot of one set. In a sub set, a slot the main set fills is blanked with
-- `false` so the main macro does not show through; otherwise the entry is removed.
function M.clear(main, sub, bar, slot)
  local set = find_set(main, sub, true)
  local bk, sk = key(bar), key(slot)
  if sub and is_table(raw_slot(find_set(main, nil, false), bar, slot)) then
    if not is_table(set[bk]) then
      set[bk] = {}
    end
    set[bk][sk] = false
  elseif is_table(set[bk]) then
    set[bk][sk] = nil
    if next(set[bk]) == nil then
      set[bk] = nil
    end
  end
  M.save()
end

return M
