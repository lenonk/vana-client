-- How long the effects we put on others last, as LandSandBoat reckons them (no client knows; data/*.tsv made from
-- LSB's scripts and data): which action result lands which effect, its length from our job, levels, merits, job
-- points, gear and effects, and what it replaces. Resist rolls and random terms are unknown: those lengths are upper
-- bounds, cut short by the server's wear-off message.

local M = {}

local function lines(path)
  local out = {}
  for line in (fs.read(path) or ""):gmatch("[^\n]+") do
    if line:sub(1, 1) ~= "#" then
      local f = {}
      for field in (line .. "\t"):gmatch("([^\t]*)\t") do f[#f + 1] = field end
      out[#out + 1] = f
    end
  end
  return out
end

-- rows[kind][id] = { row... }; kind "spell", "ability", "ws" or "pet".
local rows, rules, gear, augments, merits, jps

local function keep(...) rows, rules, gear, augments, merits, jps = ... end

-- Built into locals and kept only once whole: a load cut short (the time budget) is tried again, never half used.
local function load()
  if rows then return end
  local rows, rules, gear, augments, merits, jps = { spell = {}, ability = {}, ws = {}, pet = {} }, {}, {}, {}, {}, {}
  for _, f in ipairs(lines("data/actions.tsv")) do
    local kind, id = f[1], tonumber(f[2])
    if #f >= 11 and rows[kind] and id then
      local base
      local lo, hi = f[7]:match("^([%d.]+)%-([%d.]+)$")
      if f[7] == "-" then
        base = 0
      elseif hi then
        base = tonumber(hi) or 0 -- a random length: the longest is predicted
      else
        base = tonumber(f[7]) or 0
      end
      local toks, mods, jp, status, lvl = {}, {}, {}, {}, nil
      for t in f[9]:gmatch("[^,]+") do
        local k, v = t:match("^([^:]+):?(.*)$")
        v = tonumber(v)
        toks[#toks + 1] = { k, v }
        if k == "mod" and v then mods[#mods + 1] = v end
        if k == "jp" and v then jp[#jp + 1] = v end
        if k == "status" and v then status[v] = true end
        if k == "lvl" and v and not lvl then lvl = v end
      end
      local landed = {}
      for m in f[6]:gmatch("[^,]+") do
        if tonumber(m) then landed[tonumber(m)] = true end
      end
      local list = rows[kind][id] or {}
      rows[kind][id] = list
      list[#list + 1] = {
        id = id, target = f[3], effect = tonumber(f[4]) or 0, param = f[5]:sub(1, 1), landed = landed, base = base,
        random = f[8] == "ENH_RANDOM", formula = f[8], mods = mods, jps = jp, statuses = status, lvl = lvl,
        toks = toks, know = f[10]:sub(1, 1), tier = tonumber(f[11]) or 0,
      }
    end
  end
  for _, f in ipairs(lines("data/status.tsv")) do
    local id = tonumber(f[1])
    if #f >= 9 and id then
      rules[id] = {
        flags = f[2], overwrite = f[3], exclusion = f[4] ~= "-" and f[4] or nil, negative = tonumber(f[5]),
        remove = tonumber(f[7]), min = tonumber(f[8]) or 0,
      }
    end
  end
  for _, f in ipairs(lines("data/gear.tsv")) do
    local i, m, v = tonumber(f[1]), tonumber(f[2]), tonumber(f[3])
    if #f >= 5 and i and m and v then
      local latent = f[5] == "dynamis" and 1 or f[5] == "assault" and 2 or 0
      gear[i] = gear[i] or {}
      table.insert(gear[i], { m, v, tonumber(f[4]) or 0, latent })
    end
  end
  for _, f in ipairs(lines("data/augments.tsv")) do
    local a, k, m, v = tonumber(f[1]), tonumber(f[2]), tonumber(f[3]), tonumber(f[4])
    if #f >= 4 and a and k and m and v then
      augments[a] = augments[a] or {}
      table.insert(augments[a], { k, m, v })
    end
  end
  for _, f in ipairs(lines("data/merits.tsv")) do
    local id, v, jobs = tonumber(f[1]), tonumber(f[2]), tonumber(f[3])
    if #f >= 4 and id and v and jobs then merits[id] = { v, jobs, f[4] == "1" } end
  end
  for _, f in ipairs(lines("data/jobpoints.tsv")) do
    local id, job = tonumber(f[1]), tonumber(f[2])
    if id and job then jps[id] = job end
  end
  keep(rows, rules, gear, augments, merits, jps)
end

function M.rows(kind, id)
  load()
  return rows[kind][id]
end

function M.rule(status)
  load()
  return rules[status]
end

-- Dynamis and Assault zones for the two latent rows (Millennium Horn, Storm Fife; the Assault list inferred).
local DYNAMIS = { [39] = true, [40] = true, [41] = true, [42] = true, [134] = true, [135] = true, [185] = true,
  [186] = true, [187] = true, [188] = true, [294] = true, [295] = true, [296] = true, [297] = true }
local ASSAULT = { [55] = true, [56] = true, [63] = true, [64] = true, [66] = true, [69] = true, [77] = true }

-- Merit level caps by main level.
local function merit_cap(level)
  for _, c in ipairs({ { 80, 15 }, { 75, 10 }, { 70, 9 }, { 65, 8 }, { 60, 7 }, { 55, 6 }, { 50, 5 }, { 40, 4 },
    { 30, 3 }, { 20, 2 }, { 10, 1 } }) do
    if level >= c[1] then return c[2] end
  end
  return 0
end

local DARK_SEAL, SABER_DANCE = 2496, 3200

-- Our side of a prediction: jobs, merits, job points, gear and effect mods, skills, TP.
local Caster = {}
Caster.__index = Caster
M.Caster = Caster

function Caster.new(t)
  t = t or {}
  t.main = t.main or { 0, 0 }
  t.statuses = t.statuses or {}
  t.mods = t.mods or {}
  t.merits = t.merits or {}
  t.jps = t.jps or {}
  t.skills = t.skills or {}
  t.tp = t.tp or 1000
  t.ammo = t.ammo or 0
  return setmetatable(t, Caster)
end

function Caster:has(s) return not self.pet and self.statuses[s] == true end

function Caster:m(id)
  if self.pet then return 0 end
  return self.mods[id] or 0
end

-- getMerit: the upgrades (at most the level's cap) times the value, for a general merit or one of our main job at
-- main level 75 (96 for weapon skill merits).
function Caster:merit(id)
  load()
  if self.pet then return 0 end
  local r = merits[id]
  if not r then return 0 end
  local v, jobs, ws = r[1], r[2], r[3]
  if jobs ~= 0 and ((jobs & (1 << self.main[1])) == 0 or self.main[2] < (ws and 96 or 75)) then return 0 end
  return math.min(self.merits[id] or 0, merit_cap(self.main[2])) * v
end

-- getJobPointLevel: only for our main job at 99.
function Caster:jp(id)
  load()
  if self.pet or self.main[2] < 99 or jps[id] ~= self.main[1] then return 0 end
  return self.jps[id] or 0
end

function Caster:skill(id) return self.skills[id] or 0 end

local function add(c, m, v) c.mods[m] = (c.mods[m] or 0) + v end

-- The item in each equipment slot with its extra data: slot -> {id, ext}.
function M.equipped()
  local out, bags = {}, {}
  for slot, e in pairs(game.equipment()) do
    if e.id then
      if not bags[e.bag] then
        bags[e.bag] = {}
        for _, it in ipairs(game.items(e.bag)) do bags[e.bag][it.slot] = it.extdata end
      end
      out[slot] = { id = e.id, ext = bags[e.bag][e.slot] }
    end
  end
  return out
end

-- An equipped item's duration mods: its table rows (latents by zone) and its augments, none while our main level is
-- below the item's (level sync).
function M.item_mods(piece, main_level, zone)
  load()
  local out = {}
  if not piece then return out end
  for _, g in ipairs(gear[piece.id] or {}) do
    local m, v, level, latent = g[1], g[2], g[3], g[4]
    local on = latent == 1 and DYNAMIS[zone] or latent == 2 and ASSAULT[zone] or latent == 0
    if on and main_level >= level then out[#out + 1] = { m, v } end
  end
  -- LSB reads slots 0..3 of every equipment's extra data as standard augments.
  local item = res.item(piece.id)
  local ext = piece.ext
  if ext and #ext >= 10 and main_level >= (item and item.level or 0) then
    for k = 0, 3 do
      local lo, hi = ext:byte(3 + 2 * k, 4 + 2 * k)
      local raw = lo | (hi << 8)
      local id, v = raw & 0x7ff, raw >> 11
      for _, a in ipairs(augments[id] or {}) do
        local mult, m, base = a[1], a[2], a[3]
        out[#out + 1] = { m, ((base > 0) and (base + v) or (base - v)) * math.max(mult, 1) }
      end
    end
  end
  return out
end

-- One slot's value of a mod.
function M.slot_mod(gear_now, slot, m, main_level, zone)
  local n = 0
  for _, x in ipairs(M.item_mods(gear_now[slot], main_level, zone)) do
    if x[1] == m then n = n + x[2] end
  end
  return n
end

-- Our snapshot. `lost`: effects that left us within the last moment (the cast that used them reports after their
-- wear-off); `roll_on_me(status)`: the total of a roll that landed on us; `pet_tp`: our pet's TP.
function M.caster(lost, tp, roll_on_me, pet_tp)
  local p = game.player()
  local c = Caster.new({ main = { p.main_job or 0, p.main_level or 0 }, tp = tp or 1000, pet_tp = pet_tp })
  for _, b in ipairs(game.buffs()) do c.statuses[b.id] = true end
  for _, s in ipairs(lost or {}) do c.statuses[s] = true end
  for id, m in pairs(game.merits()) do c.merits[id] = m.level end
  local jp = game.job_points()
  for _, j in pairs(jp) do
    for id, level in pairs(j.upgrades or {}) do c.jps[id] = level end
  end
  local sk = game.skills()
  for id, s in pairs(sk.combat or {}) do c.skills[id] = s.level end
  for id, s in pairs(sk.magic or {}) do c.skills[id] = s.level end
  local now = M.equipped()
  c.gear = now
  for slot = 0, 15 do
    local piece = now[slot]
    if piece then
      if slot == 2 then
        local it = res.item(piece.id)
        c.instrument = it ~= nil and (it.skill == 41 or it.skill == 42)
      end
      if slot == 3 then c.ammo = piece.id end
      for _, x in ipairs(M.item_mods(piece, c.main[2], p.zone)) do add(c, x[1], x[2]) end
    end
  end
  -- Mods our effects add, and the BRD job point gift.
  local s = 0
  if c.main[1] == 20 and c.main[2] >= 20 then s = 3 * math.floor((c.main[2] - 10) / 10) end
  local arts_w = c:has(358) or c:has(401)
  if arts_w then add(c, 339, 2 * s) end
  if c:has(377) then
    local s2 = math.floor(1.5 * s)
    add(c, 339, arts_w and math.ceil(2 * s2 / 1.5) or 2 * s2)
    add(c, 477, 108)
  elseif c:has(359) or c:has(402) then
    add(c, 477, 72)
  end
  -- DARK_MAGIC_DURATION from Dark Seal (merit x ENHANCES_DARK_SEAL / 10).
  if c:has(345) then add(c, 1072, c:merit(DARK_SEAL) * c:m(1073) / 10) end
  -- SAMBA_PDURATION from Saber Dance merits.
  if c:has(410) then
    local v = c:merit(SABER_DANCE) - 5
    if v > 0 then add(c, 498, v) end
  end
  -- ENH_MAGIC_DURATION from Naturalist's Roll on us: its power by the total we saw land.
  local total = c:has(339) and roll_on_me and roll_on_me(339)
  if total then
    local POWER = { 6, 7, 15, 8, 9, 10, 5, 11, 12, 13, 20, -5 }
    add(c, 890, POWER[math.max(1, math.min(12, total))])
  end
  -- Clarion Call: one more song a target.
  if c:has(499) then add(c, 453, 1) end
  -- BRD 1200 job points spent, main BRD 99: SONG_DURATION_BONUS +5.
  if c.main[1] == 10 and c.main[2] == 99 and jp[10] and jp[10].spent >= 1200 then add(c, 454, 5) end
  return c
end

-- Spells of the white magic group (Perpetuance, Embolden).
function M.white(spell)
  local s = res.spell(spell)
  return s ~= nil and s.kind == 1
end

-- The weapon skill TP formula for the TP-scaled rows.
local function ws_tp(id, tp)
  local function f(a, b, c)
    if tp <= 2000 then return a + (b - a) * math.max(tp - 1000, 0) / 1000 end
    return b + (c - b) * (tp - 2000) / 1000
  end
  local function is(...)
    for _, x in ipairs({ ... }) do if id == x then return true end end
    return false
  end
  if is(83, 181, 80, 85) then return 120 + 6 * tp / 100 end
  if id == 87 then return 60 + 3 * tp / 100 end
  if is(138, 187, 155, 58, 219, 99, 188, 29, 186, 52) then return 6 * tp / 100 end
  if id == 107 then return 18 * tp / 100 end
  if id == 102 then return 30 + 3 * tp / 100 end
  if id == 129 then return f(30, 60, 120) end
  if is(139, 16) then return 75 + 15 * tp / 1000 end
  if id == 17 then return 30 + 6 * tp / 100 end
  if id == 224 then return 45 + 45 * tp / 1000 end
  if is(162, 2, 65, 239) then return tp / 500 end
  if id == 15 then return 15 + 3 * tp / 1000 end
  return nil
end

local function repair_secs(ammo)
  return ({ [18731] = 15, [18732] = 30, [18733] = 45, [19185] = 60 })[ammo] or 0
end

-- The song family's mod (the row's first song mod: 433..450 but the all-songs and duration ones).
local function song_mod(c, row)
  for _, m in ipairs(row.mods) do
    if m >= 433 and m <= 450 and not (m == 444 or m == 446 or m == 449) then return c:m(m) end
  end
  return 0
end

local function enhancing(c, row, t, white)
  local d = row.formula == "ENH_RANDOM" and 420 + 120 or row.base
  local function tmod(m) return (t.mods or {})[m] or 0 end
  if white and t.has[534] and not c.pet then d = d * (0.5 + tmod(1019) / 100) end
  d = d + d * c:m(890) / 100
  if c.main[1] == 5 then d = d + c:merit(2320) + c:jp(169) end
  if row.formula == "ENH_REGEN" then
    d = d + c:m(339) + 3 * c:jp(104)
  elseif row.formula == "ENH_RANDOM" and row.effect == 69 then
    d = d + tmod(947)
  elseif row.formula == "ENH_RANDOM" and row.effect == 71 then
    d = d + tmod(946)
  elseif row.formula == "ENH_EN" then
    d = d + tmod(938)
  end
  if row.statuses[419] and c:has(419) and t.me then d = d * 3 end
  if c:has(469) and white then d = d * 2 end
  if row.lvl and t.level and t.level < row.lvl then d = d * t.level / row.lvl end
  return d
end

-- ENF, with the resist rate at 1 (an upper bound) and the monster taken as no NM.
local function enfeebling(c, row, t)
  local d = row.effect == 11 and 60 or row.effect == 10 and 7 or row.base
  if row.effect >= 128 and row.effect <= 133 then d = d + c:merit(2258) end
  if row.statuses[454] then
    if c:has(454) and t.first then d = d * 2 end
    if c.main[1] == 5 then
      d = d + c:merit(2316) + c:jp(167)
      if c:has(494) then d = d + c:jp(162) end
    end
    d = math.floor(d * (1 + c:m(1151) / 100))
  end
  d = math.floor(d)
  if row.effect == 11 then d = math.max(5, math.min(60, d)) end
  return d
end

local FIXED = {}
for _, f in ipairs({ "FIXED", "NIN_FIXED", "BLU_ENF", "BLU_ENF_RAW", "DIA_BIO", "MOB_STATUS", "MOB_STATUS_EX",
  "MOB_GAZE", "RANDOM", "WS_R", "WS_FIXED", "QD_SLEEP", "DESPERATE", "HOLY2_AMNESIA", "JA_CHARM_BIND", "SOLACE_SS",
  "ARISE_RR" }) do
  FIXED[f] = true
end

-- LSB's length for one row landing on `t` ({me, level, has, first, mods}); nil when there is nothing to time.
-- `prior`: the time left on our timer of the same effect there (steps).
function M.predict(c, row, t, white, prior)
  local base, f = row.base, row.formula
  local d
  if FIXED[f] then
    d = base
  elseif f == "ENH" or f == "ENH_REGEN" or f == "ENH_RANDOM" or f == "ENH_EN" then
    d = enhancing(c, row, t, white)
  elseif f == "ENF" then
    d = enfeebling(c, row, t)
  elseif f == "HELIX" then
    d = 30
    if c.main[2] >= 60 then d = d + 60 elseif c.main[2] >= 40 then d = d + 30 end
    if c:has(359) then d = d + 3 * c:jp(643) end
    d = math.floor(d + c:m(477))
  elseif f == "KAUSTRA" then
    d = math.floor(3 * (1 + math.max(0, math.min(500, c:skill(37))) / 11))
  elseif f == "IMPACT" then
    d = 180
  elseif f == "ABSORB_STAT" or f == "ABSORB_SELF" then
    local b = math.max(0, math.min(10000, 180 + math.floor((c:skill(37) - 490.5) / 5)))
    d = math.floor(b * (1 + c:m(1072) / 100) * (1 + c:m(1138) / 100)) + c:m(1136)
  elseif f == "DRAIN_MAXHP" then
    d = 180 * (1 + c:m(1072) / 100)
  elseif f == "ENH_SONG" or f == "ENH_SONG_SVD" then
    local b = song_mod(c, row) + c:m(452)
    d = math.floor(120 * (1 + 0.1 * b + c:m(454) / 100))
    if c:has(499) then d = d + 2 * c:jp(322) end
    if c:has(231) then d = d + c:jp(328) end
    if c:has(455) then d = d + 2 * c:jp(326) end
    if f == "ENH_SONG_SVD" then
      if c:has(52) then d = d * 2 elseif c:has(231) then d = math.floor(d * 1.5) end
    end
    if c:has(348) then d = d * 2 end
  elseif f == "ENF_SONG" then
    local gb = song_mod(c, row) + c:m(452)
    d = math.floor(base * (1 + gb / 10 + c:m(454) / 100))
    if row.effect == 2 then d = d + c:jp(327) end
    if c:has(499) then d = d + 2 * c:jp(322) end
    if c:has(455) then d = d + 2 * c:jp(326) end
    if c:has(348) then d = d * 2 end
  elseif f == "BLU_BUFF" or f == "BLU_BUFF_DIFF2" then
    -- Diffusion's merit reaches only the caster.
    local m = c:merit(3010)
    if c:has(356) and t.me and m > 0 then
      d = f == "BLU_BUFF" and base + (m - 5) * base / 100 or base + base / 100 * m
    else
      d = base
    end
  elseif f == "INDI" then
    d = 180 + c:m(960)
  elseif f == "BP_WARD" or f == "BP_WARD*R" then
    d = base + math.max(0, math.min(200, c:skill(38) - 300))
  elseif f == "BP_PD" then
    d = 30 + math.min(c:skill(38), 600) / 20
  elseif f == "BP_NIGHTMARE" then
    d = 90
  elseif f == "MOB_TP_SCALED" then
    local tp = c.pet_tp or 1000
    d = math.max(180, math.min(540, 180 + 180 * (tp - 1000) / 1000))
  elseif f == "ROLL" then
    d = 300 + c:merit(3076) + c:m(882) + 2 * c:jp(545)
  elseif f == "STEP" then
    local jp = c:jp(609)
    d = prior and math.min(math.floor(prior) + 30 + jp, 120 + jp) or 60 + jp
  elseif f == "JIG" then
    d = math.floor(((row.id == 196 and 180 or 120) + c:jp(613)) * (1 + math.max(0, math.min(50, c:m(492))) / 100))
  elseif f == "SAMBA" then
    d = (120 + c:m(490) + 2 * c:jp(611)) * (100 + c:m(498)) / 100
  elseif f == "RUNE" then
    d = 300
  elseif f == "VALLATION" then
    d = 120 + c:jp(707)
  elseif f == "VALIANCE" then
    d = 180 + c:jp(707)
  elseif f == "OFA" then
    d = 30 + c:jp(712)
  elseif f == "LIEMENT" then
    d = 10 + c:m(1017)
  elseif f == "GAMBIT" then
    d = 60 + c:jp(713) + c:m(1018)
  elseif f == "RAYKE" then
    d = 27 + c:merit(3458) + c:m(1014) * c:merit(3458) / 3
  elseif f == "PLUS_MOD" then
    d = base
    for _, m in ipairs(row.mods) do d = d + c:m(m) end
  elseif f == "PLUS_JP" then
    d = base
    for _, j in ipairs(row.jps) do d = d + c:jp(j) end
  elseif f == "JA_KILLER_INSTINCT" then
    d = 180 + c:merit(2562) - 10
  elseif f == "JA_TOMAHAWK" then
    d = 30 + c:merit(2050) - 15
  elseif f == "JA_ANGON" then
    d = 15 + c:merit(2882)
  elseif f == "JA_PENANCE" then
    d = c:merit(2118)
  elseif f == "JA_SHADOWBIND" then
    d = 30 + c:m(425) + c:jp(357)
  elseif f == "JA_FERAL_HOWL" then
    d = 10 + (c:m(503) >= 1 and c:merit(2560) / 5 or 0)
  elseif f == "JA_REPAIR" then
    d = repair_secs(c.ammo)
  elseif f == "WS_TPR" or f == "WS_TP_PART_R" or f == "WS_TP_NOR" then
    local v = ws_tp(row.id, math.min(c.tp, 3000))
    if not v then return nil end
    d = math.floor(v)
  else
    return nil
  end
  -- The container raises it to the effect's least length.
  local r = M.rule(row.effect)
  if d > 0 then return math.max(d, r and r.min or 0) end
  return nil
end

-- Whether a result lands this row: its message, and the value the row's param kind expects.
function M.lands(row, message, value)
  if not row.landed[message] then return false end
  local p = row.param
  if p == "E" then return value == row.effect end
  if p == "D" and row.formula == "HELIX" then return value > 0 end
  if p == "R" then return value >= 1 and value <= 11 end
  if p == "S" or p == "G" then return value > 0 end
  if p == "V" then return value == 531 end
  return true
end

-- The icons a status shows as (the wear-off and party lists' data): sleep II and lullaby as sleep, Copy Image by its
-- shadows, Doubt as Intimidate.
function M.icon_matches(status, icon)
  if status == icon then return true end
  if status == 19 or status == 193 then return icon == 2 end
  if status == 66 then return icon >= 444 and icon <= 446 end
  if status == 576 then return icon == 22 end
  return false
end

-- The status rule's flag (d death, z zone, n no loss message ...).
function M.flag(status, f)
  local r = M.rule(status)
  return r ~= nil and r.flags:find(f, 1, true) ~= nil
end

-- Effects drawn with icon 0 or the no-loss flag: the server never says they ended.
function M.silent_end(status)
  if status == 793 or status == 798 or status == 801 or status == 805 or status == 806 then return true end
  return M.flag(status, "n")
end

-- Statuses a new one ends on the same target: its `remove`, `negative` and exclusion group, Dia vs Bio, the stat
-- boosts' and storms' exclusivity. The same status is the caller's (several kinds keep copies).
function M.ends(status)
  load()
  local r = rules[status]
  local out = {}
  if not r then return out end
  if r.remove and r.remove ~= status then out[#out + 1] = r.remove end
  if r.negative and r.negative ~= status then out[#out + 1] = r.negative end
  if r.exclusion then
    for id, o in pairs(rules) do
      if id ~= status and o.exclusion == r.exclusion then out[#out + 1] = id end
    end
  end
  if status == 134 then out[#out + 1] = 135 end
  for _, span in ipairs({ { 80, 86 }, { 178, 185 } }) do
    if status >= span[1] and status <= span[2] then
      for s = span[1], span[2] do
        if s ~= status then out[#out + 1] = s end
      end
    end
  end
  return out
end

-- Several copies of these coexist on a target (songs, maneuvers, runes, bust): keyed by tier too.
function M.duplicates(status)
  local r = M.rule(status)
  return r ~= nil and r.overwrite == "ignore_duplicate"
end

return M
