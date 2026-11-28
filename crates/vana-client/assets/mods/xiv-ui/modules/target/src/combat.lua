-- Observed cast/status adapter. Timers remains the authoritative shared tracker.
local X = require("src.xiv")
local observer = require("src.status_observer")
local C = { casts = {}, fallback = {}, last_hit = {} }
local party_cache, party_at, timer_cache = {}, -1, {}
local own_id
local function player_id()
  if not own_id then local me=game.player();if me and type(me.id)=='number' and me.id>0 then own_id=me.id end end
  return own_id
end
for _,event in ipairs({'zone_change','login','logout'}) do events.on(event,function()own_id=nil end) end
local START = 0x6163 -- low half of the action arg for "ca" (a cast/ready begins)
local function refresh_party()
  party_cache = {}; for _, m in ipairs(game.party()) do party_cache[m.id] = m end
  party_at = clock()
end
events.on("party_change", function() party_at = -1 end)
events.on("party_buffs", function() party_at = -1 end)
local removed = { [64] = true, [83] = true, [123] = true, [159] = true, [168] = true,
  [204] = true, [206] = true, [321] = true, [322] = true, [341] = true, [342] = true,
  [343] = true, [344] = true, [350] = true, [378] = true, [453] = true, [531] = true,
  [647] = true, [805] = true, [806] = true }
local damage = { [1] = true, [2] = true, [67] = true, [110] = true, [185] = true, [252] = true }
local sleep = { [2] = true, [19] = true, [193] = true }
local function started(param)
  return type(param) == "number" and param % 65536 == START
end
local function ability_name(id)
  if not X.id(id) then return nil end
  if id >= 256 and res.mob_skill then
    local name = res.mob_skill(id)
    if name and name ~= "" then return name end
  end
  local ability = res.ability and res.ability(id)
  return ability and ability.name or nil
end
local function cast_resource(value)
  if type(value)~='number' or value<0 or value>4294967295 or value%1~=0 then return nil end
  local id=value%65536 -- native track_cast casts start-result value to u16
  return X.id(id) and id or nil
end
events.on("action", function(a)
  local t = a.targets and a.targets[1]
  local r = t and t.results and t.results[1]
  if a.category == 8 then
    C.casts[a.actor] = nil -- a new unresolved start cannot leave an old skill label
    if not started(a.param) then C.casts[a.actor] = nil
    elseif r and cast_resource(r.value) then
      local id=cast_resource(r.value)
      local spell = res.spell(id)
      if spell and (spell.cast_time or 0) > 0 then
        C.casts[a.actor] = { spell = id, name = spell.name or "Casting",
          start = clock(), total = spell.cast_time }
      end
    end
  elseif a.category == 7 then
    -- Category 7's result value is the ability's name-table id; its text is
    -- only the generic "readies" line and must not be used as the label.
    if started(a.param) then
      local name = r and ability_name(cast_resource(r.value))
      C.casts[a.actor] = { name = (name and name ~= "" and text.strip(name)) or "Readying ability",
        start = clock(), total = 10, note = true }
    else C.casts[a.actor] = nil end
  elseif a.category == 3 or a.category == 4 or a.category == 6 or a.category == 11
      or a.category == 12 then C.casts[a.actor] = nil end
  local hit_recorded = false
  for _, tg in ipairs(a.targets or {}) do
    timer_cache[tg.id] = nil
    -- Self-contained last-hit tracker for the enemy target-of-target fallback (no Aggro dependency).
    if not hit_recorded and type(a.actor)=='number' and a.actor>0 and type(tg.id)=='number' and tg.id>0
        and type(tg.results)=='table' and #tg.results>0 then
      C.last_hit[a.actor] = { id = tg.id, at = clock() }
      hit_recorded = true
    end
    for _, rr in ipairs(tg.results or {}) do
      local message, value = rr.message, rr.value
      if removed[message] and C.fallback[tg.id] and X.id(value) and value > 0 then
        C.fallback[tg.id][value] = nil
      elseif damage[message] and C.fallback[tg.id] then
        for id in pairs(sleep) do C.fallback[tg.id][id] = nil end
      end
      if rr.proc and rr.proc.message==168 and X.id(rr.proc.value) and C.fallback[tg.id] then
        C.fallback[tg.id][rr.proc.value]=nil
      end
      for _,id in ipairs(observer.effects(a,tg,rr)) do
        C.fallback[tg.id]=C.fallback[tg.id] or {}
        for _,ended in ipairs(observer.ends(id)) do C.fallback[tg.id][ended]=nil end
        C.fallback[tg.id][id]=clock()
      end
    end
  end
end)
events.on("battle_message", function(actor, target, message, param)
  message=(tonumber(message) or 0)%32768 -- same high-bit flag handling as native Timers
  timer_cache[target]=nil
  if message == 204 or message == 206 then
    if C.fallback[target] and X.id(param) then C.fallback[target][param] = nil end
  elseif message == 6 or message == 20 or message == 97 then C.fallback[target], C.casts[target] = nil, nil end
  if message == 16 or message == 29 or message == 84 then C.casts[actor] = nil end
end)
local function tracked_on(id)
  local cache=timer_cache[id]
  if not cache or clock()-cache.at>=0.2 then
    cache={at=clock(),rows=X.call('timers','effects_on',id)};timer_cache[id]=cache
  end
  for _,e in ipairs(cache.rows or {}) do
    -- Once a timed effect is owned by Timers, permanently retire the matching
    -- unknown fallback. Its later expiry must not resurrect an untimed icon.
    if (e.left~=nil or e.secs~=nil) and C.fallback[id] then C.fallback[id][e.status]=nil end
  end
  return cache
end
local poll_at=1
timer.every(0.25,function()
  -- Hand over off-target effects too, with a bounded number of provider calls.
  local ids={};for id in pairs(C.fallback) do ids[#ids+1]=id end;table.sort(ids)
  for _=1,math.min(8,#ids) do
    if poll_at>#ids then poll_at=1 end
    tracked_on(ids[poll_at]);poll_at=poll_at+1
  end
end)
events.on("zone_change", function() C.casts, C.fallback, C.last_hit, party_cache, timer_cache = {}, {}, {}, {}, {}; party_at = -1 end)
timer.every(1, function()
  for id, c in pairs(timer_cache) do if clock() - c.at > 30 then timer_cache[id] = nil end end
  for id, c in pairs(C.casts) do if clock() - c.start > (c.total or 5) + 2 then C.casts[id] = nil end end
  for actor, lh in pairs(C.last_hit) do if clock() - lh.at > 8 then C.last_hit[actor] = nil end end
  for id, statuses in pairs(C.fallback) do
    for status, at in pairs(statuses) do if clock() - at > 600 then statuses[status] = nil end end
    if not next(statuses) then C.fallback[id] = nil end
  end
end)
function C.cast(id)
  if id == player_id() then
    local c = game.cast()
    if c and not c.event then
      local spell = X.id(c.spell) and res.spell(c.spell)
      return { name = spell and spell.name or "Casting", progress = c.progress or 0,
        left = c.left, source = "Client cast state" }
    end
    return nil
  end
  local c = C.casts[id]
  if not c then return nil end
  local elapsed = clock() - c.start
  if c.note then
    if elapsed > 10 then C.casts[id] = nil; return nil end
    return { name = c.name, progress = 0, source = "Observed readying; duration unknown" }
  end
  if elapsed > c.total + 2 then C.casts[id] = nil; return nil end
  return { name = c.name, progress = math.min(0.99, elapsed / c.total),
    left = math.max(0, c.total - elapsed), source = "Estimated from observed spell start" }
end
function C.effects(id)
  if player_id() == id then
    local list = {}; for _, e in ipairs(game.buffs() or {}) do
      list[#list + 1] = { id = e.id, remaining = e.remaining, source = "Client status state" }
    end; return list
  end
  if party_at < 0 or clock() - party_at >= 0.1 then refresh_party() end
  local m = party_cache[id]
  if m and m.in_zone == false then return {} end
  local entity=not m and game.entity(id)
  if entity and (entity.status==2 or entity.status==3) then
    C.fallback[id],C.casts[id],timer_cache[id]=nil,nil,nil;return {}
  end
  local cache = tracked_on(id)
  local tracked, fallback = cache.rows, C.fallback[id]
  local out, seen = {}, {}
  local function add(row)
    if not row or not X.id(row.id) or seen[row.id] then return end
    seen[row.id] = true; out[#out + 1] = row
  end
  if m then
    -- Party state is the authoritative list; Timers only enriches durations.
    local by_id = {}; for _, e in ipairs(tracked or {}) do by_id[e.status] = e end
    for _, status in ipairs(m.buffs or {}) do
      local e = by_id[status]
      local left=e and e.left and e.left-(clock()-cache.at)
      add({ id=status,name=e and e.name,secs=e and e.secs,
        left=left and left>0 and left or nil,
        source=left and left>0 and 'Tracked duration (estimated)' or 'Party status state; duration unknown' })
    end
    -- The actual party list is authoritative, even when empty. Never merge an
    -- old action fallback back into removed party icons.
    C.fallback[id]=nil
    return out
  elseif tracked then
    for _, e in ipairs(tracked) do
      add({ id = e.status, name = e.name, secs = e.secs,
        left = e.left and e.left - (clock() - cache.at),
        source = e.left and "Tracked duration (estimated)" or "Observed; duration unknown" })
    end
  end
  -- An empty Timers result must not hide effects we saw land ourselves. Merge
  -- both sources and de-duplicate by status id.
  for status, at in pairs(fallback or {}) do
    if clock() - at <= 600 then
      add({ id = status, name = res.status(status) or ("Status " .. status),
        source = "Observed action result; duration unknown" })
    else fallback[status] = nil end
  end
  return out
end
function C.casts_for(ids)
  local out,seen={},{}
  for i,id in ipairs(ids or {}) do
    if i>24 then break end
    if type(id)=='number' and id>0 and id<=4294967295 and id%1==0 and not seen[id] then
      seen[id]=true;out[id]=C.cast(id) or false
    end
  end
  return out
end
return C
