-- LuAshitacast profiles, run as LuAshitacast runs them: the file returns the profile; its Handle* functions run
-- around every action packet we send (typed, macro or menu alike) and HandleDefault on the send tick while no action
-- is under way; gFunc.Equip / EquipSet fill a buffer sent when the handler returns; gData answers in its names.

local core = require("src.core")
local compat = require("src.compat")
local names = require("src.names")
local LAC = names.LAC

local M = {}

local env, profile
M.file = nil
local buffer, interim = {}, {}
local current = "N/A"
local action, pet_action = nil, nil
local disabled = {}
local lock = nil          -- {set, ends}
local delayed = nil       -- {set, ends}
local debug = false
local sent_at = {}        -- slot -> time (predicted for 0.2 s)

local SLOT_ID = {}
for i, n in ipairs(LAC.SLOTS) do SLOT_ID[n:lower()] = i - 1 end

local function slot_of(k)
  if type(k) == "number" then return (k >= 1 and k <= 16) and k - 1 or nil end
  if type(k) ~= "string" then return nil end
  return SLOT_ID[k:lower()]
end

local function say(text) chat.add("Gearshift: " .. text) end

local function defaults()
  return { AllowAddSet = false, Debug = false, SafeCall = true, AllowSyncEquip = true, HorizonMode = false,
    EquipBags = { 8, 10, 11, 12, 13, 14, 15, 16, 0 }, PetskillDelay = 4.0, WeaponskillDelay = 3.0, AbilityDelay = 2.5,
    SpellOffset = 1.0, RangedBase = 10.0, RangedOffset = 0.5, ItemBase = 8, ItemOffset = 1.0, FastCast = 0, Snapshot = 0 }
end

-- ---- Running the profile ----

local function call(name, ...)
  local f = profile and profile[name]
  if f == nil then return false end
  if type(f) ~= "function" then say(name .. " is not a function") return false end
  local prev = current
  current = name
  if env.gSettings.SafeCall ~= false then
    local ok, err = pcall(f, ...)
    if not ok then say(name .. ": " .. tostring(err)) end
  else
    f(...)
  end
  current = prev
  return true
end

-- ---- Equipping ----

local function item_want(v)
  if type(v) == "string" then
    local l = v:lower()
    if l == "remove" then return { remove = true, priority = -100 } end
    if l == "displaced" then return { displaced = true } end
    if l == "ignore" then return nil end
    return { name = v }
  end
  if type(v) == "table" and type(v.Name) == "string" then
    local l = v.Name:lower()
    if l == "remove" then return { remove = true, priority = tonumber(v.Priority) or -100 } end
    if l == "ignore" then return nil end
    local augs = v.Augment
    if type(augs) == "string" then augs = { augs } end
    local bag = v.Bag
    if type(bag) == "string" then bag = LAC.CONTAINERS[bag] end
    return { name = v.Name, augments = augs, bag = bag, priority = tonumber(v.Priority) or 0 }
  end
end

local function resolve_set(set)
  local out = {}
  for k, v in pairs(set) do
    local s = slot_of(k)
    if s then out[s] = v end
  end
  return out
end

-- The buffer as packets in a style: "set" (one 0x051), "single" (0x050s), "auto" (0x050s up to 8 pieces).
local function packets_for(buf, style, label)
  if lock and clock() < lock.ends then buf = resolve_set(lock.set) elseif lock then lock = nil end
  local wants = {}
  for s, v in pairs(buf) do
    if not disabled[s] then
      local w = item_want(v)
      if w and not w.displaced then wants[s] = w end
    end
  end
  if next(wants) == nil then return {} end
  local list = core.resolve(wants, { log_names = false, bags = env.gSettings.EquipBags })
  if debug and #list > 0 then
    local t = {}
    for _, c in ipairs(list) do
      local r = c.id and core.record(c.id)
      t[#t + 1] = LAC.SLOTS[c.slot + 1] .. ": " .. (r and r.name or "remove")
    end
    say((label or current) .. ": " .. table.concat(t, ", "))
  end
  return core.packets(list, style == "set" and "set" or (style == "single" and "single" or "auto"), 8)
end

local function take(buf_name)
  local b = buf_name == "interim" and interim or buffer
  if buf_name == "interim" then interim = {} else buffer = {} end
  return b
end

-- ---- gFunc ----

local function equip_into(buf, slot, item)
  local s = slot_of(slot)
  if not s then say("invalid slot: " .. tostring(slot)) return end
  if type(item) == "string" and item:lower() == "ignore" then buf[s] = nil return end
  buf[s] = item
end

local function find_set(name)
  if type(name) == "table" then return name end
  if type(name) ~= "string" or not profile or type(profile.Sets) ~= "table" then return nil end
  local t = profile.Sets
  for part in name:gmatch("[^%.]+") do
    if type(t) ~= "table" then return nil end
    local found
    for k, v in pairs(t) do
      if type(k) == "string" and k:lower() == part:lower() then found = v break end
    end
    t = found
  end
  return t
end

local function equip_set_into(buf, set)
  local t = find_set(set)
  if type(t) ~= "table" then say("set not found: " .. tostring(set)) return end
  for k, v in pairs(t) do
    if slot_of(k) then equip_into(buf, k, v) end
  end
end

local function make_gfunc()
  local g = {}
  g.Equip = function(slot, item) equip_into(buffer, slot, item) end
  g.EquipSet = function(set) equip_set_into(buffer, set) end
  g.InterimEquip = function(slot, item) equip_into(interim, slot, item) end
  g.InterimEquipSet = function(set) equip_set_into(interim, set) end
  g.ForceEquip = function(slot, item)
    local b = {}
    equip_into(b, slot, item)
    core.send(packets_for(b, "auto"))
  end
  g.ForceEquipSet = function(set)
    local b = {}
    equip_set_into(b, set)
    core.send(packets_for(b, "auto"))
  end
  g.LockSet = function(set, seconds)
    local t = find_set(set)
    if type(t) ~= "table" then say("set not found: " .. tostring(set)) return end
    lock = { set = t, ends = clock() + (tonumber(seconds) or 0) }
    core.send(packets_for(resolve_set(t), "auto"))
  end
  g.LockStyle = function() end
  g.CancelAction = function() if action then action.block = true end end
  g.ChangeActionTarget = function(index)
    if not action then return end
    for _, e in ipairs(game.entities() or {}) do
      if e.index == index then core.retarget(action.act, e.id, e.index) return end
    end
  end
  g.ChangeActionId = function() say("ChangeActionId is not supported") end
  g.ClearEquipBuffer = function() buffer = {} end
  g.Combine = function(base, over)
    local out = {}
    for _, t in ipairs({ base or {}, over or {} }) do
      for k, v in pairs(t) do
        local s = slot_of(k)
        if s then out[LAC.SLOTS[s + 1]] = v end
      end
    end
    return out
  end
  g.CompareItem = function() return false end
  g.Disable = function(slot)
    if type(slot) == "string" and slot:lower() == "all" then for s = 0, 15 do disabled[s] = true end
    else local s = slot_of(slot) if s then disabled[s] = true end end
  end
  g.Enable = function(slot)
    if type(slot) == "string" and slot:lower() == "all" then disabled = {}
    else local s = slot_of(slot) if s then disabled[s] = nil end end
  end
  g.Echo = function(_, text) chat.add(tostring(text)) end
  g.Message = function(text) say(tostring(text)) end
  g.Error = function(text) say(tostring(text)) end
  g.LoadFile = function(path) return M.load_file(path) end
  g.SetMidDelay = function(seconds) delayed = { ends = clock() + (tonumber(seconds) or 0) } end
  g.AddSet = function() say("addset is not supported") end
  g.EvaluateLevels = function(sets, level)
    for key, v in pairs(sets) do
      if type(key) == "string" and key:match("_Priority$") and type(v) == "table" then
        local out = {}
        for slot, entry in pairs(v) do
          if type(entry) == "table" and entry[1] ~= nil then
            for _, it in ipairs(entry) do
              local lvl = type(it) == "table" and it.Level
              if not lvl then
                local n = type(it) == "table" and it.Name or it
                for _, f in ipairs(game.find_items(n) or {}) do
                  local r = core.record(f.id)
                  lvl = r and r.level
                  break
                end
              end
              if (lvl or 0) <= level then out[slot] = it break end
            end
          else
            out[slot] = entry
          end
        end
        sets[key:gsub("_Priority$", "")] = out
      end
    end
  end
  g.ApplyBaseSets = function(sets)
    local changed = true
    while changed do
      changed = false
      local function walk(t)
        for _, v in pairs(t) do
          if type(v) == "table" then
            if type(v.BaseSet) == "string" then
              local base = find_set(v.BaseSet) or sets[v.BaseSet]
              if type(base) == "table" and base.BaseSet == nil then
                for k, item in pairs(base) do if slot_of(k) and v[k] == nil then v[k] = item end end
                v.BaseSet = nil
                changed = true
              end
            end
            walk(v)
          end
        end
      end
      walk(sets)
    end
  end
  return g
end

-- ---- gData ----

local function entity_table(e)
  if not e then return nil end
  local t = "Unknown"
  if e.me or e.in_party then t = "Party" elseif e.in_alliance then t = "Alliance"
  elseif e.kind == "player" then t = "PC" elseif e.kind == "npc" then t = "NPC" elseif e.kind == "mob" then t = "Monster" end
  return { Distance = e.distance, HPP = e.hpp, Id = e.id, Index = e.index, Name = e.name,
    Status = LAC.STATUS[e.status] or "Unknown", Type = t }
end

local function action_table()
  if not action then return nil end
  local a, p = action.act, game.player()
  if a.kind == "item" then
    local id
    for _, x in ipairs(game.items(a.bag) or {}) do if x.slot == a.index then id = x.id break end end
    local r = id and core.record(id)
    return { ActionType = "Item", Name = r and r.name, Id = id, Resource = r,
      CastTime = r and (r.cast_time or 0) * 1000, Recast = r and (r.reuse_delay or 0) * 1000, Resend = false }
  elseif a.category == 3 then
    local r = res.spell(a.param) or {}
    local mp = r.mp or 0
    return { ActionType = "Spell", Name = r.name, Id = a.param, Resource = r, Resend = false,
      CastTime = (r.cast_time or 0) * 1000, Recast = (r.recast or 0) * 1000, MpCost = mp,
      Element = LAC.ELEMENTS[r.element or 15] or "Unknown", Skill = LAC.SKILLS[r.skill or 0] or "Unknown",
      Type = LAC.SPELL_TYPES[r.kind or 0] or "Unknown", MpAftercast = (p.mp or 0) - mp,
      MppAftercast = p.mp_max and p.mp_max > 0 and math.floor(((p.mp or 0) - mp) * 100 / p.mp_max) or p.mpp }
  elseif a.category == 16 then
    return { ActionType = "Ranged", Name = "Ranged", Id = 0, Resend = false }
  elseif a.category == 7 then
    local r = res.ability(a.param) or {}
    return { ActionType = "Weaponskill", Name = r.name, Id = a.param, Resource = r, Resend = false }
  else
    local r = res.ability(a.param + 0x200) or {}
    return { ActionType = "Ability", Name = r.name, Id = a.param, Resource = r, Resend = false,
      Type = LAC.ABILITY_TYPES[r.recast_id or -1] or "Unknown" }
  end
end

local function make_gdata()
  local d = {}
  d.GetCurrentCall = function() return current end
  d.GetAction = action_table
  d.GetActionTarget = function()
    if not action then return nil end
    return entity_table(game.entity(action.act.target))
  end
  d.GetEntity = function(index)
    for _, e in ipairs(game.entities() or {}) do if e.index == index then return entity_table(e) end end
    local me = game.player()
    if me.index == index then return entity_table(game.entity(me.id)) end
    return {}
  end
  d.GetTarget = function() return entity_table(game.target()) end
  d.GetBuffCount = function(b)
    local n = 0
    local want = type(b) == "string" and b:lower() or b
    for _, x in ipairs(game.buffs() or {}) do
      if x.id == want or (type(want) == "string" and (res.status(x.id) or ""):lower() == want) then n = n + 1 end
    end
    return n
  end
  d.GetEnvironment = function()
    local t, w, p = game.time(), game.weather(), game.player()
    local storm
    for _, b in ipairs(game.buffs() or {}) do storm = storm or names.STORMS[b.id] end
    local ew = storm or w
    return { Area = res.zone(p.zone or 0), Day = LAC.DAYS[t.weekday], DayElement = LAC.DAY_ELEMENTS[t.weekday],
      MoonPhase = res.moon(t.moon_phase), MoonPercent = t.moon, RawWeather = LAC.WEATHER[w] or "Unknown",
      RawWeatherElement = LAC.WEATHER_ELEMENT[w] or "None", Weather = LAC.WEATHER[ew] or "Unknown",
      WeatherElement = LAC.WEATHER_ELEMENT[ew] or "None", Time = t.hour + t.minute / 100,
      Timestamp = { day = t.day, hour = t.hour, minute = t.minute } }
  end
  d.GetEquipment = function()
    local out = {}
    for s, w in pairs(core.worn()) do
      local r = core.record(w.id)
      out[LAC.SLOTS[s + 1]] = { Container = w.bag, Item = { Id = w.id, Index = w.index }, Name = r and r.name, Resource = r }
    end
    return out
  end
  d.GetPet = function()
    local e = game.entity_by_tag("pet")
    if not e or (e.hpp or 0) == 0 then return nil end
    local t = entity_table(e)
    t.TP = e.tp
    return t
  end
  d.GetPetAction = function() return pet_action end
  d.GetPlayer = function()
    local p = game.player()
    return { HP = p.hp, MaxHP = p.hp_max, HPP = p.hpp, MP = p.mp, MaxMP = p.mp_max, MPP = p.mpp, TP = p.tp,
      Name = p.name, Status = LAC.STATUS[p.status] or "Unknown", MainJob = res.job(p.main_job or 0),
      SubJob = res.job(p.sub_job or 0), MainJobLevel = p.main_level, SubJobLevel = p.sub_level,
      MainJobSync = p.main_level, SubJobSync = p.sub_level, IsMoving = false }
  end
  d.GetParty = function()
    local n = 0
    for _, m in ipairs(game.party() or {}) do if m.party == 0 then n = n + 1 end end
    return { Count = n, InParty = n > 1 }
  end
  d.GetAlliance = function()
    local n = #(game.party() or {})
    return { Count = n, InAlly = n > 6 }
  end
  d.GetEquipSlot = function(s) local id = slot_of(s) return id and id + 1 or 0 end
  return d
end

-- Ashita's API, the parts profiles reach most: chat commands and the player's jobs.
local function make_ashitacore(run_command)
  local player = {
    GetMainJob = function() return game.player().main_job end,
    GetSubJob = function() return game.player().sub_job end,
    GetMainJobLevel = function() return game.player().main_level end,
    GetSubJobLevel = function() return game.player().sub_level end,
    GetHPPercent = function() return game.player().hpp end,
    GetMPPercent = function() return game.player().mpp end,
  }
  local function obj(t) return setmetatable(t, { __index = function() return function() return 0 end end }) end
  local chatm = obj({ QueueCommand = function(_, _, cmd) run_command(cmd) end })
  local memory = obj({ GetPlayer = function() return obj(player) end })
  return obj({ GetChatManager = function() return chatm end, GetMemoryManager = function() return memory end })
end

-- ---- Loading ----

function M.dirs()
  local p = game.player()
  return { "ashitacast/" .. p.name .. "_" .. p.id .. "/", "ashitacast/" }
end

local function find_file(dir, name)
  local lname = name:lower()
  for _, n in ipairs(fs.list(dir) or {}) do
    if n:lower() == lname then return dir .. n end
  end
end

function M.find()
  local p = game.player()
  local short = res.job(p.main_job or 0)
  if not short then return nil end
  return find_file("ashitacast/" .. p.name .. "_" .. p.id .. "/", short .. ".lua") or find_file("ashitacast/", p.name .. "_" .. short .. ".lua")
end

local run_command

function M.load_file(path)
  local candidates = { path, path .. ".lua" }
  for _, dir in ipairs(M.dirs()) do
    candidates[#candidates + 1] = dir .. path
    candidates[#candidates + 1] = dir .. path .. ".lua"
  end
  for _, c in ipairs(candidates) do
    local dir, base = c:match("^(.-)([^/]+)$")
    local f = find_file(dir, base)
    if f then
      local chunk, err = load(fs.read(f), "@" .. f, "t", env)
      if not chunk then say(err) return nil end
      local ok, v = pcall(chunk)
      if not ok then say(tostring(v)) return nil end
      return v
    end
  end
  say("file not found: " .. path)
end

local function make_env()
  local e = {}
  for _, k in ipairs({ "type", "tonumber", "pairs", "ipairs", "next", "select", "assert", "error", "pcall", "xpcall",
    "setmetatable", "getmetatable", "rawset", "rawget", "rawequal", "rawlen", "os", "coroutine", "utf8" }) do
    e[k] = _G[k]
  end
  compat.install(e)
  e.print = function(...) local t = {} for i = 1, select("#", ...) do t[#t + 1] = tostring(select(i, ...)) end say(table.concat(t, " ")) end
  e._G = e
  e.gFunc, e.gData = make_gfunc(), make_gdata()
  e.gSettings = defaults()
  e.gState = {}
  e.AshitaCore = make_ashitacore(function(cmd) run_command(cmd) end)
  e.ashita = { events = { register = function() end, unregister = function() end },
    tasks = { once = function(delay, f) timer.after(delay or 0, f) end } }
  e.coroutine = setmetatable({ sleep = function(s) sleep(s) end }, { __index = coroutine })
  e.require = function(name)
    local v = M.load_file(tostring(name):gsub("%.", "/"))
    return v
  end
  return e
end

function M.unload()
  if profile then call("OnUnload") end
  env, profile, M.file, action, buffer, interim, lock, delayed = nil, nil, nil, nil, {}, {}, nil, nil
end

function M.load(path)
  M.unload()
  env = make_env()
  local chunk, err = load(fs.read(path) or "", "@" .. path, "t", env)
  if not chunk then say("could not load " .. path .. ": " .. tostring(err)) env = nil return false end
  local ok, p = pcall(chunk)
  if not ok then say("could not load " .. path .. ": " .. tostring(p)) env = nil return false end
  profile, M.file = p, path
  env.gProfile = p
  say("loaded profile " .. path)
  if type(p) == "table" then
    p.FilePath, p.FileName = path, path:match("[^/]+$")
    call("OnLoad")
  end
  return true
end

function M.loaded() return profile ~= nil end

-- The item names the loaded profile's sets hold.
function M.gear_names() return type(profile) == "table" and core.set_names(profile.Sets, slot_of) or {} end

-- ---- The action packet ----

local PRE = { [3] = { "HandlePrecast", "set" }, [7] = { "HandleWeaponskill", "auto" }, [9] = { "HandleAbility", "auto" },
  [16] = { "HandlePreshot", "set" } }
local MID = { [3] = "HandleMidcast", [16] = "HandleMidshot" }

local function estimate(act)
  local s = env.gSettings
  if act.kind == "item" then return s.ItemBase + s.ItemOffset end
  if act.category == 3 then
    local r = res.spell(act.param) or {}
    return (r.cast_time or 0) * (100 - (s.FastCast or 0)) / 100 + s.SpellOffset
  end
  if act.category == 7 then return s.WeaponskillDelay end
  if act.category == 16 then return s.RangedBase * (100 - (s.Snapshot or 0)) / 100 + s.RangedOffset end
  return s.AbilityDelay
end

function M.action(act)
  if not profile then return end
  local pre = act.kind == "item" and { "HandleItem", "auto" } or PRE[act.category]
  if not pre then return end
  action = { act = act, block = false, ends = clock() + estimate(act) }
  delayed = nil
  buffer, interim = {}, {}
  call(pre[1])
  if action.block then
    action, buffer = nil, {}
    return false
  end
  core.send(packets_for(take(), pre[2], pre[1]))
  core.H.inject(act.packet)
  local mid = act.kind ~= "item" and MID[act.category]
  if mid then
    action.ends = clock() + estimate(act)
    buffer, interim = {}, {}
    call(mid)
    if delayed then
      delayed.set = take()
      core.send(packets_for(take("interim"), "single", mid))
    else
      core.send(packets_for(take(), "single", mid))
    end
  end
  return false
end

local DONE = { [2] = true, [3] = true, [4] = true, [5] = true, [6] = true, [14] = true, [15] = true }

function M.on_action(a)
  if not profile then return end
  local me = game.player().id
  if a.actor == me and (DONE[a.category] or ((a.category == 8 or a.category == 12) and a.param == 28787)) then
    action, delayed = nil, nil
  end
end

-- The send tick: actions past their estimate end, a delayed mid set goes, and HandleDefault runs while idle.
function M.tick()
  if not profile then return end
  local now = clock()
  if action and now > action.ends then action = nil end
  if action then
    if delayed and delayed.set and now + 0.4 > delayed.ends then
      core.send(packets_for(delayed.set, "auto"))
      delayed = nil
    end
    return
  end
  buffer, interim = {}, {}
  call("HandleDefault")
  core.send(packets_for(take(), "auto", "HandleDefault"))
end

-- ---- Commands (/lac ...) ----

function M.command(args)
  local cmd = (args[1] or ""):lower()
  if cmd == "fwd" then
    call("HandleCommand", { table.unpack(args, 2) })
  elseif cmd == "load" then
    local f = args[2] and (find_file("ashitacast/", args[2]) or find_file("ashitacast/", args[2] .. ".lua")) or M.find()
    if f then M.load(f) else say("profile not found") end
  elseif cmd == "reload" then
    if M.file then M.load(M.file) end
  elseif cmd == "unload" then
    M.unload()
    say("profile unloaded")
  elseif cmd == "set" then
    env.gFunc.LockSet(args[2], tonumber(args[3]) or 3)
  elseif cmd == "disable" then
    env.gFunc.Disable(args[2] or "all")
  elseif cmd == "enable" then
    env.gFunc.Enable(args[2] or "all")
  elseif cmd == "equip" then
    local b = {}
    for i = 2, #args - 1, 2 do equip_into(b, args[i], args[i + 1]) end
    core.send(packets_for(b, "auto"))
  elseif cmd == "naked" then
    local b = {}
    for s = 0, 15 do b[s] = "remove" end
    core.send(packets_for(b, "auto"))
    for s = 0, 15 do disabled[s] = true end
  elseif cmd == "debug" then
    debug = args[2] == nil and not debug or args[2] == "on"
    say("debug " .. (debug and "on" or "off"))
  elseif cmd == "list" then
    if profile and type(profile.Sets) == "table" then
      local t = {}
      for k in pairs(profile.Sets) do t[#t + 1] = tostring(k) end
      table.sort(t)
      say(table.concat(t, ", "))
    end
  else
    say("unknown command: " .. cmd)
  end
end

-- Ashita's chat commands from profiles: /lac, /bind, others to the game.
run_command = function(cmd)
  local t = compat.tokens(tostring(cmd))
  local w = (t[1] or ""):lower()
  if w == "/lac" or w == "/luashitacast" then
    M.command({ table.unpack(t, 2) })
  elseif w == "/bind" and t[2] then
    local rest = tostring(cmd):gsub("^%S+%s+%S+%s*", "")
    pcall(binds.set, t[2], function() run_command(rest) end)
  elseif w == "/unbind" and t[2] then
    pcall(binds.unset, t[2])
  elseif w == "/alias" then
    log.debug("ashitacast: " .. cmd)
  elseif w ~= "" then
    commands.run(cmd)
  end
end

return M
