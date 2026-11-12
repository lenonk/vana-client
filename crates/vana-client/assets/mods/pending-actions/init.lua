-- Pending actions (modern UI): a spell, weapon skill, job ability or item we asked for (c2s 0x01A / 0x037) waits
-- until the server answers. An action of ours of its kind and id confirms it (a melee round or another action's
-- does not); a battle message of ours soon after the ask that is one of the request path's refusals for its kind,
-- naming the asked id where the server sends one, is its refusal, kept a few seconds; no answer for a while says so.
-- The window stands in for the "pending" panel.

local WIN = "pending"
local SHOW_AFTER, TIMEOUT, REFUSED_WITHIN, KEEP = 0.15, 5, 2, 4
-- The 0x028 categories that answer each kind: its start or its finish (monster skills finish as 11).
local ANSWERS = { spell = { [4] = true, [8] = true }, ws = { [3] = true, [7] = true, [11] = true },
  ability = { [6] = true, [14] = true, [15] = true }, item = { [5] = true, [9] = true } }
local KIND = { [3] = "spell", [7] = "ws", [9] = "ability" }
-- 0x028 categories whose param names the action (finishes), and starts whose first record value does ("ca..").
local FINISH = { [3] = true, [4] = true, [5] = true, [6] = true, [11] = true, [13] = true, [14] = true, [15] = true }
local START = { [7] = true, [8] = true, [9] = true, [10] = true, [12] = true }
-- The refusals the server's request path sends for each kind (LSB): its controllers and action states, the target
-- check every kind goes through (12 already claimed, 155, 446) and the game scripts' checks (onMagicCastingCheck,
-- onAbilityCheck, onItemCheck: "You cannot learn...", "while medicated", "requires a pet" and the rest).
-- A spell's carry its id, except the target check's and its recast's 18 (an interrupt's 16 too): none. Weapon
-- skills' carry none. An ability's carry none or its param. An item's carry none or the item id, except a scroll's
-- 95 / 96 (cannot learn, already known), which carry the spell it teaches, and the 0 a status that stops the item
-- (stun, sleep...) sends with the item id.
local SPELL_BARE = { [12] = true, [16] = true, [18] = true, [155] = true, [446] = true }
local SPELL_NAMED = { [34] = true, [35] = true, [40] = true, [47] = true, [48] = true, [49] = true, [71] = true,
  [72] = true, [78] = true, [155] = true, [235] = true, [313] = true, [315] = true, [316] = true, [569] = true,
  [581] = true, [661] = true, [700] = true, [717] = true }
local WS_REFUSALS = { [12] = true, [71] = true, [89] = true, [155] = true, [190] = true, [191] = true, [192] = true,
  [216] = true, [217] = true, [446] = true }
local ABILITY_REFUSALS = { [4] = true, [5] = true, [12] = true, [71] = true, [72] = true, [78] = true, [87] = true,
  [88] = true, [94] = true, [154] = true, [155] = true, [192] = true, [199] = true, [215] = true, [216] = true,
  [235] = true, [307] = true, [315] = true, [316] = true, [328] = true, [336] = true, [337] = true, [347] = true,
  [356] = true, [428] = true, [429] = true, [446] = true, [523] = true, [524] = true, [525] = true, [561] = true,
  [574] = true, [662] = true, [666] = true, [745] = true }
local ITEM_REFUSALS = { [12] = true, [55] = true, [56] = true, [62] = true, [78] = true, [92] = true, [111] = true,
  [155] = true, [215] = true, [246] = true, [247] = true, [308] = true, [316] = true, [356] = true, [410] = true,
  [443] = true, [444] = true, [445] = true, [446] = true, [514] = true, [517] = true, [518] = true, [523] = true,
  [580] = true, [759] = true }
local LEARN = { [95] = true, [96] = true }
-- The system message (0x053) the target check sends instead when a player blocks outside help.
local BLOCKING = 225

local asked

local function me() return (game.player() or {}).id end

local function start(kind, id, target)
  local now = os.clock()
  asked = { kind = kind, id = id, target = target, at = now, state = "waiting", since = now }
end

-- c2s 0x01A: target @4, category @0x0a, param @0x0c (abilities: name-table id - 0x200).
packets.on_out(0x01A, function(p)
  local d = p.data
  if #d < 0x10 then return end
  local target, _, cat, param = string.unpack("<I4I2I2I4", d, 5)
  local kind = KIND[cat]
  if kind then start(kind, kind == "ability" and param + 0x200 or param, target) end
end)

-- c2s 0x037: target @4, the item's index @0x0e and bag @0x10.
packets.on_out(0x037, function(p)
  local d = p.data
  if #d < 0x14 then return end
  local target = string.unpack("<I4", d, 5)
  local index, bag = string.unpack("B", d, 0x0e + 1), string.unpack("<I4", d, 0x10 + 1)
  for _, it in ipairs(game.items(bag)) do
    if it.slot == index then start("item", it.id, target) return end
  end
end)

-- The id the server's packets carry for an ask: an ability's 0x01A param (name-table id - 0x200), else the id asked.
local function packet_id(a) return a.kind == "ability" and a.id - 0x200 or a.id end

-- The action id a 0x028 names: a finish's param, a "ca.." start's first record value (an interrupt's "sp.." and
-- melee or ranged rounds name none).
local function action_id(act)
  if FINISH[act.category] then return act.param & 0xffff end
  if START[act.category] and act.param & 0xffff == 0x6163 then
    local t = act.targets and act.targets[1]
    local r = t and t.results and t.results[1]
    return r and r.value & 0xffff
  end
end

-- Whether a 0x028 of ours answers the ask: its kind, and where the packet names an action, the asked one. Items and
-- monster skills (a mob skill id, not a weapon skill's) go by the category alone.
local function answers(a, act)
  if not ANSWERS[a.kind][act.category] then return false end
  if a.kind == "item" or (a.kind == "ws" and act.category == 11) then return true end
  return action_id(act) == packet_id(a)
end

-- Whether a battle message of ours (its id and first parameter) is the ask's refusal.
local function refusal(a, id, param)
  local named = param == packet_id(a)
  if a.kind == "spell" then
    return (SPELL_BARE[id] == true and param == 0) or (SPELL_NAMED[id] == true and named)
  elseif a.kind == "ws" then
    return WS_REFUSALS[id] == true and param == 0
  elseif a.kind == "ability" then
    return ABILITY_REFUSALS[id] == true and (param == 0 or named)
  end
  return LEARN[id] == true or (id == 0 and named) or (ITEM_REFUSALS[id] == true and (param == 0 or named))
end

events.on("action", function(a)
  if asked and asked.state == "waiting" and a.actor == me() and answers(asked, a) then asked = nil end
end)

local function refuse(why)
  asked.state, asked.since, asked.why = "refused", os.clock(), why
end

-- Whether the ask still waits for its answer and a refusal now would be one.
local function refusable() return asked and asked.state == "waiting" and os.clock() - asked.at < REFUSED_WITHIN end

events.on("battle_message", function(actor, target, message, p1, p2)
  if not refusable() or actor ~= me() or not refusal(asked, message, p1) then return end
  local m = res.message(message, { actor = actor, target = target, values = { p1, p2 } })
  refuse(m and text.strip(m.text):gsub("\n", " "):match("^%s*(.-)%s*$") or "")
end)

-- s2c 0x053: its number @0x0c. A weapon skill only targets monsters, which never block help.
packets.on_in(0x053, function(p)
  local d = p.data
  if #d < 0x0e or not refusable() or asked.kind == "ws" then return end
  if string.unpack("<I2", d, 0x0c + 1) == BLOCKING then refuse("") end
end)

local function name_of(a)
  local r
  if a.kind == "spell" then r = res.spell(a.id) elseif a.kind == "item" then r = res.item(a.id) else r = res.ability(a.id) end
  return r and r.name or ""
end

local function view()
  if not asked then return nil end
  local now = os.clock()
  if asked.state == "waiting" and now - asked.at > TIMEOUT then
    asked.state, asked.since = "quiet", now
  elseif asked.state ~= "waiting" and now - asked.since > KEEP then
    asked = nil
    return nil
  end
  if asked.state == "waiting" and now - asked.at < SHOW_AFTER then return nil end
  local on
  if asked.target ~= 0 and asked.target ~= me() then
    local e = game.entity(asked.target)
    on = e and e.name ~= "" and e.name or nil
  end
  local text = name_of(asked) .. (on and (" \u{2192} " .. on) or "")
  if asked.state == "waiting" then
    return { text = text, note = "waiting" .. ("."):rep(1 + math.floor((now - asked.at) * 3) % 3), tone = "waiting", detail = "" }
  elseif asked.state == "refused" then
    return { text = text, note = "refused", tone = "refused", detail = asked.why or "" }
  end
  return { text = text, note = "no reply", tone = "quiet", detail = "The server has not answered." }
end

local shown
events.on("frame", function()
  local v = settings.get("on") and view() or nil
  if not v then
    if shown then ui.close(WIN) shown = nil end
    return
  end
  local key = v.text .. "|" .. v.note .. "|" .. v.tone .. "|" .. v.detail
  if key == shown then return end
  if shown then
    ui.update(WIN, "chip", v)
  else
    v.type, v.id = "chip", "chip"
    ui.window(WIN, { panel = "pending", hud = true, width = 300, elements = { v } })
    ui.open(WIN)
  end
  shown = key
end)
