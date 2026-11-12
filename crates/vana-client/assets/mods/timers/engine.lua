-- The effects we put on others and on ourselves: started by the action results that say they landed, ended by the
-- server's wear-off, dispel and defeat messages and the party's effect lists; how long they last is LSB's reckoning
-- (durations.lua).

local D = require("durations")

local E = {
  -- target id -> {name, mob, effects = {{status, name, start, secs, told, tier}...}}
  on = {},
}

-- How long a row stays after its predicted end while the server's word is awaited.
local GRACE = 3.0
-- A party member's effect list may be older than an effect that just landed.
local LIST_LAG = 3.0
-- Our effects that wore off this long ago still count for the action that used them.
local LOST_FOR = 1.5

-- Action result messages removing the status in the value from the target (-na, Healing Waltz, Erase / Dispel /
-- Finale, Larceny, steals).
local REMOVED = { [83] = true, [123] = true, [159] = true, [321] = true, [341] = true, [343] = true, [378] = true,
  [453] = true }
-- A proc removing the status in its value.
local PROC_REMOVED = 168
-- Messages of anyone's effect landing with its status in the value (replaces ours of the same status).
local OTHERS_LANDED = {}
for _, m in ipairs({ 127, 186, 230, 236, 237, 242, 243, 266, 267, 268, 271, 277, 278, 319, 320 }) do
  OTHERS_LANDED[m] = true
end
-- Battle messages: the effect with the icon in the first value wore off (LSB sends only these two); the target was
-- defeated.
local WORN = { [204] = true, [206] = true }
local DEFEATED = { [6] = true, [20] = true, [97] = true }
-- Weapon skill messages (category 3 is shared with some abilities).
local WS_MSGS = { [185] = true, [188] = true, [189] = true, [264] = true, [282] = true, [283] = true }

local lost = {}      -- {status, at}: our effects that just wore off
local ws_tp = nil    -- our TP when our weapon skill started
local rolls = {}     -- roll totals that landed on us, by roll effect
local zones = {}     -- party members' zones as last seen
local arise = {}     -- {id, at}: players we cast Arise on
local luopan = nil   -- {spell, at}: a Geo- spell of ours waiting for its luopan

local now = os.clock

local function me() return game.player().id end

local function members()
  return game.party()
end

local function member(id)
  for _, m in ipairs(members()) do
    if m.id == id then return m end
  end
end

-- Our pet as our status names it.
local function my_pet() return game.player().pet end

function E.roll_on_me(status) return rolls[status] end

local function left_of(e, at)
  if not e.secs then return math.huge end
  return e.secs - (at - e.start)
end

local function remove_where(list, pred)
  local k = 1
  for i = 1, #list do
    if not pred(list[i]) then
      list[k] = list[i]
      k = k + 1
    end
  end
  for i = #list, k, -1 do list[i] = nil end
end

-- The bearer died: effects with the death flag go (all of a monster's).
function E.died(id)
  local b = E.on[id]
  if not b then return end
  local mob = b.mob
  remove_where(b.effects, function(e) return mob or D.flag(e.status, "d") end)
end

-- The server ended the effect shown as `icon` on `target`: of copies of one effect, the one ending first goes.
local function ended(target, icon)
  local b = E.on[target]
  if not b then return end
  local t = now()
  local best, at
  for i, e in ipairs(b.effects) do
    if D.icon_matches(e.status, icon) then
      local l = left_of(e, t)
      if not at or l < at then best, at = i, l end
    end
  end
  if not best then return end
  if D.duplicates(b.effects[best].status) then
    table.remove(b.effects, best)
  else
    remove_where(b.effects, function(e) return D.icon_matches(e.status, icon) end)
  end
end

local function bearer_name(id)
  local p = game.player()
  if id == p.id then return p.name, false end
  local e = game.entity(id)
  if e then return e.name, e.kind == "mob" end
  local m = member(id)
  return m and m.name or "", false
end

local function bearer(id)
  local b = E.on[id]
  if not b then
    b = { name = "", mob = false, effects = {} }
    E.on[id] = b
  end
  local who, mob = bearer_name(id)
  if who ~= "" then b.name, b.mob = who, mob end
  return b
end

-- What put an effect on: {kind = "spell" | "ability" | "ws" | "pet" | "mobskill" | "proc", id}.
local function source_name(kind, id)
  local n
  if kind == "spell" then
    local s = res.spell(id)
    n = s and s.name
  elseif kind == "ability" or kind == "pet" then
    local a = res.ability(0x200 + id)
    n = a and a.name
  elseif kind == "ws" then
    local a = res.ability(id)
    n = a and a.name
  elseif kind == "mobskill" then
    n = res.mob_skill(id)
    if type(n) == "table" then n = n.name end
  elseif kind == "proc" then
    n = res.status(id)
  end
  return n or ""
end

-- What the target looks like to the formulas: ourselves, a party member (level, effect list) or someone else.
local function target_of(id, first, c)
  local t = { me = id == me(), first = first, has = {}, mods = {} }
  if t.me then
    for _, b in ipairs(game.buffs()) do t.has[b.id] = true end
    t.mods = c.mods
  else
    local m = member(id)
    if m then
      if m.main_level and m.main_level > 0 then t.level = m.main_level end
      for _, s in ipairs(m.buffs or {}) do t.has[s] = true end
    end
  end
  return t
end

-- The TP our weapon skill used: the TP at its start, the gear's TP Bonus (not the off-hand's; not the ranged slot's
-- for a melee skill, nor the main hand's for a ranged one), Fencer gear when single-wielding.
local function ws_tp_of(ws, c)
  local ranged = ws >= 192 and ws <= 223
  local skip = ranged and { [0] = true, [1] = true } or { [1] = true, [2] = true }
  local zone, lvl = game.player().zone, c.main[2]
  local tp = ws_tp or 1000
  for slot = 0, 15 do
    if not skip[slot] then tp = tp + D.slot_mod(c.gear, slot, 345, lvl, zone) end
  end
  local function skill(slot)
    local p = c.gear[slot]
    local it = p and res.item(p.id)
    return it and it.skill or 0
  end
  local s0 = skill(0)
  if not ranged and (s0 == 2 or s0 == 3 or s0 == 5 or s0 == 9 or s0 == 11) and skill(1) == 0 then
    for slot = 0, 15 do tp = tp + D.slot_mod(c.gear, slot, 903, lvl, zone) end
  end
  return math.min(tp, 3000)
end

local function is_song_status(s) return s >= 195 and s <= 223 end

-- One row landing on `target`.
local function land(c, a, row, kind, id, target, first, value, white, double_up, t)
  local my = me()
  local f = row.formula
  -- Quick Draw boosts and Double-Up keep the end time; removals were handled.
  if f == "QD_BOOST" or f == "REMOVE" or f == "DISPEL" or f == "COPY" or f == "STEAL" or f == "AURA"
      or f == "HIDDEN_ELE" or f == "NONE" then
    return
  end
  -- The luopan comes as our pet; its life is timed once it is there.
  if f == "AURA_LUOPAN" then
    luopan = { a.param, t }
    return
  end
  if f == "SOLACE_SS" and not (c:has(417) and not target_of(target, first, c).has[37]) then return end
  if row.target == "self" and target ~= a.actor then return end
  local status = row.effect
  local name = source_name(kind, id)
  if f == "ROLL" then
    name = ("%s (%d)"):format(name, value)
    if target == my then rolls[status] = value end
    if double_up then
      local b = E.on[target]
      for _, e in ipairs(b and b.effects or {}) do
        if e.status == status then
          e.name = name
          return
        end
      end
    end
  end
  if f == "STEP" then name = ("%s (%d)"):format(name, value) end
  local prior
  for _, e in ipairs(E.on[target] and E.on[target].effects or {}) do
    if e.status == status and e.secs then
      prior = e.secs - (t - e.start)
      break
    end
  end
  local secs = D.predict(c, row, target_of(target, first, c), white, prior)
  if not secs and target ~= my then return end
  -- No wear-off reaches us: icon-0 / no-loss effects, our pet's, songs on others, Valiance / Vallation on others
  -- (their origin is the bearer), Gambit (the mob's).
  local told = not D.silent_end(status) and kind ~= "pet" and kind ~= "mobskill"
      and not (f:sub(1, 8) == "ENH_SONG" and target ~= my)
      and not ((status == 531 or status == 535) and target ~= my) and status ~= 536
  local max_songs = (c.instrument and 2 or 1) + c:m(453)
  local b = bearer(target)
  if D.duplicates(status) then
    if f:sub(1, 8) == "ENH_SONG" then
      -- The same song and tier is replaced (and, as LSB counts it, still fills its slot); at our cap the song of ours
      -- that ends first goes.
      local ours = 0
      for _, e in ipairs(b.effects) do
        if e.tier > 0 and is_song_status(e.status) then ours = ours + 1 end
      end
      remove_where(b.effects, function(e) return e.status == status and e.tier == row.tier end)
      if ours >= max_songs then
        local best, at
        for i, e in ipairs(b.effects) do
          if e.tier > 0 and is_song_status(e.status) then
            local l = left_of(e, t)
            if not at or l < at then best, at = i, l end
          end
        end
        if best then table.remove(b.effects, best) end
      end
    else
      remove_where(b.effects, function(e) return e.status == status and e.tier == row.tier end)
    end
  else
    remove_where(b.effects, function(e) return e.status == status end)
  end
  -- Rolls: a COR keeps two rolls on a target (one as a sub job); the one ending first goes.
  if f == "ROLL" then
    local limit = c.main[1] == 17 and 2 or 1
    local n, best, at = 0, nil, nil
    for i, e in ipairs(b.effects) do
      if (e.status >= 310 and e.status <= 339) or e.status == 600 then
        n = n + 1
        local l = left_of(e, t)
        if not at or l < at then best, at = i, l end
      end
    end
    if n >= limit and best then table.remove(b.effects, best) end
  end
  local gone = {}
  for _, s in ipairs(D.ends(status)) do gone[s] = true end
  remove_where(b.effects, function(e) return gone[e.status] end)
  b.effects[#b.effects + 1] = { status = status, name = name, start = t, secs = secs, told = told, tier = row.tier }
end

-- Our melee / ranged hit's additional effect: the weapon's length for it.
local function procs(a)
  local ranged = a.category == 2
  local t = now()
  local c
  for _, tg in ipairs(a.targets) do
    for _, r in ipairs(tg.results) do
      local p = r.proc
      if p and (p.message == 160 or p.message == 164) then
        c = c or D.caster(nil, nil, nil, nil)
        local status = p.value
        local secs
        for _, slot in ipairs(ranged and { 2, 3 } or { 0, 1 }) do
          local v = D.slot_mod(c.gear, slot, 953, c.main[2], game.player().zone)
          if v > 0 then
            secs = v
            break
          end
        end
        if secs then
          local b = bearer(tg.id)
          remove_where(b.effects, function(e) return e.status == status end)
          b.effects[#b.effects + 1] = { status = status, name = source_name("proc", status), start = t, secs = secs,
            told = not D.silent_end(status), tier = 0 }
        end
      end
    end
  end
end

-- Someone else's action: their landing of an effect we keep a timer for replaces ours (silently on the server);
-- their step extends the daze we put on; the player we raised with Arise accepting brings Reraise.
local function others(a)
  local t = now()
  local function tier_of(status)
    for _, r in ipairs(D.rows("spell", a.param) or {}) do
      if r.effect == status then return r.tier end
    end
    return 0
  end
  for _, tg in ipairs(a.targets) do
    for _, r in ipairs(tg.results) do
      local b = E.on[tg.id]
      if b then
        if a.category == 14 and ((r.message >= 519 and r.message <= 521) or r.message == 591) then
          local row = (D.rows("ability", a.param) or {})[1]
          if row then
            for _, e in ipairs(b.effects) do
              if e.status == row.effect then
                local gone = t - e.start
                local left = (e.secs or 0) - gone
                e.secs = gone + math.min(math.floor(left) + 30, 120)
                e.told = false
                break
              end
            end
          end
        elseif OTHERS_LANDED[r.message] and r.value ~= 0 then
          local status = r.value
          if D.duplicates(status) then
            local tier = tier_of(status)
            remove_where(b.effects, function(e) return e.status == status and e.tier == tier end)
          else
            local gone = {}
            for _, s in ipairs(D.ends(status)) do gone[s] = true end
            remove_where(b.effects, function(e) return e.status == status or gone[e.status] end)
          end
        end
      end
    end
  end
  -- Arise accepted: the raised player's Arise action; Reraise for an hour, no origin.
  if a.category == 4 and a.param == 494 then
    for i, x in ipairs(arise) do
      if x[1] == a.actor then
        table.remove(arise, i)
        local b = bearer(a.actor)
        remove_where(b.effects, function(e) return e.status == 113 end)
        b.effects[#b.effects + 1] = { status = 113, name = source_name("spell", 494), start = t, secs = 3600,
          told = false, tier = 0 }
        break
      end
    end
  end
end

-- An action for anyone: removals, defeats and others' landings end ours; our (and our pet's) landings start them.
function E.on_action(a)
  for _, tg in ipairs(a.targets) do
    for _, r in ipairs(tg.results) do
      if REMOVED[r.message] then ended(tg.id, r.value) end
      if r.proc and r.proc.message == PROC_REMOVED then ended(tg.id, r.proc.value) end
      -- Bust (426 / 427): the roll leaves every target, silently.
      if r.message == 426 or r.message == 427 then
        local row = (D.rows("ability", a.param) or {})[1]
        local b = E.on[tg.id]
        if row and b then remove_where(b.effects, function(e) return e.status == row.effect end) end
      end
      if r.info & 1 ~= 0 then E.died(tg.id) end
    end
  end
  local my = me()
  local pet = my_pet()
  if a.actor ~= my and a.actor ~= pet then
    others(a)
    return
  end
  local ours = a.actor == my
  local cat = a.category
  if ours and cat == 7 then
    ws_tp = game.player().tp
    return
  end
  local first = a.targets[1] and a.targets[1].results[1]
  local first_msg = first and first.message or 0
  local kind, src
  if cat == 4 then
    kind, src = "spell", "spell"
  elseif (cat == 6 or cat == 14 or cat == 15) and ours then
    kind, src = "ability", "ability"
  elseif cat == 3 and ours and WS_MSGS[first_msg] then
    kind, src = "ws", "ws"
  elseif cat == 3 and ours then
    kind, src = "ability", "ability"
  elseif cat == 3 or cat == 11 then
    kind, src = "pet", "mobskill"
  elseif cat == 13 then
    kind, src = "pet", "pet"
  elseif (cat == 1 or cat == 2) and ours then
    procs(a)
    return
  else
    return
  end
  local rows = D.rows(kind, a.param)
  if not rows then return end
  local lost_ids = {}
  for _, l in ipairs(lost) do lost_ids[#lost_ids + 1] = l[1] end
  local pet_e = pet and game.entity(pet)
  local c = D.caster(lost_ids, nil, E.roll_on_me, pet_e and pet_e.tp)
  if kind == "ws" then c.tp = ws_tp_of(a.param, c) end
  -- A pet casting a spell has none of our inputs.
  c.pet = not ours and kind == "spell"
  local white = kind == "spell" and D.white(a.param)
  local double_up = first_msg == 424
  local t = now()
  local e_first
  for k, row in ipairs(rows) do
    if row.param == "E" then
      e_first = k
      break
    end
  end
  for i, tg in ipairs(a.targets) do
    for _, r in ipairs(tg.results) do
      local hit, hit_at = {}, {}
      for k, row in ipairs(rows) do
        if D.lands(row, r.message, r.value) and row.know ~= "n" and row.effect ~= 0 then
          hit[#hit + 1], hit_at[#hit_at + 1] = row, k
        end
      end
      -- Several effects in one result: the value names the best; it being the first of the action means all landed
      -- (Warm-Up, Amplification ...).
      local ke
      for j, row in ipairs(hit) do
        if row.param == "E" then
          ke = j
          break
        end
      end
      if ke and (hit[ke].effect ~= r.value or hit_at[ke] ~= e_first) then
        local keep = {}
        for _, row in ipairs(hit) do
          if row.param ~= "E" or row.effect == r.value then keep[#keep + 1] = row end
        end
        hit = keep
      end
      for _, row in ipairs(hit) do
        land(c, a, row, src, a.param, tg.id, i == 1, r.value, white, double_up, t)
      end
    end
  end
  if ours and kind == "spell" and a.param == 494 then
    for _, tg in ipairs(a.targets) do arise[#arise + 1] = { tg.id, t } end
  end
end

-- A battle message: wear-offs (the first value is the icon) and defeats.
function E.on_message(target, param, message)
  local m = message & 0x7fff
  if WORN[m] then
    ended(target, param)
    if target == me() then lost[#lost + 1] = { param, now() } end
  elseif DEFEATED[m] then
    E.died(target)
  end
end

-- The party's effect lists came: a member's effects no longer listed have ended (copies of a song: as many of ours
-- as icons left).
function E.on_party_effects()
  local t = now()
  local lists = {}
  for _, m in ipairs(members()) do lists[m.id] = m.buffs or {} end
  for id, b in pairs(E.on) do
    local list = lists[id]
    if list and id ~= me() then
      local function fresh(e) return t - e.start < LIST_LAG end
      local keep = {}
      for i, e in ipairs(b.effects) do
        local listed = false
        for _, icon in ipairs(list) do
          if D.icon_matches(e.status, icon) then
            listed = true
            break
          end
        end
        keep[i] = fresh(e) or listed
      end
      local statuses = {}
      for _, e in ipairs(b.effects) do
        if D.duplicates(e.status) then statuses[e.status] = true end
      end
      for s in pairs(statuses) do
        local icons = 0
        for _, icon in ipairs(list) do
          if icon == s then icons = icons + 1 end
        end
        local ours = {}
        for i, e in ipairs(b.effects) do
          if e.status == s and not fresh(e) then ours[#ours + 1] = i end
        end
        table.sort(ours, function(x, y) return left_of(b.effects[x], t) < left_of(b.effects[y], t) end)
        for k = 1, math.max(#ours - icons, 0) do keep[ours[k]] = false end
      end
      local n = 0
      for i, e in ipairs(b.effects) do
        if keep[i] then
          n = n + 1
          b.effects[n] = e
        end
      end
      for i = #b.effects, n + 1, -1 do b.effects[i] = nil end
    end
  end
end

-- Zoning: effects on anyone outside our party stay behind.
function E.on_zone()
  local party = {}
  for _, m in ipairs(members()) do party[m.id] = true end
  local my = me()
  for id in pairs(E.on) do
    if id ~= my and not party[id] then E.on[id] = nil end
  end
  luopan = nil
end

-- The luopan's life: 600 s at most, or its HP over the drain each 3 s; Bolster and Blaze of Glory as they were at
-- the cast.
function E.luopan_life(c)
  local lvl = c.main[2]
  local jp = c:has(513) and c:jp(672) or 0
  local max = math.floor(250 * lvl / 15) * (1 + 0.03 * jp)
  local hp = c:has(569) and (max / 2 + max * c:jp(675) / 100) or max
  local drain = math.max(math.floor(lvl / 4) - jp, 1)
  return math.min(600, math.ceil(hp / drain) * 3)
end

-- Every so often: drop what ended unannounced, follow party members' zone changes, time a luopan that came, drop
-- the effects of what went out of range or died.
function E.maintain()
  local t = now()
  local my = me()
  remove_where(lost, function(l) return t - l[2] >= LOST_FOR end)
  remove_where(arise, function(x) return t - x[2] >= 600 end)
  -- A party member who changed zones lost their zone-flagged effects.
  local party = {}
  for _, m in ipairs(members()) do
    party[m.id] = true
    local old = zones[m.id]
    zones[m.id] = m.zone
    if old and old ~= m.zone and E.on[m.id] then
      remove_where(E.on[m.id].effects, function(e) return D.flag(e.status, "z") end)
    end
  end
  local present = {}
  for _, b in ipairs(game.buffs()) do present[#present + 1] = b.id end
  for id, b in pairs(E.on) do
    -- What is gone takes its effects (a player's stay while in our party); a death ends those with the death flag
    -- (all of a monster's).
    if id ~= my then
      local e = game.entity(id)
      if not e and not party[id] then
        E.on[id] = nil
      elseif e and (e.status == 2 or e.status == 3) then
        E.died(id)
      end
    end
    if E.on[id] then
      remove_where(b.effects, function(e)
        local gone = t - e.start
        local keep
        if id == my then
          keep = gone < LIST_LAG
          for _, icon in ipairs(present) do
            if D.icon_matches(e.status, icon) then keep = true end
          end
        elseif e.secs then
          keep = gone < e.secs + (e.told and GRACE or 0.3)
        else
          keep = false
        end
        return not keep
      end)
      if #b.effects == 0 then E.on[id] = nil end
    end
  end
  if luopan then
    local spell, at = luopan[1], luopan[2]
    if t - at > 5 then
      luopan = nil
    else
      local pet = my_pet()
      if pet then
        luopan = nil
        local secs = E.luopan_life(D.caster(nil, nil, E.roll_on_me, nil))
        local b = bearer(pet)
        b.mob = false
        b.effects[#b.effects + 1] = { status = 612, name = source_name("spell", spell), start = at, secs = secs,
          told = false, tier = 0 }
      end
    end
  end
end

-- The effects we keep on `id`: {status, name, left, secs} (left and secs nil with no known length).
function E.effects_on(id)
  local out, t = {}, now()
  for _, e in ipairs(E.on[id] and E.on[id].effects or {}) do
    out[#out + 1] = { status = e.status, name = e.name, secs = e.secs, left = e.secs and e.secs - (t - e.start) or nil }
  end
  return out
end

return E
