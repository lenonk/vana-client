-- RDM Buffs: the self-buffs a Red Mage keeps up, the missing ones (or wearing off) listed in an edge window. Cast All
-- or /buffs casts them in order; a row casts that one. Support jobs: NIN Utsusemi, WHM Reraise, SCH Sublimation.

local RDM, WHM, NIN, SCH = 5, 3, 13, 20
local COLOUR = { 200, 160, 255 }

local Status = {
  HASTE = 33, BLAZE_SPIKES = 34, ICE_SPIKES = 35, BLINK = 36, STONESKIN = 37, SHOCK_SPIKES = 38, AQUAVEIL = 39,
  PROTECT = 40, SHELL = 41, REGEN = 42, REFRESH = 43, COPY_IMAGE = 66, STR_BOOST = 80, ENFIRE = 94, RERAISE = 113,
  PHALANX = 116, SUBLIMATION_ACTIVATED = 187, SUBLIMATION_COMPLETE = 188, ENFIRE_II = 277, COMPOSURE = 419,
  MULTI_STRIKES = 432, COPY_IMAGE_2 = 444, COPY_IMAGE_3 = 445, COPY_IMAGE_4 = 446,
}

local Spell = {
  PROTECT = 43, PROTECT_V = 47, SHELL = 48, SHELL_V = 52, BLINK = 53, STONESKIN = 54, AQUAVEIL = 55, HASTE = 57,
  ENFIRE = 100, PHALANX = 106, PHALANX_II = 107, REGEN = 108, REFRESH = 109, REGEN_II = 110, RERAISE = 135, BLAZE_SPIKES = 249,
  ICE_SPIKES = 250, SHOCK_SPIKES = 251, ENFIRE_II = 312, UTSUSEMI_ICHI = 338, UTSUSEMI_NI = 339, REFRESH_II = 473,
  GAIN_STR = 486, TEMPER = 493, HASTE_II = 511, REFRESH_III = 894, TEMPER_II = 895,
}

local GAINS = { "STR", "DEX", "VIT", "AGI", "INT", "MND", "CHR" }
local ENSPELLS = { "Fire", "Ice", "Wind", "Earth", "Thunder", "Water" }
local SPIKES = {
  Blaze = { Spell.BLAZE_SPIKES, Status.BLAZE_SPIKES },
  Ice = { Spell.ICE_SPIKES, Status.ICE_SPIKES },
  Shock = { Spell.SHOCK_SPIKES, Status.SHOCK_SPIKES },
}
local SHADOWS = { Status.COPY_IMAGE, Status.COPY_IMAGE_2, Status.COPY_IMAGE_3, Status.COPY_IMAGE_4 }

local AFTER_CAST = 3 -- seconds after a cast bar ends before the next action goes through
local AFTER_ABILITY = 1.5

local function index_of(list, v)
  for i, x in ipairs(list) do
    if x == v then return i end
  end
end

-- The buffs wanted now, in casting order: {key, label, statuses, spells (best first) or ability}.
local function wanted(p)
  local out = {}
  local function add(key, label, statuses, spells, ability)
    out[#out + 1] = { key = key, label = label, statuses = statuses, spells = spells, ability = ability }
  end
  local on = settings.get
  if on("composure") then add("composure", "Composure", { Status.COMPOSURE }, nil, "Composure") end
  if on("haste") then add("haste", "Haste", { Status.HASTE }, { Spell.HASTE_II, Spell.HASTE }) end
  if on("refresh") then
    add("refresh", "Refresh", { Status.REFRESH }, { Spell.REFRESH_III, Spell.REFRESH_II, Spell.REFRESH })
  end
  if on("phalanx") then add("phalanx", "Phalanx", { Status.PHALANX }, { Spell.PHALANX_II, Spell.PHALANX }) end
  if on("temper") then add("temper", "Temper", { Status.MULTI_STRIKES }, { Spell.TEMPER_II, Spell.TEMPER }) end
  local g = index_of(GAINS, on("gain"))
  if g then add("gain", "Gain-" .. GAINS[g], { Status.STR_BOOST + g - 1 }, { Spell.GAIN_STR + g - 1 }) end
  local en = index_of(ENSPELLS, on("enspell"))
  if en then
    add("enspell", "Enspell", { Status.ENFIRE_II + en - 1, Status.ENFIRE + en - 1 },
      { Spell.ENFIRE_II + en - 1, Spell.ENFIRE + en - 1 })
  end
  if on("protect") then
    local pro, she = {}, {}
    for i = 0, 4 do
      pro[#pro + 1] = Spell.PROTECT_V - i
      she[#she + 1] = Spell.SHELL_V - i
    end
    add("protect", "Protect", { Status.PROTECT }, pro)
    add("shell", "Shell", { Status.SHELL }, she)
  end
  if on("regen") then add("regen", "Regen", { Status.REGEN }, { Spell.REGEN_II, Spell.REGEN }) end
  if on("stoneskin") then add("stoneskin", "Stoneskin", { Status.STONESKIN }, { Spell.STONESKIN }) end
  if on("aquaveil") then add("aquaveil", "Aquaveil", { Status.AQUAVEIL }, { Spell.AQUAVEIL }) end
  if on("shadows") then
    if p.sub_job == NIN then
      add("shadows", "Utsusemi", SHADOWS, { Spell.UTSUSEMI_NI, Spell.UTSUSEMI_ICHI })
    else
      add("shadows", "Blink", { Status.BLINK }, { Spell.BLINK })
    end
  end
  local spikes = SPIKES[on("spikes")]
  if spikes then add("spikes", "Spikes", { spikes[2] }, { spikes[1] }) end
  if on("reraise") and p.sub_job == WHM then add("reraise", "Reraise", { Status.RERAISE }, { Spell.RERAISE }) end
  if on("sublimation") and p.sub_job == SCH then
    add("sublimation", "Sublimation", { Status.SUBLIMATION_ACTIVATED, Status.SUBLIMATION_COMPLETE }, nil, "Sublimation")
  end
  return out
end

-- Whether your jobs can cast a learned spell now (job point gifts on the main job only).
local function can_cast(id, p, known)
  local s = known[id] and res.spell(id)
  if not s then return false end
  local function by(job, level, main)
    local need = job and s.levels[job]
    if not need or not level then return false end
    if s.job_points and s.job_points[job] then
      local jp = main and level >= 99 and game.job_points()[job]
      return jp and jp.spent >= need or false
    end
    return (main or s.main_only ~= job) and level >= need
  end
  return by(p.main_job, p.main_level, true) or by(p.sub_job, p.sub_level, false)
end

local function ability_id(name)
  for _, id in ipairs(game.abilities()) do
    local a = res.ability(id)
    if a and a.name == name then return id, a end
  end
end

-- The wanted buffs that are down or wearing off, each with what casts it and its recast.
local function missing()
  local p = game.player()
  if p.main_job ~= RDM then return {}, false end
  local up = {}
  for _, b in ipairs(game.buffs()) do
    local left = b.remaining or math.huge
    up[b.id] = math.max(up[b.id] or 0, left)
  end
  local known = {}
  for _, id in ipairs(game.spells()) do known[id] = true end
  local recasts = game.recasts()
  local early = settings.get("early")
  local out = {}
  for _, w in ipairs(wanted(p)) do
    local left
    for _, st in ipairs(w.statuses) do
      if up[st] then left = math.max(left or 0, up[st]) end
    end
    if not left or left < early then
      local m = { key = w.key, label = w.label, left = left }
      if w.ability then
        local id, a = ability_id(w.ability)
        if id then
          m.ability, m.icon = id, { kind = "ability", id = id }
          m.recast = recasts.abilities[a.recast_id] or 0
        end
      else
        for _, id in ipairs(w.spells) do
          if can_cast(id, p, known) then
            local s = res.spell(id)
            m.spell, m.icon = id, { kind = "spell", id = id }
            m.label = s.name or w.label
            m.recast = recasts.spells[id] or 0
            m.mp = s.mp
            break
          end
        end
      end
      if m.ability or m.spell then out[#out + 1] = m end
    end
  end
  return out, true
end

local busy, stop = false, false

-- Casts one buff on yourself and waits until the next action can go. False when no cast bar came up.
local function cast_one(m)
  if m.ability then
    actions.ability(m.ability)
    wait_until(function() return stop end, AFTER_ABILITY)
    return true
  end
  actions.cast(m.spell, "<me>")
  wait_until(function() return game.cast() ~= nil end, 2)
  if not game.cast() then return false end
  wait_until(function() return game.cast() == nil end, 30)
  wait_until(function() return stop end, AFTER_CAST)
  return true
end

local refresh

local function cast_all()
  if busy then return end
  busy, stop = true, false
  refresh()
  local tried = {}
  while not stop do
    local pick
    for _, m in ipairs((missing())) do
      if not tried[m.key] and m.recast <= 0 then
        pick = m
        break
      end
    end
    if not pick then break end
    tried[pick.key] = true
    cast_one(pick)
  end
  busy, stop = false, false
  refresh()
end

local shown = {}

refresh = function()
  local list, rdm = missing()
  if not settings.get("window") or not rdm then
    ui.close("buffs")
    return
  end
  shown = list
  local mp = game.player().mp or 0
  local rows = {}
  for _, m in ipairs(list) do
    local note, dim = "", false
    if m.recast > 0 then
      note, dim = ("recast %ds"):format(math.ceil(m.recast)), true
    elseif m.mp and m.mp > mp then
      note, dim = "no MP", true
    elseif m.left then
      note = ("%ds left"):format(math.floor(m.left))
    end
    rows[#rows + 1] = { cells = { m.label, note }, icon = m.icon, colour = dim and { 150, 150, 150 } or nil }
  end
  local els
  if #rows == 0 then
    els = { { type = "text", text = "All buffs are up." } }
  else
    els = {
      { type = "list", id = "list", height = #rows, rows = rows,
        columns = { { title = "Buff" }, { title = "", width = 90, align = "right" } } },
      { type = "buttons", id = "go", items = { "Cast All", "Cancel" } },
    }
  end
  ui.window("buffs", { edge = true, width = 280, title = "Buffs",
    heading = #rows == 0 and "Buffs" or ("Buffs: %d missing"):format(#rows), elements = els })
  ui.open("buffs")
end

ui.on("buffs", function(_, e)
  if e.kind == "press" and e.id == "go" then
    if e.index == 2 then
      stop = true
    else
      cast_all()
    end
  elseif e.kind == "select" and e.id == "list" then
    local m = shown[e.index]
    if m and not busy then
      busy, stop = true, false
      cast_one(m)
      busy, stop = false, false
      refresh()
    end
  end
end)

timer.every(0.5, refresh)
events.on("settings_changed", refresh)

commands.register("buffs", function(arg)
  local list, rdm = missing()
  if not rdm then
    chat.add("RDM Buffs: your main job is not Red Mage.", COLOUR)
    return
  end
  if arg and arg:lower() == "cancel" then
    stop = true
    return
  end
  if arg and arg:lower() == "list" then
    if #list == 0 then
      chat.add("RDM Buffs: all buffs are up.", COLOUR)
    end
    for _, m in ipairs(list) do
      local note = m.recast > 0 and (" (recast %ds)"):format(math.ceil(m.recast)) or ""
      chat.add("RDM Buffs: " .. m.label .. note, COLOUR)
    end
    return
  end
  cast_all()
end)
