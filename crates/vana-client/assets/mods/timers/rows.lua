-- The Timers' rows: our recasts, the effects on us and on our party and others, the ones we put on monsters, and the
-- other mods' rows (timers.add). Each row: {icon, name, left, total, fills}.

local E = require("engine")

local R = {}

local now = os.clock

-- The longest time left seen per key (the gauge's whole length); a jump up is a new instance.
local totals = {}

local function total(key, left, seen)
  seen[key] = true
  local t = totals[key]
  if not t or left > t.last + 1 then
    t = { total = left, last = left }
    totals[key] = t
  end
  t.total = math.max(t.total, left)
  t.last = left
  return t.total
end

-- Ability records by recast id, in id order (job abilities and pet commands).
local by_timer
local function abilities_on(timer)
  if not by_timer then
    by_timer = {}
    local any = false
    for id = 0x200, 0x7ff do
      local a = res.ability(id)
      if a then
        any = true
        by_timer[a.recast_id] = by_timer[a.recast_id] or {}
        table.insert(by_timer[a.recast_id], a)
      end
    end
    if not any then
      by_timer = nil
      return {}
    end
  end
  return by_timer[timer] or {}
end

-- The ability on a recast timer: the one we know (pet commands always); a sub-list's row (Ready, Quick Draw,
-- Stratagems) names the timer over its moves.
local function ability_for_timer(timer, known)
  local found
  for _, a in ipairs(abilities_on(timer)) do
    if a.id >= 0x400 or known[a.id] then
      if a.sub ~= 0 then return a end
      found = found or a
    end
  end
  return found
end

-- The charge abilities by their moves' kind (9 Quick Draw, 0xf Stratagems, 0x12 Ready): the charges at our job's
-- level and the full recast in seconds; none when the job does not give them (retail's rule). The client has the same
-- rule for /recast (client/timers.rs, charge_rule and charges): change both together.
function R.charge_rule(kind, mj, ml, sj, sl)
  local n, full
  if kind == 9 then
    local lv = mj == 0x11 and ml or sj == 0x11 and sl or nil
    n, full = lv and (lv < 0x28 and 0 or 2) or 0, 0x78
  elseif kind == 0xf then
    local lv = mj == 0x14 and ml or sj == 0x14 and sl or nil
    n, full = lv and ((lv - 10) // 0x14 + 1) or 0, 0xf0
  elseif kind == 0x12 then
    -- Retail tests the sub job against 0x14 here, not 9 (kept).
    local lv = mj == 9 and ml or sj == 0x14 and sl or nil
    n, full = lv and (lv < 0x19 and 0 or 3) or 0, 0x5a
  else
    return nil
  end
  return n, full
end

-- With charges: the full recast adjusted by the entry's calc2 (added) and calc1 (sixtieths off), then the seconds to
-- the next charge, the charges ready and a charge's share.
function R.charges(left, n, full, calc1, calc2)
  if n <= 0 then return nil end
  local function trunc(x) return x < 0 and -math.floor(-x) or math.floor(x) end
  full = full + calc2
  local whole = full - (calc1 & 0x3f) * (1 / 60) * full
  local per = trunc(whole / n)
  whole = trunc(whole)
  if per <= 0 or trunc(whole / n) <= 0 then return nil end
  local nxt = math.fmod(left, per)
  if nxt == 0 and left ~= 0 then nxt = per end
  return nxt, trunc((whole - left) / trunc(whole / n)), per
end

local function recast_rows(seen)
  local rows = {}
  local r = game.recasts()
  local p = game.player()
  local known = {}
  for _, id in ipairs(game.abilities()) do known[id] = true end
  for timer, left in pairs(r.abilities) do
    local a = left > 0 and ability_for_timer(timer, known)
    if a then
      local name = a.name or ""
      local calc = r.calc and r.calc[timer] or { calc1 = 0, calc2 = 0 }
      local n, full = R.charge_rule(a.sub ~= 0 and a.sub or a.kind, p.main_job, p.main_level, p.sub_job, p.sub_level)
      local up = math.ceil(left)
      local nxt, ready, per
      if n then nxt, ready, per = R.charges(up, n, full, calc.calc1, calc.calc2) end
      local row = { icon = { kind = "ability", id = a.id }, name = name, fills = true }
      if nxt then
        row.name = ("%s (%d)"):format(name, ready)
        -- The share's fraction of a second carries over from the whole.
        row.left, row.total = nxt - (up - left), per
      else
        row.left, row.total = left, total("a" .. timer, left, seen)
      end
      rows[#rows + 1] = row
    end
  end
  for spell, left in pairs(r.spells) do
    if left > 0 then
      local s = res.spell(spell)
      rows[#rows + 1] = { icon = { kind = "spell", id = spell }, name = s and s.name or "", left = left,
        total = total("s" .. spell, left, seen), fills = true }
    end
  end
  table.sort(rows, function(a, b)
    if a.left ~= b.left then return a.left < b.left end
    return a.name < b.name
  end)
  return rows
end

-- Our effects' gauge lengths: per status, each copy by its end (songs, maneuvers and runes keep several of one
-- status). A copy whose end moved by more than a second is a new one.
local copies = {}

local function copy_total(id, left, claimed)
  local ends = now() + left
  local list = copies[id] or {}
  copies[id] = list
  for _, c in ipairs(list) do
    if not claimed[c] and math.abs(c.ends - ends) <= 1 then
      claimed[c] = true
      c.ends, c.total = ends, math.max(c.total, left)
      return c.total
    end
  end
  local c = { ends = ends, total = left }
  list[#list + 1] = c
  claimed[c] = true
  return left
end

-- Our effects with end times, shortest first, named by what we cast when it was ours.
local function own_rows()
  local names = {}
  for _, e in ipairs(E.effects_on(game.player().id)) do
    if e.name ~= "" then names[e.status] = e.name end
  end
  local rows, claimed = {}, {}
  for _, b in ipairs(game.buffs()) do
    local info = b.remaining and res.status_info(b.id)
    if b.remaining and b.remaining > 0 and not (info and info.untimed) then
      rows[#rows + 1] = { icon = { kind = "status", id = b.id }, name = names[b.id] or res.status(b.id) or "",
        left = b.remaining, total = copy_total(b.id, b.remaining, claimed), fills = false }
    end
  end
  for id, list in pairs(copies) do
    for i = #list, 1, -1 do
      if not claimed[list[i]] then table.remove(list, i) end
    end
    if #list == 0 then copies[id] = nil end
  end
  table.sort(rows, function(a, b)
    if a.left ~= b.left then return a.left < b.left end
    return a.icon.id < b.icon.id
  end)
  return rows
end

local function applied_rows(id)
  local rows = {}
  for _, e in ipairs(E.effects_on(id)) do
    rows[#rows + 1] = { icon = { kind = "status", id = e.status }, name = e.name, left = e.left or 0,
      total = e.secs or 0, fills = false }
  end
  table.sort(rows, function(a, b)
    if a.left ~= b.left then return a.left < b.left end
    return a.icon.id < b.icon.id
  end)
  return rows
end

-- The other mods' rows (timers.add): {mod, section 1..3, group, icon, name, ends, total, fills}.
R.mod_timers = {}

local SECTION = { recast = 1, recasts = 1, debuff = 3, debuffs = 3 }

function R.add(mod_id, name, seconds, options)
  options = options or {}
  local section = SECTION[options.section] or 2
  local icon = options.icon
  if icon ~= nil then
    if type(icon) ~= "table" or type(icon.id) ~= "number" then error("icon needs a numeric id", 0) end
    icon = { kind = icon.kind or "status", id = icon.id }
  end
  local secs = math.max(tonumber(seconds) or 0, 0)
  R.remove(mod_id, name)
  local fills = options.fills
  if fills == nil then fills = section == 1 end
  local t = tonumber(options.total) or 0
  table.insert(R.mod_timers, { mod = mod_id, section = section, group = options.group, icon = icon,
    name = tostring(name), ends = now() + secs, total = t > 0 and t or secs, fills = fills })
end

function R.remove(mod_id, name)
  name = tostring(name)
  for i = #R.mod_timers, 1, -1 do
    local t = R.mod_timers[i]
    if t.mod == mod_id and t.name == name then table.remove(R.mod_timers, i) end
  end
end

function R.clear(mod_id)
  for i = #R.mod_timers, 1, -1 do
    if R.mod_timers[i].mod == mod_id then table.remove(R.mod_timers, i) end
  end
end

local function mod_rows(secs)
  local loaded = {}
  for _, m in ipairs(mods.list()) do loaded[m.id:lower()] = true end
  local t = now()
  for i = #R.mod_timers, 1, -1 do
    local m = R.mod_timers[i]
    if not loaded[m.mod:lower()] or t - m.ends >= 1 then table.remove(R.mod_timers, i) end
  end
  for _, m in ipairs(R.mod_timers) do
    local row = { icon = m.icon, name = m.name, left = math.max(m.ends - t, 0), total = m.total, fills = m.fills }
    local sec = secs[m.section]
    local name = m.group or (m.section > 1 and "Timers" or nil)
    local into
    for _, g in ipairs(sec) do
      if g.name == name then into = g end
    end
    if into then
      into.rows[#into.rows + 1] = row
      table.sort(into.rows, function(a, b) return a.left < b.left end)
    else
      sec[#sec + 1] = { name = name, rows = { row } }
    end
  end
end

-- The three sections' groups: recasts; us, then others by party order, then the rest; monsters, ours targeted first.
-- `builtin`: with our tracking's rows, else only the other mods'.
function R.sections(builtin)
  local secs = { {}, {}, {} }
  if builtin then
    local seen = {}
    local recasts = recast_rows(seen)
    local own = own_rows()
    for k in pairs(totals) do
      if not seen[k] then totals[k] = nil end
    end
    if #recasts > 0 then secs[1][1] = { rows = recasts } end
    local p = game.player()
    if #own > 0 then secs[2][1] = { name = p.name, rows = own } end
    local order = {}
    for k, m in ipairs(game.party()) do order[m.id] = k end
    local people, mobs = {}, {}
    for id, b in pairs(E.on) do
      if id ~= p.id then
        if b.mob then mobs[#mobs + 1] = { id, b } else people[#people + 1] = { id, b } end
      end
    end
    table.sort(people, function(x, y)
      local a, b = order[x[1]] or math.huge, order[y[1]] or math.huge
      if a ~= b then return a < b end
      return x[2].name < y[2].name
    end)
    for _, x in ipairs(people) do
      local rows = applied_rows(x[1])
      if #rows > 0 then secs[2][#secs[2] + 1] = { name = x[2].name, rows = rows } end
    end
    local function latest(b)
      local t = -math.huge
      for _, e in ipairs(b.effects) do t = math.max(t, e.start) end
      return t
    end
    table.sort(mobs, function(x, y)
      local tx, ty = x[1] == p.target, y[1] == p.target
      if tx ~= ty then return tx end
      local lx, ly = latest(x[2]), latest(y[2])
      if lx ~= ly then return lx > ly end
      if x[2].name ~= y[2].name then return x[2].name < y[2].name end
      return x[1] < y[1]
    end)
    for _, x in ipairs(mobs) do
      local rows = applied_rows(x[1])
      if #rows > 0 then secs[3][#secs[3] + 1] = { name = x[2].name, rows = rows } end
    end
  end
  mod_rows(secs)
  return secs
end

return R
