-- The skillchain helper (the skillchain add-ons): in a party, the chains the party can make together; on our target,
-- an open resonance, what closes it, and when to magic burst. The tracking (tracker.lua) and LandSandBoat's
-- properties (sc.lua, data/skillchains.txt) are this mod's; it draws in the helper's edge window (panel
-- "skillchains": its place, fold and /uimodes; beside the Timers in the classic UI). Other mods read its facts through
-- its services (facts, weapon_skills, rules).

local SC = require("sc")
local T = require("tracker")

local open = {}          -- chain -> its ways shown (the modern view's folds)
local members, combos    -- kept a half second (they need the ability lists)
local members_at = -math.huge
local shown = nil        -- the lines last sent
local made = false
-- An older client knows no lines element: it draws its view in the bare window, as before.
local legacy = false

local function level_tone(c)
  local l = SC.level(c)
  if l == 3 or l == 4 then return "text" end
  if l == 2 then return "gold" end
  return "bronze"
end

local function chain_span(c, strong)
  return { text = SC.NAMES[c], tone = level_tone(c), strong = strong, elements = SC.elements(c) }
end

-- "You: Blade: Ten  ->  Ayame: Tachi: Kasha", a level-4 Light / Darkness marked.
local function way_text(w)
  local lv4 = SC.level(w.chain) == 4 and "  (level 4)" or ""
  return ("%s: %s  \u{2192}  %s: %s%s"):format(w.opener[1], w.opener[2], w.closer[1], w.closer[2], lv4)
end

-- The modern view: the monster and its clock, the chain made or the properties open, the bar, who did it, what
-- closes it; then each chain the party can make, folded to its heading until clicked.
local function modern_lines(v)
  local out = {}
  local n = v.now
  if n then
    local waiting = n.age < T.WAIT
    local clock = waiting and ("wait %.1f s"):format(T.WAIT - n.age) or ("%.1f s left"):format(n.left)
    out[#out + 1] = { spans = { { text = n.mob, tone = "muted" } }, right = { text = clock, tone = waiting and "muted" or "gold" } }
    if n.chain then
      out[#out + 1] = { spans = { { text = "Skillchain:", tone = "gold", strong = true }, chain_span(n.chain, true) } }
      local els = {}
      for _, e in ipairs(SC.elements(n.chain)) do els[#els + 1] = SC.ELEMENT_NAMES[e] end
      out[#out + 1] = { text = "Magic burst: " .. table.concat(els, ", ") }
    else
      local props = {}
      for _, p in ipairs(n.props) do props[#props + 1] = SC.NAMES[p] end
      out[#out + 1] = { text = "Open: " .. table.concat(props, " \u{b7} "), strong = true }
    end
    local f = waiting and n.age / T.WAIT or n.left / math.max(n.life - T.WAIT, 0.1)
    out[#out + 1] = { bar = math.max(0, math.min(1, f)), tone = waiting and "muted" or "gold" }
    out[#out + 1] = { text = "by " .. n.by, tone = "muted", gap = 4 }
    if #n.closers == 0 then
      out[#out + 1] = { text = "Nothing the party knows closes it.", tone = "muted" }
    else
      out[#out + 1] = { heading = true, text = "Close with", tone = "gold" }
    end
    for _, c in ipairs(n.closers) do
      out[#out + 1] = { spans = { chain_span(c.chain, false),
        { text = c.who .. ": " .. c.skill, tone = c.ready and "text" or "muted" } }, column = 128 }
    end
    out[#out].gap = (out[#out].gap or 0) + 8
  end
  out[#out + 1] = { heading = true, big = true, text = "Party chains", tone = "gold" }
  if #v.chains == 0 then
    out[#out + 1] = { text = "None known yet: the party's skills show here as they are used.", tone = "muted" }
  end
  for _, ch in ipairs(v.chains) do
    local note = #ch.ways == 1 and "1 way" or (#ch.ways .. " ways")
    out[#out + 1] = { fold = tostring(ch.chain), open = open[ch.chain] == true, spans = { chain_span(ch.chain, true) },
      right = { text = note, tone = "muted" } }
    if open[ch.chain] then
      for _, w in ipairs(ch.ways) do
        out[#out + 1] = { text = way_text(w), tone = w.ready and "text" or "muted", indent = 16 }
      end
    end
    out[#out].gap = 3
  end
  return out
end

-- The classic view, one colour a line: the open resonance and four closers, then each chain once with its best way.
local function classic_lines(v, ms, cbs)
  local out = {}
  local function who(k) return ms[k].me and "You" or ms[k].name end
  local function ready(k) return ms[k].tp >= 1000 end
  local n = v.now
  if n then
    if n.chain then
      out[#out + 1] = { text = "Skillchain: " .. SC.NAMES[n.chain], tone = "gold" }
      local els = {}
      for _, e in ipairs(SC.elements(n.chain)) do els[#els + 1] = SC.ELEMENT_NAMES[e] end
      out[#out + 1] = { text = ("Burst %s: %.1f s"):format(table.concat(els, " / "), n.left) }
    else
      out[#out + 1] = { text = n.mob .. ": " .. SC.props_text(n.props) }
    end
    if n.age < T.WAIT then
      out[#out + 1] = { text = ("Wait %.1f s"):format(T.WAIT - n.age), tone = "muted" }
      out[#out + 1] = { bar = n.age / T.WAIT, tone = { 0x40, 0x40, 0x40, 0xa9 } }
    else
      out[#out + 1] = { text = ("Close within %.1f s"):format(n.left), tone = "muted" }
      out[#out + 1] = { bar = math.min(1, n.left / math.max(n.life - T.WAIT, 0.1)), tone = { 0x80, 0x70, 0x20, 0xa9 } }
    end
    for k = 1, math.min(4, #n.closers) do
      local c = n.closers[k]
      out[#out + 1] = { text = c.who .. ": " .. c.skill, tone = c.ready and "text" or "muted" }
      out[#out + 1] = { text = "  > " .. SC.chain_text(c.chain), tone = level_tone(c.chain) }
    end
  end
  out[#out + 1] = { heading = true, text = "Party chains", tone = "gold" }
  if #cbs == 0 then out[#out + 1] = { text = "None known yet.", tone = "muted" } end
  local done = {}
  for _, cb in ipairs(cbs) do
    if not done[cb.chain] then
      done[cb.chain] = true
      local best, key
      for _, x in ipairs(cbs) do
        if x.chain == cb.chain then
          local us = ms[x.opener[1]].me or ms[x.closer[1]].me
          local k = (us and 0 or 2) + (ready(x.opener[1]) and 0 or 1)
          if not key or k < key then best, key = x, k end
        end
      end
      best = best or cb
      out[#out + 1] = { text = SC.chain_text(cb.chain), tone = level_tone(cb.chain) }
      out[#out + 1] = { text = who(best.opener[1]) .. ": " .. T.name(best.opener[2]),
        tone = ready(best.opener[1]) and "text" or "muted" }
      out[#out + 1] = { text = "  > " .. who(best.closer[1]) .. ": " .. T.name(best.closer[2]), tone = "muted" }
    end
  end
  return out
end

-- The party and its chains, worked out again when half a second old (the view and the facts service share them).
local function party()
  if not members or os.clock() - members_at >= 0.5 then
    members = T.members()
    combos = T.combos(members)
    members_at = os.clock()
  end
  return members, combos
end

-- Only an older client's refusal of the new element means legacy; any other error is a bug to show.
local function older(err)
  return tostring(err):find("unknown element type", 1, true) ~= nil
end

local function refresh()
  T.tick()
  if legacy then
    if settings.get("on") then ui.open("skillchains") else ui.close("skillchains") end
    return
  end
  if not settings.get("on") then
    if made then ui.close("skillchains") end
    shown = nil
    return
  end
  party()
  local v = T.view(members, combos)
  if not v then
    if made then ui.close("skillchains") end
    shown = nil
    return
  end
  local lines = game.info().modern_ui and modern_lines(v) or classic_lines(v, members, combos)
  local key = json.encode(lines)
  if not made then
    local ok, err = pcall(ui.window, "skillchains", { edge = true, panel = "skillchains", resize = true, fit = true,
      width = 240, title = "Skillchains", elements = { { type = "lines", id = "view", lines = lines } } })
    if not ok then
      if not older(err) then error(err, 0) end
      legacy = true
      ui.window("skillchains", { edge = true, panel = "skillchains", resize = true, title = "Skillchains" })
      ui.open("skillchains")
      return
    end
    made = true
  elseif key ~= shown then
    ui.update("skillchains", "view", { lines = lines })
  end
  if shown == nil then ui.open("skillchains") end
  shown = key
end

ui.on("skillchains", function(_, ev)
  if ev.kind == "fold" then
    local c = tonumber(ev.key)
    if c then open[c] = not open[c] or nil end
    refresh()
  end
end)

events.on("action", function(a) T.on_action(a) end)
events.on("settings_changed", refresh)
timer.every(0.1, refresh)

-- Facts for other mods (the battle menu's suggestions): the monster followed, who it last went for, its open
-- resonance, and the other members with their skills' names and properties.
mods.provide("facts", function()
  local focus = T.focus()
  local ms = party()
  local partners = {}
  for _, m in ipairs(ms) do
    if not m.me then
      local skills = {}
      for _, s in ipairs(m.skills) do skills[#skills + 1] = { name = T.name(s), props = SC.props(s) } end
      partners[#partners + 1] = { name = m.name, tp = m.tp, skills = skills }
    end
  end
  local r = focus and T.on[focus]
  local resonance
  if r and T.left(r) > 0 then
    resonance = { against = T.against(r), chain = r.chain, age = T.age(r), left = T.left(r) }
  end
  return { focus = focus, hitting = focus and T.hitting(focus) or nil, resonance = resonance, partners = partners }
end)
-- Weapon skills' properties and power order (the skill level that unlocks them), by id.
mods.provide("weapon_skills", function(ids)
  local out = {}
  for _, id in ipairs(ids or {}) do
    local e = SC.data().weapon[id]
    if e then out[id] = { props = e.props, power = e.skill } end
  end
  return out
end)
-- The chain rules: map {closer's property, resonance, chain}, names, elements by property.
mods.provide("rules", function()
  local els = {}
  for p = 1, 16 do els[p] = SC.elements(p) end
  return { map = SC.MAP, names = SC.NAMES, elements = els }
end)
