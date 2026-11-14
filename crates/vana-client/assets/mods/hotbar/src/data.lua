-- Macro storage and resolution by job.
--
-- File: macros-<World>.json in the mod's data folder (macros.json until the world is known; a new world's file
-- starts from it). Keyed by main job:
--
--   {
--     "RDM": {
--       "library": { "<id>": <entry> },
--       "main": { "<bar>": { "<slot>": <macro> } },
--       "sub":  { "WHM": { "<bar>": { "<slot>": <macro> } } },   -- overrides "main" as RDM/WHM
--       "pad":  { "lt": { "up": <macro>, ... }, "rt": {...}, "lr": {...} }
--     }
--   }
--
-- Slot value: a macro { name, label?, lines }, a library reference { ref = "<id>" }, or false in a sub set (blanks
-- the main set's slot). Library entry: { name, label?, action = { kind, name, target, line? }?, lines } or a group
-- { name, label?, group = { <entry>, ... } }. Bar and slot keys are strings (JSON object keys).

local M = {}

M.MAX_BARS = 3
M.MAX_SLOTS = 12
M.PAD_SIDES = { "lt", "rt", "lr" }
M.PAD_BUTTONS = { "up", "right", "down", "left", "y", "b", "a", "x", "lb", "rb", "l3", "r3" }

local LEGACY = "macros.json"
local FILE = LEGACY

local deps = nil
local db = {}

local function is_table(v) return type(v) == "table" end
local function key(n) return tostring(n) end

-- ---------------------------------------------------------------------------
-- Text
-- ---------------------------------------------------------------------------

local function trim(s)
  return (tostring(s or ""):gsub("^%s+", ""):gsub("%s+$", ""))
end
M.trim = trim

-- "a | b | c" -> { "a", "b", "c" }, empty pieces dropped.
function M.split_lines(text)
  local out = {}
  for piece in tostring(text or ""):gmatch("[^|]+") do
    piece = trim(piece)
    if piece ~= "" then out[#out + 1] = piece end
  end
  return out
end

function M.join_lines(lines)
  return table.concat(lines or {}, " | ")
end

-- A slot's text: up to 7 characters, or two lines of 7.
local function wrap(text)
  text = trim(text)
  if #text <= 7 then return text end
  local a, b = text:match("^(%S+)%s+(.+)$")
  if a then return a:sub(1, 7) .. "\n" .. b:sub(1, 7) end
  return text:sub(1, 7)
end

function M.display_label(macro)
  if not is_table(macro) then return "" end
  local label = trim(macro.label)
  return wrap(label ~= "" and label or tostring(macro.name or ""))
end

-- A macro's name, else its first line.
function M.name_of(macro)
  return trim(macro.name) ~= "" and macro.name or ((macro.lines or {})[1] or "")
end

-- A macro's lines, or a group's members numbered, one per line.
function M.tip_text(macro)
  local lines = macro.lines or {}
  if macro.group then
    lines = {}
    for n, m in ipairs(macro.group) do lines[n] = ("%d. %s"):format(n, m.lines[1] or m.name) end
  end
  return table.concat(lines, "\n")
end

-- ---------------------------------------------------------------------------
-- Load and save
-- ---------------------------------------------------------------------------

-- d = { read, write, decode, encode, log? }
function M.init(d)
  deps = d
  return M.load()
end

function M.load()
  db = {}
  local raw = deps.read(FILE)
  if not raw and FILE ~= LEGACY then raw = deps.read(LEGACY) end
  if not raw or raw:match("^%s*$") then return true end
  local ok, parsed = pcall(deps.decode, raw)
  if not ok or not is_table(parsed) then
    pcall(deps.write, FILE .. ".bad", raw)
    return false, tostring(parsed)
  end
  db = parsed
  return true
end

function M.file() return FILE end

-- Switches to world `name`'s file (nil or "": macros.json). Returns false when unchanged, else true and load()'s.
function M.set_world(name)
  local file = name and name ~= "" and ("macros-" .. (name:gsub("[^%w%-_]", "_")) .. ".json") or LEGACY
  if file == FILE then return false end
  FILE = file
  return true, M.load()
end

function M.save()
  local ok, err = pcall(function() deps.write(FILE, deps.encode(db)) end)
  if not ok and deps.log then deps.log("could not save " .. FILE .. ": " .. tostring(err)) end
  return ok, err
end

-- ---------------------------------------------------------------------------
-- Tables
-- ---------------------------------------------------------------------------

local function child(t, k, create)
  if not is_table(t) then return nil end
  if is_table(t[k]) then return t[k] end
  if not create then return nil end
  t[k] = {}
  return t[k]
end

-- db[main][part]: "library", "main", "sub" or "pad".
local function job_part(main, part, create)
  if not main then return nil end
  return child(child(db, main, create), part, create)
end

-- The main set (sub nil) or a sub job's set.
local function find_set(main, sub, create)
  if not sub then return job_part(main, "main", create) end
  return child(job_part(main, "sub", create), sub, create)
end

local function raw_slot(set, row, slot)
  local r = child(set, key(row), false)
  return r and r[key(slot)]
end

local function put_slot(set, row, slot, v)
  child(set, key(row), true)[key(slot)] = v
end

-- Removes a slot, and its row when that empties.
local function drop_slot(set, row, slot)
  local r = child(set, key(row), false)
  if not r then return end
  r[key(slot)] = nil
  if next(r) == nil then set[key(row)] = nil end
end

-- ---------------------------------------------------------------------------
-- Library
-- ---------------------------------------------------------------------------

local COMMAND = { ma = "/ma", ja = "/ja", ws = "/ws", pet = "/pet", item = "/item", equip = "/equip", cmd = "" }

-- The line an action builds: /ma "Cure" <t>, /equip ring1 "Warp Ring", /attack <t>; its `line` when set.
function M.command(action)
  if not is_table(action) or not COMMAND[action.kind] or trim(action.name) == "" then return nil end
  if trim(action.line) ~= "" then return trim(action.line) end
  local target = trim(action.target)
  if action.kind == "equip" then
    if target == "" then return nil end
    return ('/equip %s "%s"'):format(target, trim(action.name))
  end
  if action.kind == "cmd" then
    return target ~= "" and ("%s <%s>"):format(trim(action.name), target) or trim(action.name)
  end
  local line = ('%s "%s"'):format(COMMAND[action.kind], trim(action.name))
  if target ~= "" then line = line .. " <" .. target .. ">" end
  return line
end

-- An entry's lines as run: its action's line, then its own (none for a group).
function M.entry_lines(entry)
  local out = {}
  if is_table(entry.group) then return out end
  out[1] = M.command(entry.action)
  for _, l in ipairs(entry.lines or {}) do out[#out + 1] = tostring(l) end
  return out
end

local function member(e)
  return { name = e.name or "", label = e.label, lines = M.entry_lines(e), action = e.action }
end

-- One runnable macro per entry table (the runner keys on it); a saved entry is a new table.
local expanded = setmetatable({}, { __mode = "k" })

local function expand(entry, id)
  local m = expanded[entry]
  if m then return m end
  m = member(entry)
  m.ref = id
  if is_table(entry.group) then
    m.group = {}
    for k, e in ipairs(entry.group) do
      if is_table(e) then m.group[k] = member(e) end
    end
  end
  expanded[entry] = m
  return m
end

function M.get_entry(main, id)
  local lib = job_part(main, "library", false)
  local entry = lib and id ~= nil and lib[tostring(id)]
  return is_table(entry) and entry or nil
end

-- A library entry as slots run it, or nil.
function M.macro_of(main, id)
  local entry = M.get_entry(main, id)
  return entry and expand(entry, tostring(id)) or nil
end

-- A slot value as a macro: a reference's entry (nil when gone), else itself.
local function deref(main, v)
  if not is_table(v) then return nil end
  if v.ref ~= nil then return M.macro_of(main, v.ref) end
  return v
end

local function entry_id_order(a, b)
  local na, nb = tonumber(a), tonumber(b)
  if na and nb then return na < nb end
  return tostring(a) < tostring(b)
end

-- The job's entries, oldest first: { { id, entry }, ... }.
function M.library(main)
  local lib = job_part(main, "library", false) or {}
  local ids = {}
  for id, entry in pairs(lib) do
    if is_table(entry) then ids[#ids + 1] = id end
  end
  table.sort(ids, entry_id_order)
  local out = {}
  for n, id in ipairs(ids) do out[n] = { id = id, entry = lib[id] } end
  return out
end

local function clean_entry(e)
  local out = { name = trim(e.name) }
  if trim(e.label) ~= "" then out.label = trim(e.label) end
  if is_table(e.group) then
    out.group = {}
    for _, m in ipairs(e.group) do
      if is_table(m) and not is_table(m.group) then
        local c = clean_entry(m)
        if c.action or #c.lines > 0 then out.group[#out.group + 1] = c end
      end
    end
    return out
  end
  out.lines = {}
  for _, l in ipairs(e.lines or {}) do
    l = trim(l)
    if l ~= "" then out.lines[#out.lines + 1] = l end
  end
  local a = e.action
  if is_table(a) and COMMAND[a.kind] and trim(a.name) ~= "" then
    out.action = { kind = a.kind, name = trim(a.name), target = trim(a.target) }
    if trim(a.line) ~= "" then out.action.line = trim(a.line) end
  end
  return out
end

-- Saves an entry under `id` (nil: a new id). Returns the id.
function M.save_entry(main, id, entry)
  local lib = job_part(main, "library", true)
  if id == nil then
    local n = 0
    for k in pairs(lib) do n = math.max(n, tonumber(k) or 0) end
    id = n + 1
  end
  id = tostring(id)
  lib[id] = clean_entry(entry)
  M.save()
  return id
end

local function refers(v, id)
  return is_table(v) and v.ref ~= nil and tostring(v.ref) == id
end

-- fn(set) for the job's main set, crossbars and every sub set.
local function each_set(main, fn)
  for _, part in ipairs({ "main", "pad" }) do
    local set = job_part(main, part, false)
    if set then fn(set) end
  end
  for _, set in pairs(job_part(main, "sub", false) or {}) do
    if is_table(set) then fn(set) end
  end
end

-- How many slots name the entry.
function M.uses(main, id)
  id = tostring(id)
  local n = 0
  each_set(main, function(set)
    for _, row in pairs(set) do
      if is_table(row) then
        for _, v in pairs(row) do
          if refers(v, id) then n = n + 1 end
        end
      end
    end
  end)
  return n
end

-- A group's last-picked member, per character.
local function last_key(macro)
  local ok, mj = pcall(function() return require("src.jobs").current() end)
  return tostring(ok and mj or "") .. ":" .. tostring(macro.ref)
end

function M.last_member(macro)
  local t = storage and storage.character and storage.character.hb_last
  local k = type(t) == "table" and tonumber(t[last_key(macro)]) or 1
  return math.max(1, math.min(k, #(macro.group or {})))
end

function M.set_last_member(macro, k)
  if not (storage and storage.character) then return end
  if type(storage.character.hb_last) ~= "table" then storage.character.hb_last = {} end
  local lk = last_key(macro)
  if storage.character.hb_last[lk] == k then return end
  storage.character.hb_last[lk] = k
  pcall(storage.save)
end

function M.current_member(macro)
  return macro.group and macro.group[M.last_member(macro)] or nil
end

-- Removes an entry and every slot naming it. A sub set's slot over a filled main slot is blanked (false) so the main
-- macro does not show through.
function M.delete_entry(main, id)
  id = tostring(id)
  local lib = job_part(main, "library", false)
  if lib then lib[id] = nil end
  local last = storage and storage.character and storage.character.hb_last
  if type(last) == "table" and last[tostring(main) .. ":" .. id] ~= nil then
    last[tostring(main) .. ":" .. id] = nil
    pcall(storage.save)
  end
  local main_set = find_set(main, nil, false)
  each_set(main, function(set)
    for rk, row in pairs(set) do
      if is_table(row) then
        for sk, v in pairs(row) do
          if refers(v, id) then
            local under = set ~= main_set and raw_slot(main_set, rk, sk)
            if is_table(under) and not refers(under, id) then row[sk] = false else row[sk] = nil end
          end
        end
        if next(row) == nil then set[rk] = nil end
      end
    end
  end)
  M.save()
end

-- ---------------------------------------------------------------------------
-- Bars
-- ---------------------------------------------------------------------------

-- The value in one set: a macro, a reference, false (blanked) or nil.
function M.get_raw(main, sub, bar, slot)
  return raw_slot(find_set(main, sub, false), bar, slot)
end

-- A slot's macro for main/sub and where it came from ("main" or "sub"), or nil.
function M.resolve(main, sub, bar, slot)
  if sub then
    local v = raw_slot(find_set(main, sub, false), bar, slot)
    if v == false then return nil end
    if is_table(v) then
      local m = deref(main, v)
      if m then return m, "sub" end
      return nil
    end
  end
  local m = deref(main, raw_slot(find_set(main, nil, false), bar, slot))
  if m then return m, "main" end
  return nil
end

-- Every slot for main/sub: eff[bar][slot] = { macro, from = "main" | "sub" }.
function M.effective(main, sub)
  local eff = {}
  local function overlay(set, from)
    for bk, row in pairs(set or {}) do
      local b = tonumber(bk)
      if b and is_table(row) then
        for sk, v in pairs(row) do
          local s = tonumber(sk)
          if s then
            eff[b] = eff[b] or {}
            if v == false then
              eff[b][s] = nil
            elseif is_table(v) then
              local macro = deref(main, v)
              eff[b][s] = macro and { macro = macro, from = from } or nil
            end
          end
        end
      end
    end
  end
  overlay(find_set(main, nil, false), "main")
  if sub then overlay(find_set(main, sub, false), "sub") end
  return eff
end

function M.assign(main, sub, bar, slot, id)
  put_slot(find_set(main, sub, true), bar, slot, { ref = tostring(id) })
  M.save()
end

-- Puts a macro on a slot: a reference stays one, anything else is copied.
function M.set_macro(main, sub, bar, slot, macro)
  if macro.ref ~= nil then
    M.assign(main, sub, bar, slot, macro.ref)
    return
  end
  local lines = {}
  for _, l in ipairs(macro.lines or {}) do lines[#lines + 1] = tostring(l) end
  local m = { name = trim(macro.name), lines = lines }
  if trim(macro.label) ~= "" then m.label = trim(macro.label) end
  put_slot(find_set(main, sub, true), bar, slot, m)
  M.save()
end

-- Empties a slot; in a sub set over a filled main slot it is blanked (false).
function M.clear(main, sub, bar, slot)
  local set = find_set(main, sub, true)
  if sub and is_table(raw_slot(find_set(main, nil, false), bar, slot)) then
    put_slot(set, bar, slot, false)
  else
    drop_slot(set, bar, slot)
  end
  M.save()
end

-- Swaps what two slots show; references stay references.
function M.swap(main, sub, b1, s1, b2, s2)
  local m1 = M.resolve(main, sub, b1, s1)
  local m2 = M.resolve(main, sub, b2, s2)
  local function put(b, s, m)
    if m then M.set_macro(main, sub, b, s, m) else M.clear(main, sub, b, s) end
  end
  put(b1, s1, m2)
  put(b2, s2, m1)
end

-- ---------------------------------------------------------------------------
-- Crossbars
-- ---------------------------------------------------------------------------

function M.pad_get_raw(main, side, button)
  local v = raw_slot(job_part(main, "pad", false), side, button)
  return is_table(v) and v or nil
end

function M.pad_resolve(main, side, button)
  return deref(main, M.pad_get_raw(main, side, button))
end

local function pad_put(main, side, button, v)
  if v then
    put_slot(job_part(main, "pad", true), side, button, v)
  else
    local set = job_part(main, "pad", false)
    if set then drop_slot(set, side, button) end
  end
end

function M.pad_assign(main, side, button, id)
  pad_put(main, side, button, { ref = tostring(id) })
  M.save()
end

function M.pad_clear(main, side, button)
  pad_put(main, side, button, nil)
  M.save()
end

function M.pad_swap(main, s1, b1, s2, b2)
  local v1, v2 = M.pad_get_raw(main, s1, b1), M.pad_get_raw(main, s2, b2)
  pad_put(main, s1, b1, v2)
  pad_put(main, s2, b2, v1)
  M.save()
end

return M
