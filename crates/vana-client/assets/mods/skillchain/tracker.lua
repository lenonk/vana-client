-- Each monster's resonance as the action results show the party's skills land, by LSB's GetSkillChainEffect: an
-- opener leaves its properties for 10 s, a chain its properties for 1 s less per link, and a closer counts from 3 s after
-- the last. Plus the skills each party member was seen using, who each monster last went for, and what chains the
-- party can make with all it has.

local SC = require("sc")

local T = {
  on = {},    -- mob id -> {props, chain, start, life, links, by, skill}
  seen = {},  -- actor id -> skills seen used, newest last
  hits = {},  -- mob id -> {target, at}
}

-- A closer counts this long after the resonance started (LSB: start + 3 s < now).
T.WAIT = 3.0
-- An opener's resonance lasts this long; each link takes a second off.
T.LIFE = 10.0
-- Skills remembered per member.
local SEEN = 16

local now = os.clock

function T.age(r) return now() - r.start end
function T.left(r) return r.life - T.age(r) end

-- The properties a closer pairs against.
function T.against(r)
  if r.chain then return { r.chain } end
  local out = {}
  for _, p in ipairs(r.props) do
    if p ~= 0 then out[#out + 1] = p end
  end
  return out
end

-- LSB's ActionProcSkillChain (the proc kind of a skill's record) as the chain's property.
local PROC_CHAIN = { 13, 14, 9, 12, 10, 11, 2, 3, 7, 5, 1, 4, 6, 8, 15, 16 }

-- The skill an action finished, if it can carry properties.
local function skill_of(a)
  local id = a.param
  if a.category == 3 then return { kind = "weapon", id = id } end
  if a.category == 11 or a.category == 13 then return { kind = "mob", id = id } end
  if a.category == 4 and SC.data().blue[id] then return { kind = "blue", id = id } end
end

-- The party (our six), by id.
local function party_ids()
  local out = {}
  for _, m in ipairs(game.party()) do
    if m.party == 0 then out[m.id] = m end
  end
  return out
end

-- Us, a party member (trusts included) or a pet of either.
local function ours(actor, me, party)
  if actor == me.id or party[actor] then return true end
  local e = game.entity(actor)
  local master = e and e.master or 0
  if master == 0 then return false end
  if master == me.index then return true end
  for id in pairs(party) do
    local pe = game.entity(id)
    if pe and pe.index == master then return true end
  end
  return false
end

-- The action's records: one target's results, or each target's first.
local function records(a)
  local out = {}
  if #a.targets == 1 then
    for _, r in ipairs(a.targets[1].results) do out[#out + 1] = { a.targets[1].id, r } end
  else
    for _, t in ipairs(a.targets) do
      if t.results[1] then out[#out + 1] = { t.id, t.results[1] } end
    end
  end
  return out
end

local function is_mob(id)
  local e = game.entity(id)
  return e ~= nil and e.kind == "mob"
end

-- An action: a party skill that hit a monster opens, continues or replaces its resonance.
function T.on_action(a)
  local me, party = game.player(), party_ids()
  local recs = records(a)
  if (a.category == 1 or a.category == 4 or a.category == 11) and is_mob(a.actor) and recs[1]
      and ours(recs[1][1], me, party) then
    T.hits[a.actor] = { recs[1][1], now() }
  end
  local skill = skill_of(a)
  if not skill or not ours(a.actor, me, party) then return end
  local props = SC.props(skill)
  local seen = T.seen[a.actor] or {}
  T.seen[a.actor] = seen
  for i = #seen, 1, -1 do
    if seen[i].kind == skill.kind and seen[i].id == skill.id then table.remove(seen, i) end
  end
  seen[#seen + 1] = skill
  if #seen > SEEN then table.remove(seen, 1) end
  for _, rec in ipairs(recs) do
    local target, r = rec[1], rec[2]
    if r.miss == 0 and not ours(target, me, party) and is_mob(target) then
      local chain = r.proc and PROC_CHAIN[r.proc.kind] or nil
      local old = T.on[target]
      local res
      if chain then
        local life = ((old and T.left(old) > 0) and old.life or T.LIFE) - 1
        res = { props = { chain, 0, 0 }, chain = chain, start = now(), life = life,
          links = (old and old.links or 0) + 1, by = a.actor, skill = skill }
      elseif props[1] ~= 0 or props[2] ~= 0 or props[3] ~= 0 then
        res = { props = props, start = now(), life = T.LIFE, links = 0, by = a.actor, skill = skill }
      end
      if res then T.on[target] = res end
    end
  end
end

-- Now and then: resonances that ran out or whose monster went.
function T.tick()
  for id, r in pairs(T.on) do
    local e = game.entity(id)
    if T.left(r) <= -1 or not e or e.hpp <= 0 then T.on[id] = nil end
  end
  for id, h in pairs(T.hits) do
    if now() - h[2] >= 30 or not game.entity(id) then T.hits[id] = nil end
  end
end

-- A monster's last party target, while recent.
function T.hitting(mob)
  local h = T.hits[mob]
  if h and now() - h[2] < 8 then return h[1] end
end

-- A skill's name: the game's for weapon skills (and mob skills in that range), else LSB's.
function T.name(s)
  if s.kind == "weapon" or (s.kind == "mob" and s.id < 256) then
    local a = res.ability(s.id)
    if a and a.name and a.name ~= "" then return a.name end
  end
  if s.kind == "blue" then
    local sp = res.spell(s.id)
    return sp and sp.name or ""
  end
  local e = SC.entry(s)
  return e and SC.pretty(e.name) or ""
end

-- The party (us first) with the skills they have: ours from our list, a trust's from its list, anyone's as seen.
function T.members()
  local me = game.player()
  local out = {}
  local mine = {}
  for _, id in ipairs(game.abilities()) do
    local a = res.ability(id)
    if a and a.kind == 3 then
      local s = { kind = "weapon", id = id }
      if SC.has_props(s) then mine[#mine + 1] = s end
    end
  end
  out[1] = { id = me.id, name = me.name, me = true, tp = me.tp or 0, skills = mine }
  for _, m in ipairs(game.party()) do
    if m.party == 0 and m.id ~= me.id then
      local skills = {}
      local e = game.entity(m.id)
      local ids = e and e.trust and SC.trust(m.name) or nil
      for _, i in ipairs(ids or {}) do skills[#skills + 1] = { kind = "mob", id = i } end
      for _, s in ipairs(T.seen[m.id] or {}) do
        local dup = false
        for _, x in ipairs(skills) do
          if x.kind == s.kind and x.id == s.id then dup = true end
        end
        if not dup then skills[#skills + 1] = s end
      end
      -- A trust's list can hold one skill under several ids (its level variants).
      local kept, names = {}, {}
      for _, s in ipairs(skills) do
        if SC.has_props(s) then
          local n = T.name(s)
          if not names[n] then
            names[n] = true
            kept[#kept + 1] = s
          end
        end
      end
      out[#out + 1] = { id = m.id, name = m.name, me = false, tp = m.tp or 0, skills = kept }
    end
  end
  return out
end

-- Every chain two different members can make: {opener = {k, skill}, closer = {k, skill}, chain}, the strongest
-- first, then those with us in them; per pair of members and chain only the first found. Kept until a member's skills
-- change.
local kept_key, kept_combos
function T.combos(members)
  local parts = {}
  for _, m in ipairs(members) do
    local ks = {}
    for _, s in ipairs(m.skills) do ks[#ks + 1] = SC.key(s) end
    parts[#parts + 1] = table.concat(ks, ",")
  end
  local key = table.concat(parts, "|")
  if key == kept_key then return kept_combos end
  local out, seen = {}, {}
  for a, ma in ipairs(members) do
    for b, mb in ipairs(members) do
      if a ~= b then
        for _, sa in ipairs(ma.skills) do
          local pa = SC.props(sa)
          for _, sb in ipairs(mb.skills) do
            local chain = SC.form(pa, SC.props(sb))
            local k = a .. ":" .. b .. ":" .. tostring(chain)
            if chain and not seen[k] then
              seen[k] = true
              out[#out + 1] = { opener = { a, sa }, closer = { b, sb }, chain = chain, n = #out + 1 }
            end
          end
        end
      end
    end
  end
  table.sort(out, function(x, y)
    local lx, ly = SC.level(x.chain), SC.level(y.chain)
    if lx ~= ly then return lx > ly end
    local mx, my = math.min(x.opener[1], x.closer[1]), math.min(y.opener[1], y.closer[1])
    if mx ~= my then return mx < my end
    if x.opener[1] ~= y.opener[1] then return x.opener[1] < y.opener[1] end
    if x.closer[1] ~= y.closer[1] then return x.closer[1] < y.closer[1] end
    return x.n < y.n
  end)
  for i, x in ipairs(out) do x.pos = i end
  kept_key, kept_combos = key, out
  return out
end

local KIND_ORDER = { weapon = 0, mob = 1, blue = 2 }

-- What closes a resonance: {k, skill, chain}, the strongest first.
function T.closers(members, r)
  local against = T.against(r)
  local out = {}
  for k, m in ipairs(members) do
    for _, s in ipairs(m.skills) do
      local c = SC.form(against, SC.props(s))
      if c then out[#out + 1] = { k, s, c } end
    end
  end
  table.sort(out, function(x, y)
    local lx, ly = SC.level(x[3]), SC.level(y[3])
    if lx ~= ly then return lx > ly end
    if x[1] ~= y[1] then return x[1] < y[1] end
    if x[2].kind ~= y[2].kind then return KIND_ORDER[x[2].kind] < KIND_ORDER[y[2].kind] end
    return x[2].id < y[2].id
  end)
  return out
end

-- The monster the helper follows: our target when it is a monster, else the one resonating most recently.
function T.focus()
  local t = game.player().target
  if t and is_mob(t) then return t end
  local best, at
  for id, r in pairs(T.on) do
    if not at or r.start > at then best, at = id, r.start end
  end
  return best
end

-- The open resonance on the focus monster: {mob, props, chain, by, age, left, life, closers = {{who, skill, chain,
-- ready}}}, and every chain the party can make: {chain, ways = {{opener = {who, skill}, closer = {who, skill},
-- ready, chain}}}; nil out of a party of two or more. `members` and `combos` as T.members / T.combos made them.
function T.view(members, combos)
  members = members or T.members()
  if #members < 2 then return nil end
  local function who(k) return members[k].me and "You" or members[k].name end
  local function ready(k) return members[k].tp >= 1000 end
  local focus = T.focus()
  local v = { chains = {} }
  local r = focus and T.on[focus]
  if r and T.left(r) > 0 then
    local e = game.entity(focus)
    local by = ""
    if r.by == game.player().id then
      by = "You"
    else
      for _, m in ipairs(game.party()) do
        if m.id == r.by then by = m.name end
      end
    end
    local props = {}
    for _, p in ipairs(r.props) do
      if p ~= 0 then props[#props + 1] = p end
    end
    local closers = {}
    for _, c in ipairs(T.closers(members, r)) do
      closers[#closers + 1] = { who = who(c[1]), skill = T.name(c[2]), chain = c[3], ready = ready(c[1]) }
    end
    v.now = {
      mob = e and e.name or "", props = props, chain = r.chain,
      by = by == "" and T.name(r.skill) or (by .. ": " .. T.name(r.skill)),
      age = T.age(r), left = T.left(r), life = r.life, closers = closers,
    }
  end
  combos = combos or T.combos(members)
  local done = {}
  for _, cb in ipairs(combos) do
    local fam = SC.family(cb.chain)
    if not done[fam] then
      done[fam] = true
      local ways = {}
      for _, x in ipairs(combos) do
        if SC.family(x.chain) == fam then ways[#ways + 1] = x end
      end
      table.sort(ways, function(x, y)
        local lx, ly = SC.level(x.chain), SC.level(y.chain)
        if lx ~= ly then return lx > ly end
        local ux = members[x.opener[1]].me or members[x.closer[1]].me
        local uy = members[y.opener[1]].me or members[y.closer[1]].me
        if ux ~= uy then return ux end
        local rx, ry = ready(x.opener[1]), ready(y.opener[1])
        if rx ~= ry then return rx end
        return x.pos < y.pos
      end)
      local out = {}
      for _, x in ipairs(ways) do
        out[#out + 1] = { opener = { who(x.opener[1]), T.name(x.opener[2]) },
          closer = { who(x.closer[1]), T.name(x.closer[2]) }, ready = ready(x.opener[1]), chain = x.chain }
      end
      v.chains[#v.chains + 1] = { chain = fam, ways = out }
    end
  end
  return v, members, combos
end

return T
