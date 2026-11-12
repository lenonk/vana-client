-- Macro storage and job-based resolution.
--
-- macros.json (in the mod's data folder) is nested by the job you play as MAIN:
--
--   {
--     "RDM": {
--       "library": { "<id>": <entry> },                    -- the job's macros, put on slots by reference
--       "main": { "<bar>": { "<slot>": <macro> } },        -- loads whenever RDM is main
--       "sub":  { "WHM": { "<bar>": { "<slot>": <macro> } } } -- loads when RDM/WHM, overrides "main"
--     }
--   }
--
-- A macro is { name = "Cure", label = "Cure" (optional), lines = { "/ma \"Cure\" <t>", ... } }, or { ref = "<id>" }:
-- the library entry of that id, so editing an entry changes every slot that names it.
-- In a "sub" set a slot of `false` blanks a slot the main set filled.
--
-- A library entry is a macro whose first line can be built from an action:
-- { name, label?, action = { kind = "ma", name = "Cure", target = "t" }?, lines = { <lines after it> } }.
-- An equip action's target is the equipment slot ({ kind = "equip", name = "Warp Ring", target = "ring1" }); a system
-- command's name is the command itself ({ kind = "cmd", name = "/attack", target = "t" }). An action's `line`, when
-- set, is the player's line run in place of the one it builds.
-- A group entry stands for several: { name, label?, group = { <entry>, ... } }; its slot opens a list of them to pick
-- one from (src/chooser.lua), the one picked last first.
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

-- ---------------------------------------------------------------------------
-- Library
-- ---------------------------------------------------------------------------

local function library(main, create)
  if not main then
    return nil
  end
  local job = db[main]
  if not is_table(job) then
    if not create then return nil end
    job = {}
    db[main] = job
  end
  if not is_table(job.library) then
    if not create then return nil end
    job.library = {}
  end
  return job.library
end

-- A system command's action (kind "cmd") names the command itself.
local COMMAND = { ma = "/ma", ja = "/ja", ws = "/ws", pet = "/pet", item = "/item", equip = "/equip", cmd = "" }
M.ACTION_KINDS = COMMAND

-- The line an action builds: { kind = "ma", name = "Cure", target = "t" } -> /ma "Cure" <t>;
-- { kind = "equip", name = "Warp Ring", target = "ring1" } -> /equip ring1 "Warp Ring";
-- { kind = "cmd", name = "/attack", target = "t" } -> /attack <t>; { kind = "cmd", name = "/heal" } -> /heal.
-- An action with its `line` runs that.
function M.command(action)
  if not is_table(action) or not COMMAND[action.kind] or trim(action.name) == "" then
    return nil
  end
  if trim(action.line) ~= "" then
    return trim(action.line)
  end
  local target = trim(action.target)
  if action.kind == "equip" then
    if target == "" then return nil end
    return ('/equip %s "%s"'):format(target, trim(action.name))
  end
  if action.kind == "cmd" then
    return target ~= "" and ("%s <%s>"):format(trim(action.name), target) or trim(action.name)
  end
  local line = ('%s "%s"'):format(COMMAND[action.kind], trim(action.name))
  if target ~= "" then
    line = line .. " <" .. target .. ">"
  end
  return line
end

-- An entry's lines as they run: its action's line, then the entry's (none for a group: its members run).
function M.entry_lines(entry)
  local out = {}
  if is_table(entry.group) then return out end
  local first = M.command(entry.action)
  if first then
    out[1] = first
  end
  for _, l in ipairs(entry.lines or {}) do
    out[#out + 1] = tostring(l)
  end
  return out
end

-- One runnable macro per entry table, so a macro that is still running ignores a second press. A saved entry is a
-- new table, so its old expansion is dropped with it.
local expanded = setmetatable({}, { __mode = "k" })

local function member(e)
  return { name = e.name or "", label = e.label, lines = M.entry_lines(e), action = e.action }
end

local function expand(entry, id)
  local m = expanded[entry]
  if not m then
    m = { name = entry.name or "", label = entry.label, lines = M.entry_lines(entry), ref = id, action = entry.action }
    if is_table(entry.group) then
      m.group = {}
      for k, e in ipairs(entry.group) do
        if is_table(e) then m.group[k] = member(e) end
      end
    end
    expanded[entry] = m
  end
  return m
end

-- A library entry as its slots run it (a group with its members), or nil.
function M.macro_of(main, id)
  local lib = library(main, false)
  local entry = lib and id ~= nil and lib[tostring(id)]
  return is_table(entry) and expand(entry, tostring(id)) or nil
end

-- A group's member picked last (kept per character), 1 when none was; and setting it.
local function last_key(macro)
  local ok, mj = pcall(function() return require("src.jobs").current() end)
  return tostring(ok and mj or "") .. ":" .. tostring(macro.ref)
end

function M.last_member(macro)
  local t = storage and storage.character and storage.character.hb_last
  local k = type(t) == "table" and tonumber(t[last_key(macro)]) or 1
  return math.max(1, math.min(k, #(macro.group or {})))
end

-- Saved only when it changes: a group picked again and again in a fight writes nothing.
function M.set_last_member(macro, k)
  if not (storage and storage.character) then return end
  if type(storage.character.hb_last) ~= "table" then storage.character.hb_last = {} end
  local key = last_key(macro)
  if storage.character.hb_last[key] == k then return end
  storage.character.hb_last[key] = k
  pcall(storage.save)
end

-- A deleted entry's remembered member goes with it.
local function forget_last_member(main, id)
  local t = storage and storage.character and storage.character.hb_last
  local key = tostring(main) .. ":" .. tostring(id)
  if type(t) == "table" and t[key] ~= nil then
    t[key] = nil
    pcall(storage.save)
  end
end

-- The member a group's slot shows: the one picked last.
function M.current_member(macro)
  return macro.group and macro.group[M.last_member(macro)] or nil
end

-- The macro a slot value stands for: its library entry for a ref (nil when that entry is gone), else itself.
local function deref(main, v)
  if not is_table(v) then
    return nil
  end
  if v.ref ~= nil then
    local lib = library(main, false)
    local entry = lib and lib[tostring(v.ref)]
    return is_table(entry) and expand(entry, tostring(v.ref)) or nil
  end
  return v
end

local function entry_id_order(a, b)
  local na, nb = tonumber(a), tonumber(b)
  if na and nb then return na < nb end
  return tostring(a) < tostring(b)
end

-- The job's entries, oldest first: { { id = "1", entry = {...} }, ... }.
function M.library(main)
  local lib = library(main, false)
  local ids = {}
  for id, entry in pairs(lib or {}) do
    if is_table(entry) then ids[#ids + 1] = id end
  end
  table.sort(ids, entry_id_order)
  local out = {}
  for _, id in ipairs(ids) do
    out[#out + 1] = { id = id, entry = lib[id] }
  end
  return out
end

function M.get_entry(main, id)
  local lib = library(main, false)
  local entry = lib and id ~= nil and lib[tostring(id)]
  return is_table(entry) and entry or nil
end

local clean_entry

local function clean_group(list)
  local out = {}
  for _, e in ipairs(list) do
    if is_table(e) and not is_table(e.group) then
      local c = clean_entry(e)
      if c.action or #c.lines > 0 then out[#out + 1] = c end
    end
  end
  return out
end

clean_entry = function(e)
  if is_table(e.group) then
    local out = { name = trim(e.name), group = clean_group(e.group) }
    if trim(e.label) ~= "" then out.label = trim(e.label) end
    return out
  end
  local lines = {}
  for _, l in ipairs(e.lines or {}) do
    l = trim(l)
    if l ~= "" then lines[#lines + 1] = l end
  end
  local out = { name = trim(e.name), lines = lines }
  if trim(e.label) ~= "" then out.label = trim(e.label) end
  if is_table(e.action) and COMMAND[e.action.kind] and trim(e.action.name) ~= "" then
    out.action = { kind = e.action.kind, name = trim(e.action.name), target = trim(e.action.target) }
    if trim(e.action.line) ~= "" then out.action.line = trim(e.action.line) end
  end
  return out
end

-- Saves an entry under `id`, or under a new id when `id` is nil. Returns the id.
function M.save_entry(main, id, entry)
  local lib = library(main, true)
  if id == nil then
    local n = 0
    for k in pairs(lib) do
      n = math.max(n, tonumber(k) or 0)
    end
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

-- Every set of the job (its main set, then each sub job's): fn(set).
local function each_set(main, fn)
  local job = db[main]
  if not is_table(job) then return end
  if is_table(job.main) then fn(job.main) end
  for _, set in pairs(is_table(job.sub) and job.sub or {}) do
    if is_table(set) then fn(set) end
  end
end

-- How many slots of the job's sets (main and sub) name the entry.
function M.uses(main, id)
  id = tostring(id)
  local n = 0
  each_set(main, function(set)
    for _, bar in pairs(set) do
      if is_table(bar) then
        for _, v in pairs(bar) do
          if refers(v, id) then n = n + 1 end
        end
      end
    end
  end)
  return n
end

-- Removes an entry and empties every slot that names it.
-- A sub set's slot that named it stays blank (false) where the main set fills that slot: emptied (nil), it would let
-- the main set's macro show there again, which the entry had covered.
function M.delete_entry(main, id)
  id = tostring(id)
  local lib = library(main, false)
  if lib then lib[id] = nil end
  forget_last_member(main, id)
  local job = db[main]
  local main_set = is_table(job) and is_table(job.main) and job.main or nil
  each_set(main, function(set)
    local blank = set ~= main_set
    for bk, bar in pairs(set) do
      if is_table(bar) then
        for sk, v in pairs(bar) do
          if refers(v, id) then
            local under = blank and main_set and is_table(main_set[bk]) and main_set[bk][sk]
            if is_table(under) and not refers(under, id) then
              bar[sk] = false
            else
              bar[sk] = nil
            end
          end
        end
        if next(bar) == nil then set[bk] = nil end
      end
    end
  end)
  M.save()
end

-- ---------------------------------------------------------------------------
-- Resolving
-- ---------------------------------------------------------------------------

-- The value in exactly one set: a macro table, a reference ({ref}), false (blanked) or nil.
function M.get_raw(main, sub, bar, slot)
  return raw_slot(find_set(main, sub, false), bar, slot)
end

-- What a slot does for this main/sub pair: the macro and where it came from
-- ("sub" overrides "main"), or nil when the slot is empty. A slot naming a library entry gives the entry's macro,
-- with `ref` its id.
function M.resolve(main, sub, bar, slot)
  if sub then
    local v = raw_slot(find_set(main, sub, false), bar, slot)
    if v == false then
      return nil
    end
    if is_table(v) then
      local m = deref(main, v)
      if m then return m, "sub" end
      return nil
    end
  end
  local m = deref(main, raw_slot(find_set(main, nil, false), bar, slot))
  if m then
    return m, "main"
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
              local macro = deref(main, m)
              eff[b][s] = macro and { macro = macro, from = from } or nil
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

local function put_raw(main, sub, bar, slot, v)
  local set = find_set(main, sub, true)
  local bk = key(bar)
  if not is_table(set[bk]) then
    set[bk] = {}
  end
  set[bk][key(slot)] = v
end

-- Puts a library entry on a slot of one set.
function M.assign(main, sub, bar, slot, id)
  put_raw(main, sub, bar, slot, { ref = tostring(id) })
  M.save()
end

-- The library entry a slot names (as it shows for this main/sub pair), or nil.
function M.ref_of(main, sub, bar, slot)
  local m = M.resolve(main, sub, bar, slot)
  return m and m.ref or nil
end

-- Puts a macro on a slot of one set: a reference stays one (a macro with `ref`, as resolve gives), else a copy.
function M.set_macro(main, sub, bar, slot, macro)
  if macro.ref ~= nil then
    M.assign(main, sub, bar, slot, macro.ref)
    return
  end
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

-- Swaps two slots of one set by what each shows now (a slot the main set fills under a sub set takes that macro); a
-- slot naming a library entry still names it.
function M.swap(main, sub, b1, s1, b2, s2)
  local m1 = M.resolve(main, sub, b1, s1)
  local m2 = M.resolve(main, sub, b2, s2)
  local function put(b, s, m)
    if m then M.set_macro(main, sub, b, s, m) else M.clear(main, sub, b, s) end
  end
  put(b1, s1, m2)
  put(b2, s2, m1)
end

return M
