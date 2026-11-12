-- The suggestion rules: a deterministic rule set over the facts (facts.lua) naming a spell, ability or weapon skill
-- worth using now, and why. Pure: facts in, rows out ({kind, id, target, reason, group, label}), so recorded cases
-- replay them. The skillchain rules (form, level, elements, chain text) come from the Skillchain mod (R.chain).

local R = {}

-- Rows at most.
R.MAX = 4
-- Seconds left under which an effect is worth renewing.
local SOON = 30.0
-- A closer counts this long after its resonance started (the Skillchain mod's WAIT).
local WAIT = 3.0

-- The skillchain rules: {form = fn(resonance, closer), level = fn(p), elements = fn(p) -> element ids,
-- chain_text = fn(p)}; nil while the Skillchain mod is not there.
R.chain = nil

-- Ailments and the spell that lifts each: petrification, doom, silence, paralysis, curse, poison, blindness, disease,
-- plague (status ids; spells Stona .. Cursna).
local AILMENTS = { { 7, 18, "petrified" }, { 15, 20, "doomed" }, { 6, 17, "silenced" }, { 4, 15, "paralyzed" },
  { 9, 20, "cursed" }, { 3, 14, "poisoned" }, { 5, 16, "blinded" }, { 8, 19, "diseased" }, { 31, 19, "plagued" } }
-- Cure I .. VI and Curaga I .. V with a typical heal each (rough, for picking a tier, not a promise).
local CURES = { { 1, 40 }, { 2, 90 }, { 3, 190 }, { 4, 380 }, { 5, 590 }, { 6, 700 } }
local CURAGAS = { { 7, 60 }, { 8, 130 }, { 9, 270 }, { 10, 450 }, { 11, 600 } }
-- Raise I .. III, Arise, strongest first.
local RAISES = { 494, 140, 13, 12 }
-- Utsusemi: San, Ni, Ichi; the Copy Image statuses (1, 2, 3, 4+ shadows).
local UTSUSEMI = { 340, 339, 338 }
local SHADOWS = { 66, 444, 445, 446 }
-- Curing Waltz V .. I (the job ability names).
local WALTZES = { "Curing Waltz V", "Curing Waltz IV", "Curing Waltz III", "Curing Waltz II", "Curing Waltz" }
-- Our effects worth renewing before they wear, and their spells.
local RENEW = {
  { 43, { 109, 473, 894 }, "Refresh" }, { 33, { 57, 511 }, "Haste" }, { 40, { 47, 46, 45, 44, 43 }, "Protect" },
  { 41, { 52, 51, 50, 49, 48 }, "Shell" }, { 42, { 504, 477, 111, 110, 108 }, "Regen" }, { 116, { 106 }, "Phalanx" },
  { 37, { 54 }, "Stoneskin" }, { 36, { 53 }, "Blink" }, { 39, { 55 }, "Aquaveil" },
  { 113, { 848, 142, 141, 135 }, "Reraise" },
}
-- Effects worth having up between fights when missing, and their spells (strongest first).
local BETWEEN = { { 40, { 47, 46, 45, 44, 43 }, "Protect" }, { 41, { 52, 51, 50, 49, 48 }, "Shell" },
  { 113, { 848, 142, 141, 135 }, "Reraise" } }
-- Debuffs to keep on what we fight: the status and the spells that give it (strongest first).
local DEBUFFS = {
  { 134, { 25, 24, 23 }, "Dia" }, { 135, { 232, 231, 230 }, "Bio" }, { 13, { 79, 56, 345, 344 }, "slowed" },
  { 4, { 80, 58, 342, 341 }, "paralyzed" }, { 5, { 276, 254, 348, 347 }, "blinded" }, { 12, { 217, 216 }, "weighed down" },
  { 194, { 422, 421 }, "Elegy" }, { 192, { 374, 373, 372, 371, 370, 369, 368 }, "Requiem" },
  { 3, { 221, 220, 350 }, "poisoned" },
}
-- Debuff rows at most at once.
local DEBUFFS_AT_ONCE = 2
-- The elements' words in reasons, and the elemental nuke families' names, by element id (0 fire .. 7 dark).
local ELEMENT_WORDS = { [0] = "fire", "ice", "wind", "earth", "lightning", "water", "light", "darkness" }
local NUKE_FAMILIES = { [0] = "Fire", "Ice", "Wind", "Earth", "Lightning", "Water", "Light", "Dark" }
local DAY_NAMES = { [0] = "Firesday", "Iceday", "Windsday", "Earthsday", "Lightningday", "Watersday", "Lightsday",
  "Darksday" }
-- RUN's runes, Ignis .. Tenebrae (statuses 523 .. 530, in the spells' element order).
local RUNES = { [0] = "Ignis", "Gelus", "Flabra", "Tellus", "Sulpor", "Unda", "Lux", "Tenebrae" }
local IGNIS = 523
-- Main jobs that fight in melee, and that nuke.
local MELEE = { [1] = true, [2] = true, [6] = true, [7] = true, [8] = true, [9] = true, [11] = true, [12] = true,
  [13] = true, [14] = true, [16] = true, [17] = true, [18] = true, [19] = true, [22] = true }
local NUKERS = { [4] = true, [5] = true, [20] = true, [21] = true }

-- Job abilities worth using while fighting when their effect is not up: the name, its status (0: none to check),
-- when, why.
local BUFFS = {
  { "Berserk", 56, "fighting", "attack up" }, { "Warcry", 68, "fighting", "the party's attack up" },
  { "Last Resort", 64, "fighting", "attack up" }, { "Souleater", 63, "hp_over", "HP into damage while it is high" },
  { "Hasso", 353, "fighting", "faster two-handed swings" }, { "Meditate", 0, "tp_under", "TP now" },
  { "Third Eye", 67, "targeted", "the monster is on you" }, { "Sneak Attack", 65, "tp_ready", "before the weapon skill" },
  { "Focus", 59, "fighting", "accuracy up" }, { "Chakra", 0, "hp_under", "HP low" },
  { "Sentinel", 62, "hp_under", "HP low while the monster is on you" }, { "Jump", 0, "fighting", "damage and TP" },
  { "High Jump", 0, "fighting", "damage and TP" }, { "Aggressor", 58, "fighting", "accuracy up" },
}

-- ---- The facts' defaults (a recorded case leaves out what is empty) ----

local function normalize(f)
  f.party = f.party or {}
  for _, m in ipairs(f.party) do
    m.name, m.hp, m.hp_max, m.job, m.effects = m.name or "", m.hp or 0, m.hp_max or 0, m.job or 0, m.effects or {}
  end
  f.main, f.sub = f.main or { 0, 0 }, f.sub or { 0, 0 }
  f.mpp, f.tp = f.mpp or 0, f.tp or 0
  f.own, f.spells, f.abilities, f.weapon_skills, f.partners =
    f.own or {}, f.spells or {}, f.abilities or {}, f.weapon_skills or {}, f.partners or {}
  for _, s in ipairs(f.spells) do
    s.name, s.skill, s.element, s.mp, s.targets = s.name or "", s.skill or 0, s.element or 0, s.mp or 0, s.targets or 0
  end
  for _, list in ipairs({ f.abilities, f.weapon_skills }) do
    for _, a in ipairs(list) do a.name, a.props, a.power = a.name or "", a.props or { 0, 0, 0 }, a.power or 0 end
  end
  if f.target then
    local t = f.target
    t.name, t.hpp, t.ours = t.name or "", t.hpp or 0, t.ours or {}
  end
  if f.resonance then
    local r = f.resonance
    r.against, r.age, r.left = r.against or {}, r.age or 0, r.left or 0
  end
  for _, p in ipairs(f.partners) do p.tp, p.skills = p.tp or 0, p.skills or {} end
  return f
end

-- ---- Helpers ----

local function contains(list, x)
  for _, v in ipairs(list or {}) do
    if v == x then return true end
  end
  return false
end

local function spell(f, id)
  for _, s in ipairs(f.spells) do
    if s.id == id then return s end
  end
end

local function can(f, id) return spell(f, id) ~= nil end

local function ability(f, name)
  local n = name:lower()
  for _, a in ipairs(f.abilities) do
    if a.name:lower() == n then return a end
  end
end

local function has(f, status)
  for _, e in ipairs(f.own) do
    if e[1] == status then return true end
  end
  return false
end

-- Fighting: engaged, or a monster is on us.
local function fighting(f) return f.engaged == true or (f.target ~= nil and f.target.hitting_me == true) end

local function row(kind, id, target, reason)
  return { kind = kind, id = id, target = target, reason = reason, group = {} }
end

-- A row unless one for the same action is there already.
local function push(out, s)
  for _, o in ipairs(out) do
    if o.kind == s.kind and o.id == s.id then return end
  end
  out[#out + 1] = s
end

-- "the monster is on you" as a sentence.
local function sentence(why) return why:sub(1, 1):upper() .. why:sub(2) .. "." end

local function tier(tbl, missing, f)
  local have = {}
  for _, e in ipairs(tbl) do
    if can(f, e[1]) then have[#have + 1] = e end
  end
  for i = #have, 1, -1 do
    if have[i][2] <= missing then return have[i][1] end
  end
  return have[1] and have[1][1]
end

local function starts(s, p) return s:sub(1, #p) == p end

-- A spell that does magic damage of its element (a burst candidate): elemental and divine magic, Drain / Aspir, the
-- elemental ninjutsu; not the damage-over-time spells.
local function nuke(s)
  for _, p in ipairs({ "Burn", "Frost", "Choke", "Rasp", "Shock", "Drown", "Bio", "Dia" }) do
    if starts(s.name, p) then return false end
  end
  return (s.targets & 0x20) ~= 0 and (s.skill == 32 or s.skill == 36 or starts(s.name, "Drain") or starts(s.name, "Aspir")
    or (s.skill == 39 and s.name:find("ton:", 1, true) ~= nil))
end

-- Ancient magic, Flare .. Flood II: last in a family, suggested only when nothing else of the element is known.
local function ancient(id) return id >= 204 and id <= 215 end

local function b(x) return x and 1 or 0 end

-- Lexicographic compare of two equal-length key arrays: -1, 0, 1.
local function cmp(x, y)
  for i = 1, #x do
    if x[i] < y[i] then return -1 end
    if x[i] > y[i] then return 1 end
  end
  return 0
end

-- ---- Families (a row's submenu) ----

-- An elemental nuke's element (its tiers, -ga / -ja and ancient magic), else the name without its tier and its area
-- form.
local AREA = { Protectra = "Protect", Shellra = "Shell", Curaga = "Cure", Diaga = "Dia", Poisonga = "Poison",
  Banishga = "Banish", Sleepga = "Sleep" }
function R.family(s)
  if s.skill == 36 and nuke(s) then return NUKE_FAMILIES[s.element] or "Nuke" end
  local n = s.name
  for _, t in ipairs({ " VI", " V", " IV", " III", " II", ": Ichi", ": Ni", ": San" }) do
    if #n > #t and n:sub(-#t) == t then
      n = n:sub(1, -#t - 1)
      break
    end
  end
  return AREA[n] or n
end

local function is_nuke_family(fam)
  for e = 0, 7 do
    if NUKE_FAMILIES[e] == fam then return true end
  end
  return false
end

-- A nuke family's row is always its list's top (a tier back from its recast takes it again).
local function set_group(f, s, fam)
  local group = {}
  for k, x in ipairs(f.spells) do
    if R.family(x) == fam then group[#group + 1] = { x, k } end
  end
  table.sort(group, function(p, q)
    local x, y = p[1], q[1]
    local c = cmp({ b(ancient(x.id)), b(x.area), -x.mp, x.id }, { b(ancient(y.id)), b(y.area), -y.mp, y.id })
    if c ~= 0 then return c < 0 end
    return p[2] < q[2]
  end)
  if #group > 1 then
    if is_nuke_family(fam) then s.id = group[1][1].id end
    s.group = {}
    for _, g in ipairs(group) do s.group[#s.group + 1] = g[1].id end
    s.label = fam
  else
    -- One spell left castable: the row casts it (a held row's may have gone on recast).
    if #group == 1 then s.id = group[1][1].id end
    s.group, s.label = {}, nil
  end
  s.family = fam
end

-- Each spell row's family castable now.
local function add_groups(f, out)
  for _, s in ipairs(out) do
    if s.kind == "spell" then
      local sp = spell(f, s.id)
      local fam = sp and R.family(sp)
      if fam and fam ~= "" then set_group(f, s, fam) end
    end
  end
end

-- Held rows (the cursor on one): each family retaken from what is castable now, so a spell back from its recast
-- returns to the list. The family is the row's, else its spell's when that is castable now.
function R.regroup(f, out)
  normalize(f)
  for _, s in ipairs(out) do
    if s.kind == "spell" then
      local sp = spell(f, s.id)
      local fam = s.family or (sp and R.family(sp))
      if fam and fam ~= "" then set_group(f, s, fam) end
    end
  end
end

-- ---- RUN ----

-- The element a rune resists (the one its element beats, Ignis ice .. Unda fire; Lux dark, Tenebrae light).
local function rune_resists(r)
  if r <= 5 then return (r + 1) % 6 end
  if r == 6 then return 7 end
  return 6
end

local function rune_against(e)
  for r = 0, 7 do
    if rune_resists(r) == e then return r end
  end
  return 0
end

-- The elements' words of a set of runes, by element, once each ("fire and ice").
local function element_list(elements)
  local seen, words = {}, {}
  for _, e in ipairs(elements) do
    if not seen[e] then
      seen[e] = true
      words[#words + 1] = ELEMENT_WORDS[e]
    end
  end
  if #words == 0 then return "" end
  if #words == 1 then return words[1] end
  return table.concat(words, ", ", 1, #words - 1) .. " and " .. words[#words]
end

local function hp_pct(m)
  if not m or m.hp_max == 0 then return 100 end
  return (m.hp * 100) // m.hp_max
end

-- RUN (main or support): the rune for what we fight, then what to do with the runes held.
local function rune_fencer(f, out)
  local level
  if f.main[1] == 22 then level = f.main[2] elseif f.sub[1] == 22 then level = f.sub[2] else return end
  local most = level >= 65 and 3 or level >= 35 and 2 or 1
  local up = {}
  for _, e in ipairs(f.own) do
    if e[1] >= IGNIS and e[1] < IGNIS + 8 then up[#up + 1] = { e[1] - IGNIS, e[2] } end
  end
  local held = {}
  for _, r in ipairs(up) do held[#held + 1] = r[1] end
  local t = f.target and f.target.hpp > 0 and f.target or nil
  local foe = t and t.name or "monster"
  local tanking = t ~= nil and t.hitting_me == true
  local ranks = t and t.ranks
  -- Its element: resisted most, by a clear margin; its weakness: resisted least, below 0.
  local own, weak
  if ranks then
    local top, low = -math.huge, math.huge
    for e = 1, 8 do
      top = math.max(top, ranks[e])
      low = math.min(low, ranks[e])
    end
    local n = 0
    for e = 1, 8 do
      if ranks[e] == top then n = n + 1 end
    end
    if top > 0 and n == 1 then
      for e = 1, 8 do
        if ranks[e] == top then own = e - 1 end
      end
    end
    if low < 0 then
      for e = 8, 1, -1 do
        if ranks[e] == low then weak = e - 1 end
      end
    end
  end
  if fighting(f) then
    local want, why
    if tanking and own then
      want, why = rune_against(own), ("Resists the %s's %s"):format(foe, ELEMENT_WORDS[own])
    elseif weak then
      want, why = weak, ("The %s is weak to %s, for Swipe and Lunge"):format(foe, ELEMENT_WORDS[weak])
    end
    local a = want and ability(f, RUNES[want])
    if a then
      local have, soon = 0, false
      for _, r in ipairs(up) do
        if r[1] == want then
          have = have + 1
          if r[2] and r[2] < SOON then soon = true end
        end
      end
      if have < most or soon then
        push(out, row("ability", a.index, nil, ("%s (%d of %d up)."):format(why, have, most)))
      end
    end
  end
  if not fighting(f) then return end
  local hp = hp_pct(f.party[1])
  local a = tanking and hp < 35 and ability(f, "Elemental Sforzo")
  if a then push(out, row("ability", a.index, nil, ("HP low while the %s is on you."):format(foe))) end
  a = not has(f, 532) and ability(f, "Swordplay")
  if a then push(out, row("ability", a.index, nil, "Accuracy and evasion up as you fight.")) end
  if #held == 0 then return end
  local resisted = {}
  for _, r in ipairs(held) do resisted[#resisted + 1] = rune_resists(r) end
  local guarded = element_list(resisted)
  if tanking then
    if not has(f, 531) and not has(f, 535) then
      local others = false
      for _, m in ipairs(f.party) do
        if not m.me and m.here then others = true end
      end
      local first, other = "Vallation", "Valiance"
      if others then first, other = "Valiance", "Vallation" end
      a = ability(f, first) or ability(f, other)
      if a then
        local who = a.name:lower() == "valiance" and "the party takes" or "you take"
        push(out, row("ability", a.index, nil, ("Less %s damage %s."):format(guarded, who)))
      end
    end
    a = not has(f, 533) and ability(f, "Pflug")
    if a then push(out, row("ability", a.index, nil, ("Fewer %s ailments."):format(guarded))) end
    a = hp < 50 and not has(f, 537) and ability(f, "Liement")
    if a then push(out, row("ability", a.index, nil, ("Absorb %s damage."):format(guarded))) end
    a = #held >= most and not has(f, 570) and ability(f, "Battuta")
    if a then push(out, row("ability", a.index, nil, ("Parry the %s more."):format(foe))) end
  end
  -- Spending them: Swipe takes the newest rune (the one with the most time left), Lunge and Gambit every rune held.
  -- Not while the monster resists all of them.
  if not t or tanking then return end
  local function resists(e) return ranks ~= nil and ranks[e + 1] > 0 end
  local all = true
  for _, e in ipairs(held) do
    if not resists(e) then all = false end
  end
  if all then return end
  local newest, at = 0, nil
  for _, r in ipairs(up) do
    local l = r[2] or 0
    if not at or l >= at then newest, at = r[1], l end
  end
  local words = element_list(held)
  local nukers = false
  for _, m in ipairs(f.party) do
    if not m.me and m.here and NUKERS[m.job] then nukers = true end
  end
  local gambit = nukers and t.hpp > 30 and not contains(t.ours, 536) and ability(f, "Gambit")
  if gambit then
    push(out, row("ability", gambit.index, nil, ("The party's %s magic hits harder."):format(words)))
  else
    local lunge = #held >= 2 and ability(f, "Lunge")
    if lunge then
      push(out, row("ability", lunge.index, nil, ("%d runes as %s on the %s."):format(#held, words, foe)))
    else
      local swipe = not resists(newest) and ability(f, "Swipe")
      if swipe then
        push(out, row("ability", swipe.index, nil, ("A rune as %s on the %s."):format(ELEMENT_WORDS[newest], foe)))
      end
    end
  end
end

-- ---- GEO ----

-- The first spell castable now of `names`.
local function first_spell(f, names)
  for _, n in ipairs(names) do
    local l = n:lower()
    for _, s in ipairs(f.spells) do
      if s.name:lower() == l then return s end
    end
  end
end

-- GEO (main): an Indicolure always on, a Geocolure on what we fight while no luopan is out (Blaze of Glory first), and
-- the luopan looked after.
local function geomancer(f, out)
  if f.main[1] ~= 21 then return end
  local melee, nukers = 0, 0
  for _, m in ipairs(f.party) do
    if not m.me and m.here then
      if MELEE[m.job] then melee = melee + 1 end
      if NUKERS[m.job] then nukers = nukers + 1 end
    end
  end
  local melee_party = melee > nukers
  if not has(f, 612) then
    local names, why
    if melee_party then
      names, why = { "Indi-Fury", "Indi-Precision", "Indi-Haste", "Indi-Regen" }, "for the melee round you"
    else
      names, why = { "Indi-Acumen", "Indi-Focus", "Indi-Refresh", "Indi-Regen" }, "for the magic round you"
    end
    local s = first_spell(f, names)
    if s then push(out, row("spell", s.id, nil, ("No Indicolure on you: %s, %s."):format(s.name, why))) end
  end
  local luopan = f.pet and f.pet.luopan and f.pet or nil
  local foe = f.target and f.target.hpp > 0 and f.target or nil
  local in_fight = fighting(f) or f.party_fight == true or (foe ~= nil and foe.hitting ~= nil)
  if foe and in_fight then
    if not luopan then
      local names, why
      if melee_party then
        names, why = { "Geo-Frailty", "Geo-Wilt" }, "for your melee"
      else
        names, why = { "Geo-Malaise", "Geo-Languor" }, "for your magic"
      end
      local s = first_spell(f, names)
      if s then
        local a = not has(f, 569) and ability(f, "Blaze of Glory")
        if a then push(out, row("ability", a.index, nil, ("The next luopan (%s) stronger."):format(s.name))) end
        push(out, row("spell", s.id, nil, ("No luopan out: %s on the %s, %s."):format(s.name, foe.name, why)))
      end
    else
      local a = not has(f, 516) and ability(f, "Ecliptic Attrition")
      if a then push(out, row("ability", a.index, nil, "The luopan's effect stronger.")) end
    end
  end
  if not luopan then return end
  local a = not foe and not in_fight and ability(f, "Full Circle")
  if a then push(out, row("ability", a.index, nil, "No fight now: the luopan's MP back.")) end
  local hpp = luopan.hpp or 0
  a = hpp < 50 and ability(f, "Life Cycle")
  if a then
    push(out, row("ability", a.index, nil, ("The luopan at %d%% HP."):format(hpp)))
  else
    a = hpp < 70 and not has(f, 518) and ability(f, "Dematerialize")
    if a then push(out, row("ability", a.index, nil, ("The luopan at %d%% HP."):format(hpp))) end
  end
  a = f.mpp < 25 and ability(f, "Radial Arcana")
  if a then push(out, row("ability", a.index, nil, ("Your MP at %d%%."):format(f.mpp))) end
  local hurt = 0
  for _, m in ipairs(f.party) do
    if m.here and m.hp_max > 0 and m.hp * 5 < m.hp_max * 3 then hurt = hurt + 1 end
  end
  a = hurt >= 3 and ability(f, "Mending Halation")
  if a then push(out, row("ability", a.index, nil, ("%d members below 60%% HP."):format(hurt))) end
end

-- ---- The rules in their order ----

-- Raise, shadows, ailments, heal, close the open chain, burst it, pull the monster off someone, job abilities,
-- debuffs, open a chain with a partner, the strongest weapon skill, renew what wears off.
function R.suggest(f)
  normalize(f)
  local out = {}
  local function who(m) return (not m.me) and m.name or nil end
  local function named(m) return m.me and "You are" or (m.name .. " is") end
  local me = f.party[1]
  local C = R.chain

  -- Raise the fallen.
  for _, m in ipairs(f.party) do
    if not m.me and m.here and m.hp_max > 0 and m.hp == 0 then
      for _, id in ipairs(RAISES) do
        if can(f, id) then
          push(out, row("spell", id, who(m), named(m) .. " KO'd."))
          break
        end
      end
      break
    end
  end
  -- Shadows.
  if fighting(f) then
    local any = false
    for _, s in ipairs(SHADOWS) do
      if has(f, s) then any = true end
    end
    if not any then
      for _, id in ipairs(UTSUSEMI) do
        if can(f, id) then
          push(out, row("spell", id, nil, "No shadows up."))
          break
        end
      end
    end
  end
  -- Ailments.
  for _, a in ipairs(AILMENTS) do
    for _, m in ipairs(f.party) do
      if m.here and contains(m.effects, a[1]) then
        if can(f, a[2]) then push(out, row("spell", a[2], who(m), ("%s %s."):format(named(m), a[3]))) end
        break
      end
    end
  end
  -- Healing: several low, Curaga; else the lowest; a waltz when no cure is known.
  local low = {}
  for _, m in ipairs(f.party) do
    if m.here and m.hp_max > 0 and m.hp > 0 and m.hp * 4 <= m.hp_max * 3 then low[#low + 1] = m end
  end
  local curaga
  if #low >= 3 then
    local sum = 0
    for _, m in ipairs(low) do sum = sum + (m.hp_max - m.hp) end
    curaga = tier(CURAGAS, sum // #low, f)
  end
  if curaga then
    push(out, row("spell", curaga, nil, ("%d members are below 75%% HP."):format(#low)))
  elseif #low > 0 then
    local m, at
    for _, x in ipairs(low) do
      local p = x.hp * 100 // x.hp_max
      if not at or p < at then m, at = x, p end
    end
    local pct = m.hp * 100 // m.hp_max
    local reason = ("%s at %d%% HP (%d missing)."):format(named(m), pct, m.hp_max - m.hp)
    local id = tier(CURES, m.hp_max - m.hp, f)
    if id then
      push(out, row("spell", id, who(m), reason))
    elseif pct < 60 then
      for _, n in ipairs(WALTZES) do
        local a = ability(f, n)
        if a then
          push(out, row("ability", a.index, who(m), reason))
          break
        end
      end
    end
  end
  -- Close the open chain, or burst the one made.
  local r = f.resonance
  if r and C then
    if r.chain and r.left > 1.0 then
      local els = C.elements(r.chain)
      -- Not ancient magic while anything else bursts it, one target over an area, then the strongest (the last of
      -- equals, as the strongest is picked).
      local best, key
      for _, s in ipairs(f.spells) do
        if nuke(s) and s.element < 8 and contains(els, s.element) then
          local k = { b(not ancient(s.id)), b(not s.area), s.mp }
          if not key or cmp(k, key) >= 0 then best, key = s, k end
        end
      end
      if best then
        push(out, row("spell", best.id, nil, ("Burst %s: %.0f s left."):format(C.chain_text(r.chain), r.left)))
      end
    end
    if f.tp >= 1000 and r.left > 0.5 then
      local best, key, chain
      for _, a in ipairs(f.weapon_skills) do
        local c = C.form(r.against, a.props)
        if c then
          local k = { C.level(c), a.power }
          if not key or cmp(k, key) >= 0 then best, key, chain = a, k, c end
        end
      end
      if best then
        local when = r.age < WAIT and (" in %.1f s"):format(WAIT - r.age) or ""
        push(out, row("ability", best.index, nil, ("Closes the open chain%s: %s."):format(when, C.chain_text(chain))))
      end
    end
  end
  -- The monster on someone who should not hold it.
  local t = f.target
  if t and t.hitting and not t.hitting_me then
    for _, m in ipairs(f.party) do
      if m.name == t.hitting then
        if not (m.job == 7 or m.job == 13 or m.job == 22) or m.hp * 2 < m.hp_max then
          local a = ability(f, "Provoke")
          if a then
            push(out, row("ability", a.index, nil, ("The %s is attacking %s."):format(t.name, t.hitting)))
          elseif can(f, 112) then
            push(out, row("spell", 112, nil, ("The %s is attacking %s."):format(t.name, t.hitting)))
          end
        end
        break
      end
    end
  end
  -- Job abilities.
  if fighting(f) then
    local hp = hp_pct(me)
    local targeted = t ~= nil and t.hitting_me == true
    for _, e in ipairs(BUFFS) do
      local name, status, when, why = e[1], e[2], e[3], e[4]
      local a = ability(f, name)
      if a then
        local ok = when == "fighting"
          or (when == "tp_under" and f.tp < 1000)
          or (when == "tp_ready" and f.tp >= 1000 and #f.weapon_skills > 0)
          or (when == "hp_over" and hp > 75)
          or (when == "hp_under" and hp < 50 and (name ~= "Sentinel" or targeted))
          or (when == "targeted" and targeted)
        if ok and (status == 0 or not has(f, status)) and not (name == "Hasso" and has(f, 354)) then
          push(out, row("ability", a.index, nil, sentence(why)))
        end
      end
    end
    -- Yonin while the monster is on us, else Innin.
    if not has(f, 420) and not has(f, 421) then
      local name, why = "Innin", "you are not holding the monster"
      if targeted then name, why = "Yonin", "the monster is on you" end
      local a = ability(f, name)
      if a then push(out, row("ability", a.index, nil, sentence(why))) end
    end
    -- A samba while none is up.
    if not has(f, 368) and not has(f, 369) and not has(f, 370) then
      for _, n in ipairs({ "Haste Samba", "Drain Samba III", "Drain Samba II", "Drain Samba" }) do
        local a = ability(f, n)
        if a then
          push(out, row("ability", a.index, nil, "No samba up."))
          break
        end
      end
    end
  end
  rune_fencer(f, out)
  geomancer(f, out)
  -- Debuffs on the monster we target, engaged or not, while it has the HP for them to matter.
  if t and t.hpp > 30 then
    local given = 0
    for _, d in ipairs(DEBUFFS) do
      if not contains(t.ours, d[1]) and given ~= DEBUFFS_AT_ONCE then
        for _, id in ipairs(d[2]) do
          if can(f, id) then
            local word = d[3]
            local reason
            if word:sub(1, 1):match("%u") then
              reason = ("%s is not on the %s."):format(word, t.name)
            else
              reason = ("The %s is not %s."):format(t.name, word)
            end
            push(out, row("spell", id, nil, reason))
            given = given + 1
            break
          end
        end
      end
    end
  end
  -- A nuke of the element the target resists least, then the one today's weather or day strengthens, then the
  -- strongest; a single-target spell before an area one, ancient magic last.
  if t then
    local best, key
    for _, s in ipairs(f.spells) do
      if s.skill == 36 and nuke(s) and s.element < 8 then
        local e = s.element
        local rank = t.ranks and t.ranks[e + 1] or 0
        local boost = 2 * b(f.weather_element == e) + b(f.day_element == e)
        local k = { -rank, boost, b(not ancient(s.id)), b(not s.area), s.mp }
        if not key or cmp(k, key) > 0 then best, key = s, k end
      end
    end
    if best then
      local word = ELEMENT_WORDS[best.element]
      local rk = t.ranks and t.ranks[best.element + 1]
      local reason
      if rk and rk < 0 then
        reason = ("The %s is weak to %s."):format(t.name, word)
      elseif rk == 0 then
        reason = ("The %s does not resist %s."):format(t.name, word)
      elseif rk then
        reason = ("The %s resists %s least."):format(t.name, word)
      elseif f.weather_element == best.element then
        reason = ("Today's weather strengthens %s."):format(word)
      elseif f.day_element == best.element then
        reason = ("%s strengthens %s."):format(DAY_NAMES[best.element], word)
      else
        reason = ("Your strongest nuke on the %s."):format(t.name)
      end
      push(out, row("spell", best.id, nil, reason))
    end
  end
  -- Open a chain someone with TP can close.
  if C and f.engaged and f.tp >= 1000 and (not r or r.left <= 0.5) then
    local best
    for _, a in ipairs(f.weapon_skills) do
      for _, p in ipairs(f.partners) do
        if p.tp >= 1000 then
          for _, sk in ipairs(p.skills) do
            local c = C.form(a.props, sk[2])
            if c and (not best or cmp({ C.level(c), a.power }, { C.level(best[2]), best[1].power }) > 0) then
              best = { a, c, p, sk[1] }
            end
          end
        end
      end
    end
    if best then
      push(out, row("ability", best[1].index, nil,
        ("Opens %s with %s's %s (TP %d)."):format(C.chain_text(best[2]), best[3].name, best[4], best[3].tp)))
    end
  end
  -- The strongest weapon skill (the last of equals).
  if f.engaged and f.tp >= 1000 then
    local best
    for _, a in ipairs(f.weapon_skills) do
      if not best or a.power >= best.power then best = a end
    end
    if best then
      push(out, row("ability", best.index, nil, ("Your strongest weapon skill (TP %d)."):format(f.tp)))
    end
  end
  -- Renew what wears off soon; between fights, put back what is missing.
  for _, e in ipairs(RENEW) do
    for _, o in ipairs(f.own) do
      if o[1] == e[1] then
        local left = o[2]
        if left and left < SOON then
          for _, id in ipairs(e[2]) do
            if can(f, id) then
              push(out, row("spell", id, nil, ("%s wears off in %ds."):format(e[3], math.ceil(left))))
              break
            end
          end
        end
        break
      end
    end
  end
  if not fighting(f) then
    for _, e in ipairs(BETWEEN) do
      if not has(f, e[1]) then
        for _, id in ipairs(e[2]) do
          if can(f, id) then
            push(out, row("spell", id, nil, ("%s is not on you."):format(e[3])))
            break
          end
        end
      end
    end
  end
  for i = #out, R.MAX + 1, -1 do out[i] = nil end
  add_groups(f, out)
  return out
end

-- The rows as the battle_rows slot takes them (the family kept by the caller for regroup).
function R.rows(out)
  local rows = {}
  for _, s in ipairs(out) do
    rows[#rows + 1] = { kind = s.kind, id = s.id, target = s.target, reason = s.reason, label = s.label, group = s.group }
  end
  return rows
end

return R
