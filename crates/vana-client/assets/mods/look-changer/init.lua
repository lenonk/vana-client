-- //look (the dress-up add-ons): how characters are drawn on this screen only (render.look). Our race, face and
-- gear by slot, a target's look copied, one race for every other player. Kept per character.

local models = require("src.models")

local function say(t) chat.add("Look Changer: " .. t) end

local RACES = {
  ["hume m"] = 1, ["hume male"] = 1, ["hume f"] = 2, ["hume female"] = 2,
  ["elvaan m"] = 3, ["elvaan male"] = 3, ["elvaan f"] = 4, ["elvaan female"] = 4,
  ["tarutaru m"] = 5, ["tarutaru male"] = 5, ["taru m"] = 5, ["tarutaru f"] = 6, ["tarutaru female"] = 6, ["taru f"] = 6,
  mithra = 7, galka = 8,
}
local SLOTS = { head = 4, body = 5, hands = 6, legs = 7, feet = 8, main = 0, sub = 1, range = 2 }
-- bare: the game's defaults for an empty armour slot (head 0, the rest 8); no weapon is 0
local BARE = { head = 0, body = 8, hands = 8, legs = 8, feet = 8, main = 0, sub = 0, range = 0 }
local ORDER = { "race", "face", "head", "body", "hands", "legs", "feet", "main", "sub", "range" }

local function looks()
  local s = storage.character
  s.looks = s.looks or { me = {} }
  s.looks.me = s.looks.me or {}
  return s.looks
end

-- Item ids by lower-case name, built on first use a thousand at a time (a frame between).
local by_name
local function item_named(name)
  if not by_name then
    local index, n = {}, 0
    for id in pairs(models) do
      local nm = res.name("item", id)
      if nm then index[nm:lower()] = index[nm:lower()] or id end
      n = n + 1
      if n % 1000 == 0 then sleep(0.01) end
    end
    by_name = index
  end
  return by_name[name:lower()]
end

-- what each character was last given, as a key
local applied = {}

local function key(parts)
  if not parts then return "" end
  local out = {}
  for _, k in ipairs(ORDER) do out[#out + 1] = tostring(parts[k]) end
  return table.concat(out, ",")
end

local function want(parts)
  for _, k in ipairs(ORDER) do
    if parts[k] ~= nil then return parts end
  end
end

-- Every character in sight drawn as kept: ours, then the others' race.
local function apply()
  if not settings.get("on") then return end
  local l = looks()
  local me = game.player()
  if not me then return end
  local seen = {}
  for _, e in ipairs(game.entities({ kinds = { "player" } })) do
    local parts = e.id == me.id and want(l.me) or (not e.me and l.others and { race = l.others }) or nil
    seen[e.id] = true
    local k = key(parts)
    if k ~= (applied[e.id] or "") then
      render.look(e.id, parts)
      applied[e.id] = k
    end
  end
  for id in pairs(applied) do
    if not seen[id] then applied[id] = nil end
  end
end

local function reapply()
  applied = {}
  render.clear()
  apply()
end

-- "1a".."8b": faces 1..8, hair a / b.
local function face_of(s)
  local n, h = s:lower():match("^([1-8])([ab])$")
  if not n then return nil end
  return (tonumber(n) - 1) * 2 + (h == "b" and 1 or 0)
end

local function copy_from(e)
  if not (e and e.look) then return false end
  local l = looks()
  l.me = { race = e.race, face = e.look.face }
  for k in pairs(SLOTS) do l.me[k] = e.look[k] end
  return true
end

-- The forms of //look as mod.toml declares them: what the chat line's hint and //mods show.
local function usage()
  chat.add("Look Changer:")
  for _, line in ipairs(commands.usage("look")) do chat.add("  " .. line) end
end

commands.register("look", function(...)
  if not settings.get("on") then return end
  local args = { ... }
  local what = (args[1] or ""):lower()
  local rest = table.concat(args, " ", 2)
  local l = looks()
  if what == "race" then
    local r = RACES[rest:lower()]
    if not r then say("races: hume m / f, elvaan m / f, tarutaru m / f, mithra, galka.") return end
    l.me.race = r
  elseif what == "face" then
    local f = face_of(rest)
    if not f then say("faces are 1a .. 8b.") return end
    l.me.face = f
  elseif SLOTS[what] then
    local w = rest:lower()
    if w == "own" or w == "" then
      l.me[what] = nil
    elseif w == "none" then
      l.me[what] = BARE[what]
    else
      local id = item_named(rest)
      local r = id and res.item(id)
      if not r then say("no gear named " .. rest .. ".") return end
      if (r.slots or 0) & (1 << SLOTS[what]) == 0 then say(r.name .. " is not worn in " .. what .. ".") return end
      l.me[what] = models[id]
    end
  elseif what == "copy" then
    local e
    if rest ~= "" then
      for _, x in ipairs(game.entities()) do
        if x.name:lower() == rest:lower() and x.look then e = x break end
      end
    else
      local p = game.player()
      e = p and p.target and game.entity(p.target)
    end
    if not copy_from(e) then say("target someone drawn in gear, or give their name.") return end
    say("wearing " .. e.name .. "'s look.")
  elseif what == "others" then
    local w = rest:lower()
    if w == "own" or w == "" then
      l.others = nil
    else
      local r = RACES[w]
      if not r then say("races: hume m / f, elvaan m / f, tarutaru m / f, mithra, galka.") return end
      l.others = r
    end
  elseif what == "reset" then
    l.me, l.others = {}, nil
  else
    usage()
    return
  end
  reapply()
end)

events.on("load", reapply)
events.on("zone_change", reapply)
events.on("settings_changed", function()
  if settings.get("on") then reapply() else render.clear() applied = {} end
end)
timer.every(1, apply)
