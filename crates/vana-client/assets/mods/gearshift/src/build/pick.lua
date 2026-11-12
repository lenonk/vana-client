-- The set builder's picks. For a set at the player's level:
--   1. the reference is the set's best piece per slot at the level cap (75 or 99): a curated list where one exists,
--      else the best-scoring piece in the whole item list the job and race can wear at the cap;
--   2. each slot gets the owned piece, wearable now, that gives the most of that reference's weighted stats (its share);
--   3. below MIN_SHARE (nothing close to it) the slot takes the owned piece the set's weights score highest;
--   4. separately, "worth getting": the best piece by the same rule from every item wearable at this level, if it
--      beats the owned pick.
-- Weapons (main, sub, range) are picked only in sets that swap them (mages' idle and casting sets); the others keep
-- what is worn.

local stats = require("src.build.stats")

local M = {}

M.MIN_SHARE = 0.25
M.SLOT_NAMES = { [0] = "main", "sub", "range", "ammo", "head", "body", "hands", "legs", "feet", "neck", "waist",
  "left_ear", "right_ear", "left_ring", "right_ring", "back" }
local WEAPON_SLOTS = { [0] = true, [1] = true, [2] = true }
-- The pairs that share items (an earring fits either ear): each piece is used once.
local PAIR = { [11] = 12, [12] = 11, [13] = 14, [14] = 13 }
local TWO_HANDED = { great_sword = true, great_axe = true, scythe = true, polearm = true, great_katana = true,
  staff = true }
local SKILL_NAME = { [1] = "hand_to_hand", [2] = "dagger", [3] = "sword", [4] = "great_sword", [5] = "axe",
  [6] = "great_axe", [7] = "scythe", [8] = "polearm", [9] = "katana", [10] = "great_katana", [11] = "club",
  [12] = "staff", [25] = "archery", [26] = "marksmanship", [27] = "throwing" }

-- The game, through one table (tests replace it).
M.H = {
  item = function(id) return res.item(id) end,
}

-- Called in every long loop: the engine sets it to yield when its frame's share of time is used (tests: nothing).
M.pace = function() end

-- ---- Item stats, read once ----

local cache = {}
-- An item's parsed stats: by id, or by id and augment lines for an owned piece with them.
function M.stats_of(id, augments)
  local key = id
  if augments and #augments > 0 then key = id .. "|" .. table.concat(augments, "|") end
  local s = cache[key]
  if not s then
    local r = M.H.item(id)
    s = stats.parse(r and r.description or "", augments)
    cache[key] = s
  end
  return s
end

-- A stat's value on parsed stats: "acc", or a pet's "avatar.acc" (a "Pet:" line counts for every pet).
local function value(s, key)
  local dot = key:find(".", 1, true)
  if not dot then return s.stats[key] or 0 end
  local pet, stat = key:sub(1, dot - 1), key:sub(dot + 1)
  local v = 0
  if s.pet[pet] then v = v + (s.pet[pet][stat] or 0) end
  if pet ~= "pet" and s.pet.pet then v = v + (s.pet.pet[stat] or 0) end
  return v
end

-- A piece's score for a set (caps not applied).
function M.score(s, set)
  local t = 0
  for k, w in pairs(set.weights) do t = t + w * value(s, k) end
  return t
end

-- What the piece gives of the reference's kind of stats: its set value from stats the reference also gives
-- (`aligned`), and that value's share of all the piece's set value (`share`, 0..1: how alike they are, whatever
-- their levels).
function M.share(s, ref, set)
  local aligned, total = 0, 0
  for k, w in pairs(set.weights) do
    local v = w * value(s, k)
    if v > 0 then
      total = total + v
      if w * value(ref, k) > 0 then aligned = aligned + v end
    end
  end
  if total <= 0 then return 0, 0 end
  return aligned / total, aligned
end

-- ---- What may go in a slot ----

-- Job and race may wear it, in this slot, at this level (nil: any level).
function M.fits(rec, slot, job_id, race, level)
  if not rec or not rec.slots or rec.slots & (1 << slot) == 0 then return false end
  if rec.jobs & (1 << job_id) == 0 then return false end
  if race and rec.races and rec.races & (1 << race) == 0 then return false end
  if level and (rec.level or 0) > level then return false end
  if (rec.superior_level or 0) > 0 then return false end
  -- Pieces that take a second slot (a suit, a cloak: "Cannot equip leggear") are left out *(not modelled yet)*.
  if rec.description and rec.description:find("Cannot [Ee]quip") then return false end
  return true
end

-- A sub-slot piece that goes with the main (a grip under a two-handed weapon, else no grip).
local function sub_ok(rec, main_rec)
  if not main_rec then return true end
  local two = TWO_HANDED[SKILL_NAME[main_rec.skill or 0] or ""]
  local grip = (rec.skill or 0) == 0 and (rec.damage or 0) == 0 and (rec.type or 0) ~= 5
  if two then return grip end
  return not grip
end

-- ---- Picking ----

-- Totals over the picked pieces (per stat), for the caps.
local function totals(picks, skip)
  local t = {}
  for slot, p in pairs(picks) do
    if slot ~= skip and p.s then
      for k, v in pairs(p.s.stats) do t[k] = (t[k] or 0) + v end
    end
  end
  return t
end

-- A piece's score given the other slots' totals: what it adds under the caps.
local function marginal(s, set, others)
  local t = 0
  for k, w in pairs(set.weights) do
    local v = value(s, k)
    local cap = set.caps and set.caps[k]
    if cap and not k:find(".", 1, true) then
      local before = others[k] or 0
      local after = before + v
      if cap >= 0 then
        v = math.max(0, math.min(after, cap) - math.min(before, cap))
      else
        v = math.min(0, math.max(after, cap) - math.max(before, cap))
      end
    end
    t = t + w * v
  end
  return t
end

-- The best pieces per slot from candidates {slot = {{id, s, rec, owned...}}} by the set's weights and caps: each slot
-- its best alone, then a few passes letting each slot re-pick against the others' totals; a pair's two slots never
-- take the same owned piece.
function M.best_by_score(cands, set)
  local picks = {}
  for pass = 1, 3 do
    for slot = 0, 15 do
      local list = cands[slot]
      if list and #list > 0 then
        local others = totals(picks, slot)
        local best, best_v = nil, -math.huge
        local twin = PAIR[slot] and picks[PAIR[slot]]
        for _, c in ipairs(list) do
          if not (twin and c.key and c.key == twin.key) then
            local v = pass == 1 and M.score(c.s, set) or marginal(c.s, set, others)
            if v > best_v then best, best_v = c, v end
          end
          M.pace()
        end
        if best and best_v > 0 then picks[slot] = best else picks[slot] = nil end
      end
    end
  end
  return picks
end

-- The candidate giving the most of the reference's kind of stats, among those at least MIN_SHARE alike; with its
-- share (nil when none is alike enough).
local function by_share(cands, ref, set, used)
  local best, best_sh, best_al = nil, nil, 0
  for _, c in ipairs(cands) do
    M.pace()
    if not (c.key and used[c.key]) then
      local sh, al = M.share(c.s, ref, set)
      if sh >= M.MIN_SHARE and al > best_al then best, best_sh, best_al = c, sh, al end
    end
  end
  return best, best_sh
end

-- The pieces the job and race may wear in each slot at a level (nil: any), from the index: {slot = {{id, rec, s}}},
-- stats read as they go. Paced.
function M.candidates(index, job_id, race, level)
  local out = {}
  for slot = 0, 15 do
    local list = {}
    for _, id in ipairs(index[slot] or {}) do
      local rec = M.H.item(id)
      if M.fits(rec, slot, job_id, race, level) then
        list[#list + 1] = { id = id, key = id, rec = rec, s = M.stats_of(id) }
      end
      M.pace()
    end
    out[slot] = list
  end
  return out
end

-- Builds one set. ctx: job (e.g. "WAR"), job_id, race, level, owned (list of {id, key, augments}), at_cap and
-- at_level (M.candidates at the cap and at our level), bis (curated {slot = item name or id} or nil), names (name -> id).
-- Returns {slots = {slot = {pick, share, how, ref, worth}}, score}.
function M.build(ctx, set, set_name)
  -- Only pure mages swap weapons (main, sub or range resets TP), and not in melee sets.
  local swap_weapons = ctx.mage and not set.ws and not ctx.melee_sets[set_name]
  local function wanted(slot) return swap_weapons or not WEAPON_SLOTS[slot] end

  -- The owned pieces, wearable now, by slot.
  local owned = {}
  for _, o in ipairs(ctx.owned) do
    local rec = M.H.item(o.id)
    for slot = 0, 15 do
      if wanted(slot) and M.fits(rec, slot, ctx.job_id, ctx.race, ctx.level) then
        owned[slot] = owned[slot] or {}
        table.insert(owned[slot], { id = o.id, key = o.key, rec = rec, s = M.stats_of(o.id, o.augments), owned = o })
      end
    end
  end

  -- The reference per slot at the cap: curated, else the best by score in the whole list.
  local ref = {}
  local cap_cands = {}
  for slot = 0, 15 do
    if wanted(slot) then cap_cands[slot] = ctx.at_cap[slot] end
  end
  local auto = M.best_by_score(cap_cands, set)
  for slot = 0, 15 do
    local want = ctx.bis and ctx.bis[M.SLOT_NAMES[slot]]
    local id = type(want) == "number" and want or nil
    -- A name: of its items (one name may be several levels' versions), the best one wearable at the cap.
    if type(want) == "string" and ctx.names and ctx.names[want] then
      local best = -math.huge
      for _, cand in ipairs(ctx.names[want]) do
        local rec = M.H.item(cand)
        if M.fits(rec, slot, ctx.job_id, ctx.race, ctx.cap) then
          local v = M.score(M.stats_of(cand), set)
          if v > best then id, best = cand, v end
        end
      end
    end
    if id and wanted(slot) then
      ref[slot] = { id = id, s = M.stats_of(id), rec = M.H.item(id) }
    elseif auto[slot] then
      ref[slot] = auto[slot]
    end
  end

  -- The owned picks: the most of the reference, else the set's best.
  local by_score = M.best_by_score(owned, set)
  local out = { slots = {}, score = 0 }
  local used = {}
  for slot = 0, 15 do
    local list = owned[slot]
    local r = ref[slot]
    local pick, share, how
    if list and r and M.score(r.s, set) > 0 then
      pick, share = by_share(list, r.s, set, used)
      how = pick and "bis" or nil
    end
    if not pick and by_score[slot] and not (by_score[slot].key and used[by_score[slot].key]) then
      pick, how = by_score[slot], "score"
      share = r and (M.share(pick.s, r.s, set)) or nil
    end
    if pick and M.score(pick.s, set) <= 0 then pick, how = nil, nil end
    if not pick then share = nil end
    if pick and pick.key then used[pick.key] = true end
    out.slots[slot] = { pick = pick, share = share, how = how, ref = r }
  end

  -- A sub piece that does not go with the picked main (a grip under one hand, a shield under two): the best that does.
  local main = out.slots[0] and out.slots[0].pick
  local sub = out.slots[1]
  if main and sub and sub.pick and not sub_ok(sub.pick.rec, main.rec) then
    local best, bv = nil, 0
    for _, c in ipairs(owned[1] or {}) do
      if sub_ok(c.rec, main.rec) and not (c.key and used[c.key]) then
        local v = M.score(c.s, set)
        if v > bv then best, bv = c, v end
      end
    end
    sub.pick, sub.how = best, best and "score" or nil
  end

  -- Worth getting: from every item wearable at this level, the reference's rule again; shown when it beats the pick.
  for slot = 0, 15 do
    if wanted(slot) and ctx.at_level[slot] then
      local cands = {}
      for _, c in ipairs(ctx.at_level[slot]) do
        if not (ctx.owns and ctx.owns[c.id]) then cands[#cands + 1] = c end
      end
      local o = out.slots[slot]
      local w
      if o.ref and M.score(o.ref.s, set) > 0 then
        w = by_share(cands, o.ref.s, set, {})
      end
      if not w then
        local best, bv = nil, 0
        for _, c in ipairs(cands) do
          local v = M.score(c.s, set)
          if v > bv then best, bv = c, v end
          M.pace()
        end
        w = best
      end
      if w and (not o.pick or M.score(w.s, set) > M.score(o.pick.s, set)) then o.worth = w end
    end
  end

  -- The set's score under its caps, and the reference's, for the window's percentage.
  local picks, refs = {}, {}
  for slot, o in pairs(out.slots) do
    if o.pick then picks[slot] = o.pick end
    if o.ref then refs[slot] = o.ref end
  end
  local function capped(p)
    local t = 0
    for slot, c in pairs(p) do t = t + marginal(c.s, set, totals(p, slot)) end
    return t
  end
  -- The best this level allows: per slot the owned pick or what is worth getting, the better.
  local reach = {}
  for slot, o in pairs(out.slots) do
    local c = o.pick
    if o.worth and (not c or M.score(o.worth.s, set) > M.score(c.s, set)) then c = o.worth end
    if c then reach[slot] = c end
  end
  out.score = capped(picks)
  out.ref_score = capped(refs)
  out.reach_score = capped(reach)
  return out
end

-- The equipment ids by slot, from a list of ids (the whole item list, read a slice at a time by the caller).
function M.index_add(index, ids)
  for _, id in ipairs(ids) do
    M.pace()
    local rec = M.H.item(id)
    if rec and rec.slots and rec.slots ~= 0 and (rec.name or "") ~= "" then
      for slot = 0, 15 do
        if rec.slots & (1 << slot) ~= 0 then
          index[slot] = index[slot] or {}
          table.insert(index[slot], id)
        end
      end
    end
  end
end

return M
