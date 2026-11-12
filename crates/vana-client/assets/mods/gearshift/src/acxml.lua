-- Ashitacast's XML profiles, run as Ashitacast runs them: sections walked as flow (if / elseif / else / return),
-- rules on its variables, equips collected per section and sent around the actions we send, its <commands> and /ac.
-- v3 files are read through Ashitacast's import rules. Odd behaviour a working file can rely on is kept; what
-- only hangs, crashes or does nothing in the original does what was meant.

local core = require("src.core")
local compat = require("src.compat")
local names = require("src.names")
local xml = require("src.xml")

local M = {}
M.file = nil

local SLOTS = { "main", "sub", "range", "ammo", "head", "body", "hands", "legs", "feet", "neck", "waist", "ear1", "ear2",
  "ring1", "ring2", "back" }
local SLOT = {}
for i, n in ipairs(SLOTS) do SLOT[n] = i - 1 end

local SECTIONS = { "sets", "load", "unload", "commands", "default", "preranged", "midranged", "precast", "midcast", "ability",
  "weaponskill", "item", "petskill", "petspell" }
-- How a section's equips go: one 0x050 each, one 0x051, 0x050s up to 8 then a 0x051, or not at all.
local MODE = { precast = "single", preranged = "single", midcast = "set", midranged = "set", default = "auto",
  ability = "auto", weaponskill = "auto", item = "auto", petskill = "auto", petspell = "auto", load = "none",
  unload = "none", commands = "none" }

local prof        -- {root, sec = {name -> node}, set = {}, cfg = {}, path}
local uvars = {}
local act         -- the current-action record (§ type, active, post, cancel, target, id)
local hold, pet_hold, gear_lock = 0, 0, 0
local pet_override = false
local disabled = {}
local overrides = {} -- {id, active, ends} in registration order
local debug = false
local args          -- a <commands> match's arguments: {text, [0] = word, ...}
local warned = {}
local last_pos, moving = nil, false
local depth = 0     -- commands run from commands
local through = {}  -- lines sent "not seen by Ashita" (mode >= 0): let through once

local function say(text) chat.add("Gearshift: " .. text) end
local function warn(text)
  if not warned[text] then
    warned[text] = true
    say(text)
  end
end

-- ---- C's number reading ----

local function atoi(s)
  return math.tointeger(tonumber((tostring(s or "")):match("^%s*([%+%-]?%d+)") or "0")) or 0
end

local function atof(s)
  local t = tostring(s or "")
  local m = t:match("^%s*([%+%-]?%d*%.?%d+[eE][%+%-]?%d+)") or t:match("^%s*([%+%-]?%d*%.?%d*)")
  return tonumber(m) or 0
end

-- Digits with at most one '.'; the empty string too.
local function numeric(s)
  if s == "" then return true end
  if not s:match("^[%d%.]+$") then return false end
  local _, dots = s:gsub("%.", "")
  return dots <= 1
end

local function fmt(v, float)
  if float then return string.format("%f", v) end
  return tostring(math.tointeger(v) or math.floor(v))
end

-- `s` split on `sep`, empty pieces kept.
local function split(s, sep)
  local out, i = {}, 1
  while true do
    local j = s:find(sep, i, true)
    if not j then
      out[#out + 1] = s:sub(i)
      return out
    end
    out[#out + 1] = s:sub(i, j - 1)
    i = j + 1
  end
end

local function trim_spaces(s) return (s:gsub("^ +", ""):gsub(" +$", "")) end

-- Ashitacast's wildcard match: '*' alone is special; characters compare with 0x20 or-ed in.
local function wild(t, p, i, j)
  while j <= #p do
    local c = p:byte(j)
    if c == 42 then
      if j == #p then return true end
      for k = i, #t do
        if wild(t, p, k, j + 1) then return true end
      end
      return false
    end
    if i > #t or (t:byte(i) | 0x20) ~= (c | 0x20) then return false end
    i, j = i + 1, j + 1
  end
  return i > #t
end

-- ---- Game data in Ashitacast's words ----

local STATUS = { [0] = "idle", "engaged", "dead", "dead", "zoning", [33] = "resting" }
local ELEMENT = { [0] = "fire", "ice", "wind", "earth", "thunder", "water", "light", "dark", [15] = "nonelemental" }
local SKILL = { [32] = "divinemagic", [33] = "healingmagic", [34] = "enhancingmagic", [35] = "enfeeblingmagic",
  [36] = "elementalmagic", [37] = "darkmagic", [38] = "summoning", [39] = "ninjutsu", [40] = "singing", [43] = "bluemagic",
  [44] = "geomancy" }
local SPELL_TYPE = { "whitemagic", "blackmagic", "summoning", "ninjutsu", "bardsong", "bluemagic" }
local ABILITY_TYPE = { [10] = "runeenchantment", [102] = "ready", [173] = "bloodpactrage", [174] = "bloodpactward",
  [193] = "corsairroll", [195] = "quickdraw", [217] = "waltz" }
local DAYS = { [0] = "firesday", "earthsday", "watersday", "windsday", "iceday", "lightningday", "lightsday", "darksday" }
local DAY_ELEMENT = { [0] = "fire", "earth", "water", "wind", "ice", "thunder", "light", "dark" }
local WEATHER = { [0] = "clear", "sunshine", "clouds", "fog", "fire", "firex2", "water", "waterx2", "earth", "earthx2",
  "wind", "windx2", "ice", "icex2", "thunder", "thunderx2", "light", "lightx2", "dark", "darkx2" }
local WEATHER_ELEMENT = { [0] = "none", "none", "none", "none", "fire", "fire", "water", "water", "earth", "earth", "wind",
  "wind", "ice", "ice", "thunder", "thunder", "light", "light", "dark", "dark" }

local function moon_name(i)
  if i <= 2 or i >= 80 then return "fullmoon" end
  if i <= 16 then return "waninggibbous" end
  if i <= 24 then return "lastquarter" end
  if i <= 37 then return "waningcrescent" end
  if i <= 44 then return "newmoon" end
  if i <= 57 then return "waxingcrescent" end
  if i <= 65 then return "firstquarter" end
  return "waxinggibbous"
end

-- The Vana'diel day count Ashitacast's clock gives (its weekday agrees with the game's).
local function day_count()
  local t = game.time()
  for _, off in ipairs({ 92514960, 0 }) do
    local d = (t.earth + off) // 3456
    if d % 8 == t.weekday then return d, t end
  end
  return t.earth // 3456, t
end

local function me() return game.player() or {} end

local function entity_at(index)
  if not index then return nil end
  local p = me()
  if index == p.index then return game.entity(p.id) end
  for _, e in ipairs(game.entities() or {}) do
    if e.index == index then return e end
  end
end

local function spawn_type(e, self_word)
  if not e then return "SPAWN_FLAGS_ERROR" end
  if e.me then return self_word or "party" end
  if e.kind == "mob" then return "monster" end
  if e.kind == "npc" then return "npc" end
  if e.in_party then return "party" end
  if e.in_alliance then return "alliance" end
  return "pc"
end

local function job_abbr(id) return res.job(id or 0) or "NON" end

local function pet()
  local e = game.entity_by_tag("pet")
  if e and (e.hpp or 0) > 0 then return e end
end

-- Buff names by id (the status names), lowered, for $b_ by name.
local buff_names
local function buffs_named()
  if not buff_names then
    buff_names = {}
    for id = 0, 649 do
      local n = res.status(id)
      if n and n ~= "" then buff_names[#buff_names + 1] = { id = id, name = n:lower() } end
    end
  end
  return buff_names
end

-- How many of these buff ids we have: a live registerbuff override answers alone.
local function buff_count(ids)
  local now = clock()
  for i = #overrides, 1, -1 do
    if now >= overrides[i].ends then table.remove(overrides, i) end
  end
  for _, o in ipairs(overrides) do
    if ids[o.id] then return o.active and 1 or 0 end
  end
  local n = 0
  for _, b in ipairs(game.buffs() or {}) do
    if ids[b.id] then n = n + 1 end
  end
  return n
end

local spell_ids, ability_ids
local function index_names()
  if spell_ids then return end
  spell_ids, ability_ids = {}, {}
  for id = 1, 1023 do
    local s = res.spell(id)
    if s and s.name and s.name ~= "" then spell_ids[s.name:lower()] = spell_ids[s.name:lower()] or id end
  end
  for id = 0, 0x7ff do
    local a = res.ability(id)
    if a and a.name and a.name ~= "" then ability_ids[a.name:lower()] = ability_ids[a.name:lower()] or id end
  end
end

-- The action's resource: a spell, an ability (by name-table id), an item, a monster ability.
local function act_res()
  if not act then return nil end
  local t = act.type
  if t == "Spell" or t == "PetSpell" then return res.spell(act.id) end
  if t == "Ability" or t == "PetSkill_Ability" then return res.ability(act.id + 0x200) end
  if t == "Weaponskill" then return res.ability(act.id) end
  if t == "Item" then return res.item(act.id) end
  if t == "PetSkill_MobSkill" then return { name = res.mob_skill(act.id) } end
end

local UNRES = "UNRESOLVED_VARIABLE"
local function ad(f)
  return function()
    if not act or not act.active then return UNRES end
    return f()
  end
end
local function magic(f, err)
  return ad(function()
    if act.type ~= "Spell" and act.type ~= "PetSpell" then return err or UNRES end
    local r = res.spell(act.id)
    if not r then return "UNRESOLVED_RESOURCE" end
    return f(r)
  end)
end

local function target_field(f)
  return function()
    local e = game.target()
    if not e then return "NO_TARGET" end
    return f(e)
  end
end
local function action_target(f)
  return ad(function()
    local e = entity_at(act.target)
    if not e then return UNRES end
    return f(e)
  end)
end
local function pet_field(f)
  return function()
    local e = pet()
    if not e then return "PET_DOESNT_EXIST" end
    return f(e)
  end
end

local function party_has(index, last)
  for _, m in ipairs(game.party() or {}) do
    if m.slot <= last and m.index == index then return true end
  end
  return false
end

local function resist(i) return function() local s = game.stats() return tostring(s and s.resist and s.resist[i] or 0) end end

local function eq_slot(slot)
  return function()
    local w = core.worn()[slot]
    if not w then return "empty" end
    local r = core.record(w.id)
    return r and r.name or "ITEM_RESOURCE_ERROR"
  end
end

local BUILTIN = {
  p_hp = function() return tostring(me().hp or 0) end,
  p_mp = function() return tostring(me().mp or 0) end,
  p_tp = function() return tostring(me().tp or 0) end,
  p_hpmax = function() return tostring(me().hp_max or 0) end,
  p_mpmax = function() return tostring(me().mp_max or 0) end,
  p_hpp = function() return tostring(me().hpp or 0) end,
  p_mpp = function() return tostring(me().mpp or 0) end,
  p_attack = function() local s = game.stats() return tostring(s and s.attack or 0) end,
  p_defense = function() local s = game.stats() return tostring(s and s.defense or 0) end,
  p_fireresist = resist(1), p_iceresist = resist(2), p_windresist = resist(3), p_earthresist = resist(4),
  p_lightningresist = resist(5), p_waterresist = resist(6), p_lightresist = resist(7), p_darkresist = resist(8),
  p_name = function() return me().name or "" end,
  p_mainjob = function() return job_abbr(me().main_job) end,
  p_subjob = function() return job_abbr(me().sub_job) end,
  p_job = function() return job_abbr(me().main_job) .. "/" .. job_abbr(me().sub_job) end,
  p_level = function() return tostring(me().main_level or 0) end,
  p_subjoblevel = function() return tostring(me().sub_level or 0) end,
  p_status = function() return STATUS[me().status or 0] or "ENTITY_STATUS_ERROR" end,
  p_ismoving = function() return moving and "true" or "false" end,
  pt_count = function()
    local n = 0
    for _, m in ipairs(game.party() or {}) do if m.slot <= 5 then n = n + 1 end end
    return tostring(math.max(n, 1))
  end,
  pt_inparty = function()
    for _, m in ipairs(game.party() or {}) do if m.slot >= 1 and m.slot <= 5 then return "true" end end
    return "false"
  end,
  pt_target = function() local t = game.target() return tostring(t ~= nil and (t.me or party_has(t.index, 5))) end,
  pt_actiontarget = function() return tostring(act ~= nil and act.active and party_has(act.target, 5)) end,
  a_count = function() return tostring(math.max(#(game.party() or {}), 1)) end,
  a_inally = function()
    for _, m in ipairs(game.party() or {}) do if m.slot >= 6 then return "true" end end
    return "false"
  end,
  a_target = function() local t = game.target() return tostring(t ~= nil and (t.me or party_has(t.index, 17))) end,
  a_actiontarget = function() return tostring(act ~= nil and act.active and party_has(act.target, 17)) end,
  t_name = target_field(function(e) return e.name end),
  t_index = target_field(function(e) return tostring(e.index) end),
  t_id = target_field(function(e) return tostring(e.id) end),
  t_hpp = target_field(function(e) return tostring(e.hpp or 0) end),
  t_distance = target_field(function(e) return string.format("%f", e.distance or 0) end),
  t_status = target_field(function(e) return STATUS[e.status or 0] or "ENTITY_STATUS_ERROR" end),
  t_type = target_field(function(e) return spawn_type(e) end),
  ad_id = ad(function() return tostring(act.id) end),
  ad_name = ad(function()
    if act.type == "Ranged" then return UNRES end
    local r = act_res()
    return r and r.name or "UNRESOLVED_RESOURCE"
  end),
  ad_casttime = magic(function(r) return tostring(math.floor((r.cast_time or 0) * 1000 + 0.5)) end),
  ad_recast = magic(function(r) return tostring(math.floor((r.recast or 0) * 1000 + 0.5)) end),
  ad_mpcost = magic(function(r) return tostring(r.mp or 0) end),
  ad_mpaftercast = magic(function(r) return tostring((me().mp or 0) - (r.mp or 0)) end),
  ad_mppaftercast = magic(function(r)
    local max = me().mp_max or 0
    if max <= 0 then return "0" end
    return tostring(((me().mp or 0) - (r.mp or 0)) * 100 // max)
  end),
  ad_element = magic(function(r) return ELEMENT[r.element] or "ELEMENT_INDEX_ERROR" end, "ELEMENT_INDEX_ERROR"),
  ad_skill = magic(function(r) return SKILL[r.skill] or "SKILL_INDEX_ERROR" end, "SKILL_INDEX_ERROR"),
  ad_type = ad(function()
    local t = act.type
    if t == "Spell" or t == "PetSpell" then
      local r = res.spell(act.id)
      return r and SPELL_TYPE[r.kind] or "SPELL_TYPE_INDEX_ERROR"
    end
    if t == "Ability" or t == "PetSkill_Ability" then
      local r = res.ability(act.id + 0x200)
      return r and ABILITY_TYPE[r.recast_id] or "ABIL_TYPE_INDEX_ERROR"
    end
    if t == "PetSkill_MobSkill" then
      index_names()
      local id = ability_ids[(res.mob_skill(act.id) or ""):lower()]
      local r = id and id >= 0x200 and res.ability(id)
      return r and ABILITY_TYPE[r.recast_id] or "ABIL_TYPE_INDEX_ERROR"
    end
    return UNRES
  end),
  at_index = ad(function() return tostring(act.target or 0) end),
  at_id = action_target(function(e) return tostring(e.id) end),
  at_name = action_target(function(e) return e.name end),
  at_hpp = action_target(function(e) return tostring(e.hpp or 0) end),
  at_distance = action_target(function(e) return string.format("%f", e.distance or 0) end),
  at_status = action_target(function(e) return STATUS[e.status or 0] or "ENTITY_STATUS_ERROR" end),
  at_type = action_target(function(e) return spawn_type(e, "self") end),
  pet_active = function() return pet() and "true" or "false" end,
  pet_hpp = pet_field(function(e) return tostring(e.hpp or 0) end),
  pet_tp = pet_field(function(e) return tostring(e.tp or 0) end),
  pet_status = pet_field(function(e) return STATUS[e.status or 0] or "ENTITY_STATUS_ERROR" end),
  pet_name = pet_field(function(e) return e.name end),
  pet_distance = pet_field(function(e) return string.format("%f", e.distance or 0) end),
  e_time = function() local t = game.time() return string.format("%02d.%02d", t.hour, t.minute) end,
  e_day = function() return DAYS[day_count() % 8] end,
  e_dayelement = function() return DAY_ELEMENT[day_count() % 8] end,
  e_moon = function() return moon_name((day_count() + 26) % 84) end,
  e_moonpct = function()
    local i = (day_count() + 26) % 84
    return tostring(math.floor(math.abs(42 - i) * 100 / 42 + 0.5))
  end,
  e_weather = function()
    for _, b in ipairs(game.buffs() or {}) do
      if names.STORMS[b.id] then return WEATHER[names.STORMS[b.id]] end
    end
    return WEATHER[game.weather() or 0] or "WEATHER_ERROR"
  end,
  e_weatherelement = function()
    for _, b in ipairs(game.buffs() or {}) do
      if names.STORMS[b.id] then return WEATHER_ELEMENT[names.STORMS[b.id]] end
    end
    return WEATHER_ELEMENT[game.weather() or 0] or "WEATHER_ERROR"
  end,
  e_area = function() return res.zone(me().zone or 0) or "" end,
}
for i, n in ipairs(SLOTS) do BUILTIN["eq_" .. n] = eq_slot(i - 1) end
local BUILTIN_NAMES = {}
for k in pairs(BUILTIN) do BUILTIN_NAMES[#BUILTIN_NAMES + 1] = k end

-- ---- $ substitution ----

-- The longest of `list` (names) that starts `text`, case counting.
local function longest(list, text)
  local best
  for _, n in ipairs(list) do
    if (not best or #n > #best) and text:sub(1, #n) == n then best = n end
  end
  return best
end

local function resolve(s)
  s = tostring(s or "")
  -- The last $ first, again and again: a value can finish a name to its left. Bounded (a value naming itself).
  for _ = 1, 256 do
    local p
    local i = 0
    while true do
      local j = s:find("$", i + 1, true)
      if not j then break end
      p, i = j, j
    end
    if not p then return s end
    local r = s:sub(p + 1)
    local cut, value = 1, nil
    if r:sub(1, 2) == "u_" then
      local list = {}
      for k in pairs(uvars) do list[#list + 1] = k end
      local n = longest(list, r:sub(3))
      if n then cut, value = 3 + #n, uvars[n] end
    elseif r:sub(1, 2) == "b_" then
      local head = r:sub(3, 5)
      local _, digits = head:gsub("%d", "")
      if digits > 0 then
        local id = atoi(head:sub(1, digits))
        cut, value = 3 + digits, tostring(buff_count({ [id] = true }))
      else
        local text, len, ids = r:sub(3):lower(), 0, {}
        for _, b in ipairs(buffs_named()) do
          if text:sub(1, #b.name) == b.name and #b.name >= len then
            if #b.name > len then ids, len = {}, #b.name end
            ids[b.id] = true
          end
        end
        cut, value = 3 + len, tostring(buff_count(ids))
      end
    elseif r:sub(1, 3) == "arg" then
      if args then
        if r:sub(4, 4) == "s" then
          cut, value = 5, args.text
        else
          local n, len = atoi(r:sub(4, 4)), 4
          while r:sub(len + 1, len + 1):match("%w") and atoi(r:sub(4, len + 1)) < #args + 1 do
            len = len + 1
            n = atoi(r:sub(4, len))
          end
          if n <= #args then cut, value = len + 1, args[n] end
        end
      end
    else
      local n = longest(BUILTIN_NAMES, r)
      if n then cut, value = 1 + #n, BUILTIN[n]() end
    end
    -- An unknown name loses its $ and stays as text.
    s = s:sub(1, p - 1) .. (value or "") .. (value and r:sub(cut) or r)
  end
  return s
end

-- ---- Rules ----

local OPS = "^/*+-|&<>=!"
local function is_op(c) return c ~= "" and OPS:find(c, 1, true) ~= nil end

-- A comparison (`=`/`==`, `!`/`!=`, `<`, `<=`, `>`, `>=`): numbers when either side is numeric (an empty side
-- is), else strings, matched with wildcards (`wildcards`) or exactly.
local function compare(l, op, r, wildcards)
  local base = op:sub(1, 1)
  if base == "=" or base == "!" then
    local eq
    if numeric(l) or numeric(r) then
      if l:find(".", 1, true) or r:find(".", 1, true) then eq = atof(l) == atof(r) else eq = atoi(l) == atoi(r) end
    elseif wildcards then
      eq = wild(l, r, 1, 1)
    else
      eq = l:lower() == r:lower()
    end
    if base == "!" then return not eq end
    return eq
  end
  local a, b
  if l:find(".", 1, true) or r:find(".", 1, true) then a, b = atof(l), atof(r) else a, b = atoi(l), atoi(r) end
  if op == "<" then return a < b end
  if op == "<=" then return a <= b end
  if op == ">" then return a > b end
  return a >= b
end

-- A basic rule's comparisons, left to right. A two-character operator leaves its first character on the left side.
local function eval_basic(s)
  for _ = 1, 64 do
    local i = s:find("[<>=!]")
    if not i then return s end
    local op, lhs = s:sub(i, i), s:sub(1, i - 1)
    local j = i + 1
    if s:sub(j, j) == "=" then
      op, lhs, j = op .. "=", s:sub(1, i), j + 1
    end
    local k = s:find("[<>=!]", j) or #s + 1
    local rhs = s:sub(j, k - 1)
    s = tostring(compare(trim_spaces(lhs), op, trim_spaces(rhs), true)) .. s:sub(k)
  end
  return s
end

local function basic(name, value)
  local pieces, any
  if value:find("|", 1, true) then
    pieces, any = split(value, "|"), true
  elseif value:find("&", 1, true) then
    pieces, any = split(value, "&"), false
  else
    pieces, any = { value }, true
  end
  for _, p in ipairs(pieces) do
    local expr = p:find("[<>!]") and ("$" .. name .. p) or ("$" .. name .. "=" .. p)
    local ok = eval_basic(resolve(expr)) == "true"
    if any and ok then return true end
    if not any and not ok then return false end
  end
  return not any
end

-- One operator pass of the advanced evaluator over `s`, for the operators in `set`, left to right. A sign (a + or -
-- with nothing to its left) belongs to its number.
local function pass(s, set, calc)
  for _ = 1, 256 do
    local i
    for x = 1, #s do
      local c = s:sub(x, x)
      if set:find(c, 1, true) then
        local before = trim_spaces(s:sub(1, x - 1))
        local prev = before:sub(-1)
        local sign = (c == "+" or c == "-") and (before == "" or is_op(prev))
        if not sign then i = x break end
      end
    end
    if not i then return s end
    -- The left operand back to the operator before it (a sign in front of it goes with it).
    local a = i - 1
    while a >= 1 and not is_op(s:sub(a, a)) do a = a - 1 end
    if a >= 1 and (s:sub(a, a) == "-" or s:sub(a, a) == "+") then
      local before = trim_spaces(s:sub(1, a - 1))
      if before == "" or is_op(before:sub(-1)) then a = a - 1 end
    end
    local op, lhs, j = s:sub(i, i), s:sub(a + 1, i - 1), i + 1
    if s:sub(j, j) == "=" then
      op, lhs, j = op .. "=", s:sub(a + 1, i), j + 1
    end
    -- The right operand on to the next operator (it may start with a sign).
    local b = j
    while s:sub(b, b) == " " do b = b + 1 end
    if s:sub(b, b) == "-" or s:sub(b, b) == "+" then b = b + 1 end
    while b <= #s and not is_op(s:sub(b, b)) do b = b + 1 end
    local rhs = s:sub(j, b - 1)
    s = s:sub(1, a) .. calc(trim_spaces(lhs), op, trim_spaces(rhs)) .. s:sub(b)
  end
  return s
end

local function arith(l, op, r)
  local float = l:find(".", 1, true) or r:find(".", 1, true)
  local x, y = float and atof(l) or atoi(l), float and atof(r) or atoi(r)
  if op:sub(1, 1) == "^" then return fmt(x ^ y, true) end
  local c = op:sub(1, 1)
  if c == "*" then return fmt(x * y, float) end
  if c == "/" then
    if float then return fmt(y == 0 and math.huge * (x < 0 and -1 or 1) or x / y, true) end
    if y == 0 then return "0" end
    local q = x // y
    if q < 0 and q * y ~= x then q = q + 1 end
    return fmt(q, false)
  end
  if c == "+" then return fmt(x + y, float) end
  return fmt(x - y, float)
end

local function ops_text(s)
  s = pass(s, "^", arith)
  s = pass(s, "*/", arith)
  s = pass(s, "+-", arith)
  s = pass(s, "<>=!", function(l, op, r) return tostring(compare(l, op, r, false)) end)
  return pass(s, "|&", function(l, op, r)
    if op:sub(1, 1) == "|" then return tostring(l == "true" or r == "true") end
    return tostring(l == "true" and r == "true")
  end)
end

local function advanced(expr)
  local s = resolve(expr)
  for _ = 1, 256 do
    local close
    for x = #s, 1, -1 do
      if s:sub(x, x) == ")" then close = x break end
    end
    if not close then break end
    local open
    for x = close - 1, 1, -1 do
      if s:sub(x, x) == "(" then open = x break end
    end
    if open then
      s = s:sub(1, open - 1) .. ops_text(s:sub(open + 1, close - 1)) .. s:sub(close + 1)
    else
      s = s:sub(1, close - 1) .. s:sub(close + 1)
    end
  end
  s = s:gsub("%(", "")
  return ops_text(s) == "true"
end

-- A setvar value: substituted, then only * / + -.
local function setvar_value(v)
  local s = resolve(v)
  if not s:find("[%*/%+%-]") then return s end
  return pass(pass(s, "*/", arith), "+-", arith)
end

local function test(n)
  local any = false
  local m = xml.get(n, "mode")
  if m and m:lower() == "or" then any = true end
  for _, a in ipairs(n.attrs) do
    local ln = a.name:lower()
    if ln ~= "mode" then
      local ok
      if ln == "advanced" then ok = advanced(a.value) else ok = basic(a.name, a.value) end
      if any and ok then return true end
      if not any and not ok then return false end
    end
  end
  -- With no rules: AND is true, OR false.
  return not any
end

-- ---- Sets and equips ----

local function find_set(name)
  local sets = prof and prof.sec.sets
  if not sets then return nil end
  local want = tostring(name):lower()
  for _, c in ipairs(sets.children) do
    local n = xml.get(c, "name")
    if n and n:lower() == want then return c end
  end
end

-- Equipment is not read while a gear lock or a pet override holds.
local function gated()
  local now = clock()
  return now < gear_lock or (pet_override and now < pet_hold)
end

local function put_slot(ctx, el, dlock, dprio)
  local s = SLOT[el.name:lower()]
  if not s then return end
  local lv, pv = xml.get(el, "lock"), xml.get(el, "priority")
  local lock = lv ~= nil and lv:lower() == "true" or (lv == nil and dlock)
  local prio = pv ~= nil and atoi(pv) or dprio
  local cur = ctx.wanted[s]
  if cur and cur.lock and not lock then return end
  local aug = xml.get(el, "augment")
  ctx.wanted[s] = { text = resolve(el.text), lock = lock, priority = prio, augment = aug and resolve(aug),
    path = xml.get(el, "augpath"), rank = xml.get(el, "augrank"), trial = xml.get(el, "augtrial") }
end

local function put_set(ctx, name, dlock, dprio, seen)
  local set = find_set(name)
  if not set then return end
  seen = seen or {}
  if seen[set] then
    warn("set " .. tostring(name) .. " is its baseset")
    return
  end
  seen[set] = true
  local base = xml.get(set, "baseset")
  if base then put_set(ctx, base, dlock, dprio, seen) end
  if gated() then return end
  for _, c in ipairs(set.children) do
    if c.name ~= "" then put_slot(ctx, c, dlock, dprio) end
  end
end

local function equip_node(ctx, n)
  local lv, pv = xml.get(n, "lock"), xml.get(n, "priority")
  local dlock, dprio = lv ~= nil and lv:lower() == "true", pv ~= nil and atoi(pv) or 0
  local set = xml.get(n, "set")
  if set then put_set(ctx, resolve(set), dlock, dprio) end
  if gated() then return end
  for _, c in ipairs(n.children) do
    if c.name ~= "" then put_slot(ctx, c, dlock, dprio) end
  end
end

-- The bits of 24 bytes of extdata, least significant first.
local function bits(ext, from, count)
  local v = 0
  for k = 0, count - 1 do
    local n = from + k
    local byte = ext:byte(n // 8 + 1) or 0
    if byte & (1 << (n % 8)) ~= 0 then v = v | (1 << k) end
  end
  return v
end

-- An item's augment path, rank and Magian trial, as Ashitacast reads them.
local function aug_meta(ext)
  if not ext or #ext < 12 then return 0, 0, 0 end
  local kind, flags = ext:byte(1), ext:byte(2)
  if kind ~= 2 and kind ~= 3 then return 0, 0, 0 end
  if flags & 0x08 ~= 0 then return 0, 0, 0 end
  if flags & 0x20 ~= 0 then return bits(ext, 16, 2) + 1, bits(ext, 18, 4), 0 end
  if flags == 0x83 then return bits(ext, 32, 2) + 1, bits(ext, 50, 5), 0 end
  if flags & 0x80 ~= 0 then return 0, 0, 0 end
  if flags & 0x40 ~= 0 then return 0, 0, bits(ext, 80, 15) end
  return 0, 0, 0
end

local PATHS = { A = 1, B = 2, C = 3, D = 4 }

local function want_of(w)
  local l = w.text:lower()
  if l == "remove" then return { remove = true, priority = w.priority } end
  local augs
  if w.augment then augs = split(w.augment, ",") end
  local path, rank, trial = PATHS[w.path or ""], w.rank and atoi(w.rank), w.trial and atoi(w.trial)
  local check
  if path or rank or trial then
    check = function(ext)
      local p, r, t = aug_meta(ext)
      return (not path or p == path) and (not rank or r == rank) and (not trial or t == trial)
    end
  end
  return { name = w.text, augments = augs, priority = w.priority, test = check }
end

local function send(ctx, mode, label)
  if mode == "none" or not next(ctx.wanted) then return end
  if act and act.cancel then return end
  local wants = {}
  for s = 0, 15 do
    local w = ctx.wanted[s]
    local l = w and w.text:lower()
    if w and l ~= "" and l ~= "ignore" and not disabled[s] then
      if l == "displaced" then
        core.sent[s] = { bag = 0, index = 0, at = core.H.now() }
      else
        wants[s] = want_of(w)
      end
    end
  end
  if not next(wants) then return end
  local list = core.resolve(wants, { log_names = false, bags = prof.cfg.equipbags })
  if debug and #list > 0 then
    local t = {}
    for _, c in ipairs(list) do
      local r = c.id and core.record(c.id)
      t[#t + 1] = SLOTS[c.slot + 1] .. ": " .. (r and r.name or "remove")
    end
    say(label .. ": " .. table.concat(t, ", "))
  end
  core.send(core.packets(list, mode, 8))
end

-- ---- Flow ----

local crawl

local function run_line(text, mode, delay)
  local function go()
    if mode >= 0 and text:sub(1, 2) ~= "//" then through[text] = (through[text] or 0) + 1 end
    depth = depth + 1
    if depth <= 8 then commands.run(text) else warn("commands calling commands too deep: " .. text) end
    depth = depth - 1
  end
  if delay then timer.after(math.max(delay, 1) / 1000, go) else go() end
end

local function buff_id(n)
  local id
  local name = xml.get(n, "name")
  if name then
    for _, b in ipairs(buffs_named()) do
      if (res.status(b.id) or "") == name then id = b.id break end
    end
  end
  local iv = xml.get(n, "id")
  if iv and atoi(iv) >= 0 and atoi(iv) <= 999 then id = atoi(iv) end
  return id
end

local function change(n)
  if not (act and act.active and not act.post and (act.type == "Spell" or act.type == "Ability" or act.type == "Weaponskill")) then
    warn("Invalid use of change tag.")
    return
  end
  index_names()
  local name, idv = xml.get(n, "name"), xml.get(n, "id")
  local new
  if name then
    local l = resolve(name):lower()
    if act.type == "Spell" then
      new = spell_ids[l]
    else
      local id = ability_ids[l]
      if act.type == "Ability" and id and id >= 0x200 and id < 0x400 then new = id - 0x200 end
      if act.type == "Weaponskill" and id and id < 0x200 then new = id end
    end
    if not new then warn("change: no " .. act.type:lower() .. " named " .. name) end
  end
  if idv then
    local id = atoi(idv)
    local ok = (act.type == "Spell" and res.spell(id)) or (act.type == "Ability" and res.ability(id + 0x200))
      or (act.type == "Weaponskill" and res.ability(id))
    if ok then new = id else warn("change: no " .. act.type:lower() .. " with id " .. idv) end
  end
  if new then act.id = new end
end

local ACTIONS = {
  command = function(n)
    local mode = xml.get(n, "mode")
    local delay = xml.get(n, "delay")
    run_line(resolve(n.text), mode and atoi(mode) or -1, delay and atoi(delay))
  end,
  echo = function(n)
    local text = resolve(n.text)
    local c = xml.get(n, "color")
    if c then
      local v = atoi(c)
      if v >= 0 and v <= 255 then
        text = "\x1E" .. string.char(math.max(v, 1)) .. text .. "\x1E\x01"
      elseif v >= 256 and v <= 511 then
        text = "\x1F" .. string.char(math.max(v & 0xFF, 1)) .. text .. "\x1E\x01"
      end
    end
    local mode = atoi(xml.get(n, "mode") or "0")
    local delay = xml.get(n, "delay")
    local function go() chat.add(text, math.max(0, math.min(mode, 255))) end
    if delay then timer.after(math.max(atoi(delay), 1) / 1000, go) else go() end
  end,
  setvar = function(n)
    local name, value = xml.get(n, "name"), xml.get(n, "value")
    if name and value then uvars[name] = setvar_value(value) end
  end,
  clearvar = function(n)
    local all = xml.get(n, "all")
    if all and all:lower() == "true" then
      uvars = {}
    elseif xml.get(n, "name") then
      uvars[xml.get(n, "name")] = nil
    end
  end,
  change = change,
  gearlock = function(n)
    local l = xml.get(n, "length")
    if not l then return end
    local ms = atoi(l)
    if ms >= 0 then gear_lock = clock() + math.min(ms, 3600000) / 1000 end
  end,
  registerbuff = function(n)
    local id = buff_id(n)
    if not id then return end
    local a = xml.get(n, "active")
    for i = #overrides, 1, -1 do
      if overrides[i].id == id then table.remove(overrides, i) end
    end
    overrides[#overrides + 1] = { id = id, active = not (a and a:lower() == "false"), ends = clock() + 2 }
  end,
  clearbuff = function(n)
    local id = buff_id(n)
    for i = #overrides, 1, -1 do
      if overrides[i].id == id then table.remove(overrides, i) end
    end
  end,
  cancel = function()
    if act and (act.type:sub(1, 3) == "Pet" or (act.active and not act.post)) then
      act.cancel = true
    else
      warn("Invalid use of cancel tag.")
    end
  end,
  override = function()
    if act and act.type:sub(1, 3) == "Pet" then pet_override = true else warn("Invalid use of override tag.") end
  end,
}

-- Walks a section's (or a body's) children; true when a <return> stopped the section.
crawl = function(parent, ctx)
  local chain, after_if = false, false
  for _, n in ipairs(parent.children) do
    local nm = n.name:lower()
    if nm == "if" then
      after_if = true
      if test(n) then
        chain = false
        if crawl(n, ctx) then return true end
      else
        chain = true
      end
    elseif nm == "elseif" then
      if not after_if then warn("Elseif node must follow an if or elseif node.") end
      after_if = true
      if chain and test(n) then
        chain = false
        if crawl(n, ctx) then return true end
      end
    elseif nm == "else" then
      if not after_if then warn("Else node must follow an if or elseif node.") end
      after_if = false
      if chain then
        chain = false
        if crawl(n, ctx) then return true end
      end
    elseif nm == "return" then
      return true
    else
      chain, after_if = false, false
      if nm == "equip" then
        equip_node(ctx, n)
      elseif ACTIONS[nm] then
        ACTIONS[nm](n)
      else
        warn("Node not recognized. [" .. (n.name == "" and xml.trim(n.text) or n.name) .. "]")
      end
    end
  end
end

-- Runs section `name` and sends its equips the section's way.
local function run(name)
  local sec = prof and prof.sec[name]
  if not sec then return end
  local ctx = { wanted = {} }
  crawl(sec, ctx)
  send(ctx, MODE[name] or "none", name)
end

-- ---- v3 files: Ashitacast's import rules ----

local V3_SECTIONS = { sets = "sets", inputcommands = "commands", idlegear = "default", preranged = "preranged",
  midranged = "midranged", premagic = "precast", midmagic = "midcast", jobability = "ability", weaponskill = "weaponskill",
  petskill = "petskill", petspell = "petspell" }
local V3_SLOTS = { lear = "ear1", rear = "ear2", lring = "ring1", rring = "ring2" }
-- v3's built-in names that v4 spells otherwise.
local V3_VARS = { p_joblevel = "p_level", eq_lear = "eq_ear1", eq_rear = "eq_ear2", eq_lring = "eq_ring1", eq_rring = "eq_ring2" }

local function v3_text(s)
  s = s:gsub("%$", "$u_")
  return (s:gsub("%%([%w_]*)", function(n) return "$" .. (V3_VARS[n] or n) end))
end

local function el(name, attrs, children, text)
  local n = { name = name, attrs = attrs or {}, attr = {}, children = children or {}, text = text or "" }
  for _, a in ipairs(n.attrs) do
    if n.attr[a.name:lower()] == nil then n.attr[a.name:lower()] = a.value end
  end
  return n
end

-- buffactive="Haste|!Slow x2" as b_ rules (one piece) or an advanced rule (several).
local function v3_buffs(value)
  local pieces = {}
  for op, piece in ("|" .. value):gmatch("([|&])([^|&]*)") do pieces[#pieces + 1] = { op, piece } end
  local rules = {}
  for k, p in ipairs(pieces) do
    local text = xml.trim(p[2])
    local neg = text:sub(1, 1) == "!"
    if neg then text = text:sub(2) end
    local name, count = text:match("^(.-)%s*x(%d+)$")
    if not name then name, count = text, "1" end
    rules[k] = { op = p[1], name = name, cmp = (neg and "<" or ">=") .. count }
  end
  if #rules == 1 then return { { name = "b_" .. rules[1].name, value = rules[1].cmp } } end
  local t = {}
  for k, r in ipairs(rules) do t[#t + 1] = (k > 1 and r.op or "") .. "($b_" .. r.name .. r.cmp .. ")" end
  return { { name = "advanced", value = table.concat(t) } }
end

local v3_node

local function v3_body(n)
  local out = {}
  for _, c in ipairs(n.children) do
    local x = v3_node(c)
    if x then out[#out + 1] = x end
  end
  return out
end

local function v3_slots(n)
  local out = {}
  for _, c in ipairs(n.children) do
    local s = c.name:lower()
    s = V3_SLOTS[s] or s
    if SLOT[s] then
      local attrs = {}
      for _, a in ipairs(c.attrs) do attrs[#attrs + 1] = { name = a.name, value = a.value } end
      local t = c.text
      if t:find("[%$%%]") then t = v3_text(t) end
      out[#out + 1] = el(s, attrs, {}, t)
    end
  end
  return out
end

v3_node = function(n)
  local nm = n.name:lower()
  local g = function(k) return xml.get(n, k) end
  if nm == "if" or nm == "elseif" then
    local attrs = {}
    for _, a in ipairs(n.attrs) do
      if a.name:lower() == "buffactive" then
        for _, r in ipairs(v3_buffs(a.value)) do attrs[#attrs + 1] = r end
      elseif a.name:lower() == "mode" then
        attrs[#attrs + 1] = { name = "mode", value = a.value }
      else
        attrs[#attrs + 1] = { name = a.name, value = v3_text(a.value) }
      end
    end
    return el(nm, attrs, v3_body(n))
  elseif nm == "else" then
    return el("else", {}, v3_body(n))
  elseif nm == "command" then
    return el("command", g("delay") and { { name = "delay", value = g("delay") } }, {}, v3_text(n.text))
  elseif nm == "clearvars" then
    return el("clearvar", { { name = "all", value = "true" } })
  elseif nm == "setvar" then
    return el("setvar", { { name = "name", value = g("name") or "" }, { name = "value", value = g("value") or "" } })
  elseif nm == "incvar" or nm == "decvar" then
    local name = g("name")
    if not name then return nil end
    local v = g("value") or "1"
    return el("setvar", { { name = "name", value = name }, { name = "value", value = "$u_" .. name .. (nm == "incvar" and " + " or " - ") .. v } })
  elseif nm == "change" then
    if g("type") or (not g("id") and not g("name")) then return nil end
    local attrs = {}
    if g("id") then attrs[#attrs + 1] = { name = "id", value = g("id") } end
    if g("name") then attrs[#attrs + 1] = { name = "name", value = g("name") } end
    return el("change", attrs)
  elseif nm == "addtochat" then
    return el("echo", { { name = "mode", value = g("color") or "0" } }, {}, v3_text(n.text))
  elseif nm == "addtoecho" then
    return el("echo", g("color") and { { name = "color", value = g("color") } }, {}, v3_text(n.text))
  elseif nm == "gearlock" then
    local s = g("length")
    return el("gearlock", s and { { name = "length", value = tostring(math.floor(atof(s) * 1000)) } })
  elseif nm == "cancel" or nm == "return" then
    return el(nm)
  elseif nm == "registerbuff" or nm == "clearbuff" then
    if not g("id") then return nil end
    local attrs = { { name = "id", value = g("id") } }
    if nm == "registerbuff" and g("active") then attrs[2] = { name = "active", value = g("active") } end
    return el(nm, attrs)
  elseif nm == "equip" then
    local attrs = {}
    if g("set") then attrs[#attrs + 1] = { name = "set", value = v3_text(g("set")) } end
    if g("priority") then attrs[#attrs + 1] = { name = "priority", value = g("priority") } end
    if g("lock") then attrs[#attrs + 1] = { name = "lock", value = g("lock") } end
    return el("equip", attrs, v3_slots(n))
  end
  -- doidlegear and anything unknown: dropped, as the importer drops them.
  return nil
end

local function is_v3(root)
  for _, c in ipairs(root.children) do
    if c.name == "idlegear" or c.name == "premagic" or c.name == "midmagic" or c.name == "jobability"
      or c.name == "inputcommands" or c.name == "variables" or c.name == "init" then return true end
  end
  return false
end

local function from_v3(root)
  local out = el("ashitacast")
  local load = el("load")
  local first = {}
  for _, c in ipairs(root.children) do
    if not first[c.name] then
      first[c.name] = true
      local to = V3_SECTIONS[c.name]
      if c.name == "sets" then
        local sets = el("sets")
        for _, s in ipairs(c.children) do
          if s.name:lower() == "set" and xml.get(s, "name") then
            local attrs = { { name = "name", value = xml.get(s, "name") } }
            if xml.get(s, "baseset") then attrs[2] = { name = "baseset", value = xml.get(s, "baseset") } end
            sets.children[#sets.children + 1] = el("set", attrs, v3_slots(s))
          end
        end
        out.children[#out.children + 1] = sets
      elseif c.name == "inputcommands" then
        local cmds = el("commands")
        for _, cmd in ipairs(c.children) do
          if cmd.name:lower() == "cmd" and xml.get(cmd, "input") then
            cmds.children[#cmds.children + 1] = el("command", { { name = "input", value = xml.get(cmd, "input") } }, v3_body(cmd))
          end
        end
        out.children[#out.children + 1] = cmds
      elseif c.name == "variables" then
        for _, v in ipairs(c.children) do
          if v.name:lower() == "var" and xml.get(v, "name") then
            table.insert(load.children, 1, el("setvar", { { name = "name", value = xml.get(v, "name") }, { name = "value", value = v.text } }))
          end
        end
      elseif c.name == "init" then
        for _, x in ipairs(v3_body(c)) do load.children[#load.children + 1] = x end
      elseif to then
        out.children[#out.children + 1] = el(to, {}, v3_body(c))
      end
    end
  end
  out.children[#out.children + 1] = load
  return out
end

-- ---- Loading ----

local DEFAULTS = { petskilldelay = 1500, weaponskilldelay = 1500, abilitydelay = 1500, spelloffset = 500,
  petspelloffset = 500, rangeddelay = 10000, rangedoffset = 500, itemoffset = 500, fastcast = 0, snapshot = 0 }

local function read_settings(root)
  local cfg = { equipbags = { 0, 8, 10, 11, 12, 13, 14, 15, 16 } }
  for k, v in pairs(DEFAULTS) do cfg[k] = v end
  local st
  for _, c in ipairs(root.children) do
    if c.name == "settings" then st = c break end
  end
  if not st then return cfg end
  local function val(name)
    for _, c in ipairs(st.children) do
      if c.name == name then return c.text end
    end
  end
  local bags = val("equipbags")
  if bags then
    -- Values above 12 are dropped, as Ashitacast drops them.
    cfg.equipbags = {}
    for _, p in ipairs(split(bags, ",")) do
      local b = atoi(p)
      if b >= 0 and b <= 12 then cfg.equipbags[#cfg.equipbags + 1] = b end
    end
    if #cfg.equipbags == 0 then cfg.equipbags = { 0 } end
  end
  for k in pairs(DEFAULTS) do
    local v = val(k)
    if v then
      local n = atoi(v)
      if k == "fastcast" or k == "snapshot" then cfg[k] = math.max(0, math.min(n, 80)) else cfg[k] = math.max(1, math.min(n, 3600000)) end
    end
  end
  return cfg
end

local function find_file(dir, name)
  local lname = name:lower()
  for _, n in ipairs(fs.list(dir) or {}) do
    if n:lower() == lname then return dir .. n end
  end
end

function M.find()
  local p = me()
  local short = res.job(p.main_job or 0)
  if not short then return nil end
  return find_file("ashitacast/" .. p.name .. "_" .. p.id .. "/", short .. ".xml") or find_file("ashitacast/", p.name .. "_" .. short .. ".xml")
    -- LegacyAC (Ashitacast v3) keeps its files in a separate folder.
    or find_file("legacyac/", p.name .. "_" .. short .. ".xml")
end

local function strip_comments(s)
  while true do
    local a = s:find("<!--", 1, true)
    if not a then return s end
    local b = s:find("-->", a + 4, true)
    if not b then return s end
    s = s:sub(1, a - 1) .. s:sub(b + 3)
  end
end

-- An include's file: as named (in the data folder), else beside the profile.
local function include_text(path, dir)
  path = xml.trim(path)
  local d, b = path:match("^(.-)([^/]+)$")
  local f = find_file(dir .. d, b) or find_file(d, b)
  return f and fs.read(f)
end

local function prepare(text, dir)
  text = strip_comments(text)
  local a = text:find("<ashitacast ", 1, true) or text:find("<ashitacast>", 1, true) or text:find("<ashitacast/", 1, true)
  if not a then error("Could not find <ashitacast> tag", 0) end
  local _, b = text:find("</ashitacast>", a, true)
  text = text:sub(a, b or #text)
  -- Plain includes take the named file's text in their place (bounded: a file can include itself).
  for _ = 1, 64 do
    local s, e, path = text:find("<include>(.-)</include>")
    if not s then break end
    local inc = include_text(path, dir)
    if not inc then error("include not found: " .. xml.trim(path), 0) end
    text = text:sub(1, s - 1) .. inc .. text:sub(e + 1)
  end
  return text
end

-- <include merge="true">: each section of the named file merged into ours, before ours when ours comes after it.
local function merge_includes(root, dir)
  local i = 1
  while i <= #root.children do
    local c = root.children[i]
    local m = xml.get(c, "merge")
    if c.name == "include" and m == "true" then
      local inc = include_text(c.text, dir)
      if not inc then error("include not found: " .. xml.trim(c.text), 0) end
      local doc = xml.parse("<x>" .. strip_comments(inc) .. "</x>").children[1]
      local base = doc
      for _, x in ipairs(doc.children) do
        if x.name == "ashitacast" then base = x end
      end
      table.remove(root.children, i)
      for _, sec in ipairs(base.children) do
        local at
        for k, x in ipairs(root.children) do
          if x.name == sec.name then at = k break end
        end
        if not at then
          table.insert(root.children, 1, sec)
          i = i + 1
        elseif at >= i then
          for k = #sec.children, 1, -1 do table.insert(root.children[at].children, 1, sec.children[k]) end
        else
          for _, x in ipairs(sec.children) do table.insert(root.children[at].children, x) end
        end
      end
    else
      i = i + 1
    end
  end
end

function M.unload()
  if prof then run("unload") end
  prof, M.file = nil, nil
  hold, pet_hold, gear_lock, pet_override = 0, 0, 0, false
end

-- `keep_vars`: a job change keeps the user variables; a load by hand clears them.
function M.load(path, keep_vars)
  M.unload()
  if not keep_vars then uvars = {} end
  warned = {}
  local ok, err = pcall(function()
    local dir = path:match("^(.-)[^/]*$")
    local doc = xml.parse(prepare(fs.read(path) or "", dir))
    local root = doc.children[1]
    if not root or root.name ~= "ashitacast" then error("Could not find <ashitacast> tag", 0) end
    merge_includes(root, dir)
    if is_v3(root) then root = from_v3(root) end
    local sec, count = {}, {}
    for _, c in ipairs(root.children) do
      count[c.name] = (count[c.name] or 0) + 1
      if count[c.name] > 1 and c.name ~= "item" then
        for _, n in ipairs(SECTIONS) do
          if n == c.name then error("Multiple instances of section: " .. n, 0) end
        end
      end
      if not sec[c.name] then sec[c.name] = c end
    end
    prof = { root = root, sec = sec, cfg = read_settings(root), path = path }
  end)
  if not ok then
    say("could not load " .. path .. ": " .. tostring(err))
    prof = nil
    return false
  end
  M.file = path
  debug = false
  say("loaded profile " .. path)
  run("load")
  return true
end

function M.loaded() return prof ~= nil end

-- The item names the profile's sets hold.
function M.gear_names()
  local out, seen = {}, {}
  local sets = prof and prof.sec.sets
  if not sets then return out end
  for _, s in ipairs(sets.children) do
    for _, c in ipairs(s.children) do
      local t = xml.trim(c.text)
      local l = t:lower()
      if SLOT[c.name:lower()] and t ~= "" and not t:find("$", 1, true) and l ~= "ignore" and l ~= "remove"
        and l ~= "displaced" and not seen[l] then
        seen[l] = true
        out[#out + 1] = t
      end
    end
  end
  table.sort(out)
  return out
end

-- ---- Actions ----

local function item_id(a)
  for _, it in ipairs(game.items(a.bag) or {}) do
    if it.slot == a.index then return it.id end
  end
  return 0
end

-- The action's packet with the id a <change> chose.
local function packet_of(a)
  if a.kind == "item" or act.id == a.param then return a.packet end
  return a.packet:sub(1, 12) .. string.pack("<I4", act.id) .. a.packet:sub(17)
end

local function ms(v) return v / 1000 end

function M.action(a)
  if not prof then return end
  local kind
  if a.kind == "item" then
    kind = "Item"
  elseif a.category == 3 then
    kind = "Spell"
  elseif a.category == 7 then
    kind = "Weaponskill"
  elseif a.category == 9 then
    kind = "Ability"
  elseif a.category == 16 then
    kind = "Ranged"
  else
    return
  end
  local cfg = prof.cfg
  act = { type = kind, active = true, post = false, cancel = false, target = a.tindex, id = kind == "Item" and item_id(a) or a.param }
  local now = clock()
  if kind == "Spell" or kind == "Ranged" then
    run(kind == "Spell" and "precast" or "preranged")
    if act.cancel then act.active = false return false end
    if kind == "Spell" then
      local r = res.spell(act.id) or {}
      hold = now + (r.cast_time or 0) * (100 - cfg.fastcast) / 100 + ms(cfg.spelloffset)
    else
      hold = now + ms(cfg.rangeddelay) * (100 - cfg.snapshot) / 100 + ms(cfg.rangedoffset)
    end
    core.H.inject(packet_of(a))
    act.post = true
    run(kind == "Spell" and "midcast" or "midranged")
    act.active = false
  elseif kind == "Item" then
    run("item")
    if act.cancel then return false end
    local r = res.item(act.id)
    hold = now + (r and r.cast_time or 2.8) + ms(cfg.itemoffset)
    core.H.inject(a.packet)
    -- The item's action stays live until the next one, as Ashitacast leaves it.
  else
    run(kind == "Weaponskill" and "weaponskill" or "ability")
    act.active = false
    if act.cancel then return false end
    hold = now + ms(kind == "Weaponskill" and cfg.weaponskilldelay or cfg.abilitydelay)
    core.H.inject(packet_of(a))
  end
  return false
end

local DONE = { [2] = true, [3] = true, [4] = true, [5] = true, [6] = true, [14] = true }
local PET_DONE = { [4] = true, [11] = true, [13] = true }

local function index_of(id)
  if not id then return 0 end
  local e = game.entity(id)
  return e and e.index or 0
end

function M.on_action(a)
  if not prof then return end
  local interrupt = (a.category == 8 or a.category == 12) and a.param == 28787
  if a.actor == me().id then
    if DONE[a.category] or interrupt then hold = 0 end
    return
  end
  local p = pet()
  if not p or a.actor ~= p.id then return end
  if PET_DONE[a.category] or (a.category == 8 and a.param == 28787) then
    pet_override, pet_hold = false, 0
    return
  end
  local t = a.targets and a.targets[1]
  local r = t and t.results and t.results[1]
  if a.category == 7 then
    pet_override, pet_hold = false, 0
    act = { type = (r and r.message == 43) and "PetSkill_MobSkill" or "PetSkill_Ability", active = true, post = false,
      cancel = false, id = r and r.value or 0, target = index_of(t and t.id) }
    run("petskill")
    act.active = false
    if not act.cancel then pet_hold = clock() + ms(prof.cfg.petskilldelay) end
  elseif a.category == 8 then
    local id = (r and r.value or 0) & 0x3FF
    local s = res.spell(id)
    if not s then return end
    pet_override, pet_hold = false, 0
    act = { type = "PetSpell", active = true, post = false, cancel = false, id = id, target = index_of(t and t.id) }
    run("petspell")
    act.active = false
    if not act.cancel then pet_hold = clock() + (s.cast_time or 0) + ms(prof.cfg.petspelloffset) end
  end
end

-- The send tick: `default` while neither we nor the pet are busy.
function M.tick()
  if not prof then return end
  local p = me()
  local pos = p.pos and (p.pos.x .. "," .. p.pos.z)
  moving = last_pos ~= nil and pos ~= last_pos
  local now = clock()
  if now >= hold and now >= pet_hold then
    if act then act.cancel, act.post = false, true end
    run("default")
  end
  last_pos = pos
end

-- ---- <commands> ----

-- A typed line a <commands> child takes (the shortest matching input wins): false when taken.
function M.typed(line)
  if not prof then return nil end
  if through[line] then
    through[line] = through[line] > 1 and through[line] - 1 or nil
    return nil
  end
  local cmds = prof.sec.commands
  if not cmds then return nil end
  local best, best_input
  local l = line:lower()
  for _, c in ipairs(cmds.children) do
    local input = xml.get(c, "input")
    if input and l:sub(1, #input) == input:lower() and (not best or #input < #best_input) then
      best, best_input = c, input
    end
  end
  if not best then return nil end
  local t = compat.tokens(line)
  local a = { text = (line:sub(#best_input + 1):gsub("^ +", "")) }
  for i, w in ipairs(t) do a[i - 1] = w end
  local prev = args
  args = a
  local ctx = { wanted = {} }
  crawl(best, ctx)
  args = prev
  return false
end

-- ---- /ac ----

local function set_now(name, secs)
  gear_lock = 0
  local ctx = { wanted = {} }
  put_set(ctx, name, false, 0)
  send(ctx, "set", "/ac set")
  gear_lock = clock() + secs
end

function M.command(t)
  local cmd = (t[1] or ""):lower()
  if cmd == "load" then
    local p = me()
    local f
    if t[2] then
      local n = t[2]:lower():sub(-4) == ".xml" and t[2] or t[2] .. ".xml"
      f = find_file("ashitacast/" .. p.name .. "_" .. p.id .. "/", n) or find_file("ashitacast/", n)
    else
      f = M.find()
    end
    if f then M.load(f) else say("profile not found") end
  elseif cmd == "reload" then
    if M.file then M.load(M.file) end
  elseif cmd == "unload" then
    M.unload()
    uvars = {}
    say("profile unloaded")
  elseif not prof then
    say("no XML profile loaded")
  elseif cmd == "set" and t[2] then
    local msv = tonumber(t[3] or "5000") or 5000
    if msv < 1 then say("/ac set: the duration must be at least 1") return end
    set_now(t[2], math.min(msv, 3600000) / 1000)
  elseif cmd == "disable" or cmd == "enable" then
    local s = t[2] and SLOT[t[2]:lower()]
    for i = 0, 15 do
      if not t[2] or i == s then disabled[i] = cmd == "disable" or nil end
    end
  elseif cmd == "naked" then
    local list = {}
    for s, w in pairs(core.worn()) do list[#list + 1] = { slot = s, bag = w.bag, index = 0, priority = 0 } end
    table.sort(list, function(x, y) return x.slot < y.slot end)
    core.send(core.packets(list, "single"))
    for i = 0, 15 do disabled[i] = true end
  elseif cmd == "setvar" and t[2] then
    uvars[t[2]] = resolve(table.concat(t, " ", 3))
  elseif cmd == "clearvar" and t[2] then
    uvars[t[2]] = nil
  elseif cmd == "clearvars" then
    uvars = {}
  elseif t[1] == "eval" then
    run_line(resolve(table.concat(t, " ", 2)), 0)
  elseif cmd == "debug" then
    debug = t[2] == nil and not debug or (t[2] or ""):lower() == "on"
    say("debug " .. (debug and "on" or "off"))
  elseif cmd == "print" and (t[2] or ""):lower() == "uvars" then
    local keys = {}
    for k in pairs(uvars) do keys[#keys + 1] = k end
    table.sort(keys)
    for _, k in ipairs(keys) do say("$u_" .. k .. " = " .. uvars[k]) end
  elseif cmd == "print" then
    table.sort(BUILTIN_NAMES)
    for _, k in ipairs(BUILTIN_NAMES) do say("$" .. k .. " = " .. BUILTIN[k]()) end
  else
    say("/ac " .. cmd .. ": not supported here")
  end
end

-- For tests: the evaluators alone.
M._resolve, M._basic, M._advanced, M._setvar = resolve, basic, advanced, setvar_value
M._uvars = function(t) if t then uvars = t end return uvars end
M._args = function(a) args = a end
M._aug_meta = aug_meta

return M
