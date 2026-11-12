-- The set builder's engine: the whole item list indexed by slot (a slice a frame), our gear scanned, each set of the
-- job built for our level (one a frame, again when the job, the level or the bags change), and the sets worn around
-- actions as the other front ends wear theirs: precast, the action, midcast; between actions the set for our status.
-- Saved per character (storage.character.builder): only what the player chose by hand (a slot's piece, held through
-- rebuilds) and the toggles; every other pick follows the level.

local core = require("src.core")
local pick = require("src.build.pick")
local sets = require("src.build.sets")
local wslist = require("src.build.ws")
local blu = require("src.build.blu")

local M = {}

-- The game, through one table (tests replace it).
M.H = {
  player = function() return game.player() end,
  me = function()
    local p = game.player()
    return p and game.entity(p.id)
  end,
  items = function(bag) return game.items(bag) end,
  item = function(id) return res.item(id) end,
  spell = function(id) return res.spell(id) end,
  ability = function(id) return res.ability(id) end,
  job = function(id) return res.job(id) end,
  time = function() return game.time() end,
  weather = function() return game.weather() end,
  buffs = function() return game.buffs() end,
  -- Our pet is out: an entity marked as ours.
  pet = function()
    for _, e in ipairs(game.entities() or {}) do
      if e.pet then return true end
    end
    return false
  end,
  storage = function() return storage.character end,
  cap_setting = function() return settings.get("cap") end,
  -- Gear Swapping and Set Builder on: the sets are built (worn only with the window's switch on too).
  on = function() return settings.get("on") ~= false and settings.get("builder") ~= false end,
}

-- Equipment item ids (armour and weapons).
M.RANGES = { { 0x2800, 0x3FFF }, { 0x4000, 0x59FF }, { 0x5A00, 0x6FFF } }
M.SLICE = 600

local state = {
  index = {}, next_id = nil, range = 1, ready = false,
  job = nil, level = nil, owned = nil, owned_sig = nil,
  at_cap = nil, at_level = nil, need_cands = false,
  built = {}, queue = {}, names = {},
}
M.state = state

-- ---- The saved choices ----

local function saved()
  local st = M.H.storage()
  st.builder = st.builder or { jobs = {}, modes = {} }
  return st.builder
end

-- The hand picks of a job's set: {slot name = {name, augments}}.
function M.hand(job, set_name)
  local b = saved()
  b.jobs[job] = b.jobs[job] or {}
  b.jobs[job][set_name] = b.jobs[job][set_name] or {}
  return b.jobs[job][set_name]
end

function M.modes() return saved().modes end

-- The player turned the builder's swapping on (the window's switch), per character.
function M.active() return saved().active == true end
function M.set_active(on) saved().active = on or nil end

-- ---- The level cap the references are for ----

function M.cap()
  local c = M.H.cap_setting()
  if c == "75" or c == 75 then return 75 end
  if c == "99" or c == 99 then return 99 end
  return (state.level or 1) > 75 and 99 or 75
end

-- ---- Our gear ----

local ALL_BAGS = { 0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16 }
local EQUIP = {}
for _, b in ipairs(core.EQUIP_BAGS) do EQUIP[b] = true end

-- Every piece in the equippable bags, and the others' too (to say where a piece is kept).
local function scan()
  local owned, elsewhere, sig = {}, {}, {}
  for _, bag in ipairs(ALL_BAGS) do
    for _, it in ipairs(M.H.items(bag) or {}) do
      local rec = it.id and it.id > 0 and pick.H.item(it.id)
      if rec and rec.slots and rec.slots ~= 0 then
        core.want_augments(it.extdata, it.id)
        local entry = { id = it.id, key = bag .. ":" .. it.slot, bag = bag, index = it.slot,
          augments = it.extdata and core.augments[it.extdata] or nil }
        if EQUIP[bag] then owned[#owned + 1] = entry else elsewhere[#elsewhere + 1] = entry end
        sig[#sig + 1] = entry.key .. "=" .. it.id .. (entry.augments and "a" or "")
      end
    end
  end
  return owned, elsewhere, table.concat(sig, ",")
end

-- ---- The sets of a job ----

-- The weapon types we hold weapons of (their weapon skills get sets).
local SKILL_NAME = { [1] = "hand_to_hand", [2] = "dagger", [3] = "sword", [4] = "great_sword", [5] = "axe",
  [6] = "great_axe", [7] = "scythe", [8] = "polearm", [9] = "katana", [10] = "great_katana", [11] = "club",
  [12] = "staff", [25] = "archery", [26] = "marksmanship", [27] = "throwing" }

function M.weapon_types()
  local out = {}
  for _, o in ipairs(state.owned or {}) do
    local rec = pick.H.item(o.id)
    if rec and rec.skill and SKILL_NAME[rec.skill] then out[SKILL_NAME[rec.skill]] = true end
  end
  return out
end

-- A set by name: a catalogue set, or "WS:<id>" from the weapon skill's facts.
function M.set_def(name)
  local id = name:match("^WS:(%d+)$")
  if id then
    id = tonumber(id)
    local ws = wslist[id]
    if not ws then return nil end
    local a = M.H.ability(id)
    return sets.ws_set(id, ws, a and a.name)
  end
  return sets.SETS[name]
end

-- The job's sets in the window's order: the catalogue's, then a set per weapon skill of the weapon types we hold.
function M.job_sets(job)
  local out = {}
  for _, s in ipairs(sets.JOBS[job] or {}) do out[#out + 1] = s end
  local types = M.weapon_types()
  for _, id in ipairs(sets.ws_ids(job, wslist)) do
    if types[wslist[id].skill] then out[#out + 1] = "WS:" .. id end
  end
  return out
end

local function ctx()
  local p = M.H.player() or {}
  local me = M.H.me() or {}
  local owns = {}
  for _, o in ipairs(state.owned or {}) do owns[o.id] = true end
  return { job = state.job, job_id = p.main_job, race = me.race, level = state.level, cap = M.cap(),
    owned = state.owned or {}, owns = owns, at_cap = state.at_cap or {}, at_level = state.at_level or {},
    names = state.names,
    mage = sets.MAGE_JOBS[state.job], melee_sets = sets.MELEE_SETS, bis = nil }
end

-- Builds one set now (the window's selection may ask before the queue reaches it).
function M.build(name)
  local def = M.set_def(name)
  if not def or not state.ready or not state.at_cap then return nil end
  local c = ctx()
  local ok, bis = pcall(require, "src.build.bis")
  if ok and bis then c.bis = bis.lookup(M.cap(), state.job, name) end
  local r = pick.build(c, def, name)
  r.at = state.level
  state.built[name] = r
  return r
end

-- ---- The frame: the index, the scan and the build queue ----

local function index_step()
  if state.ready then return end
  local r = M.RANGES[state.range]
  local from = state.next_id or r[1]
  local to = math.min(from + M.SLICE - 1, r[2])
  local ids = {}
  for id = from, to do ids[#ids + 1] = id end
  pick.index_add(state.index, ids)
  for _, id in ipairs(ids) do
    local rec = pick.H.item(id)
    if rec and rec.name and rec.name ~= "" and rec.slots and rec.slots ~= 0 then
      state.names[rec.name] = state.names[rec.name] or {}
      table.insert(state.names[rec.name], id)
    end
  end
  if to >= r[2] then
    state.range, state.next_id = state.range + 1, nil
    if state.range > #M.RANGES then state.ready = true end
  else
    state.next_id = to + 1
  end
end

-- Queues every set of the job again.
function M.rebuild()
  state.built, state.queue = {}, {}
  if not state.job then return end
  for _, s in ipairs(M.job_sets(state.job)) do state.queue[#state.queue + 1] = s end
end

-- Puts a set first in the queue (the one the window shows, or an action wants).
function M.want_first(name)
  if state.built[name] then return end
  for i, s in ipairs(state.queue) do
    if s == name then table.remove(state.queue, i) break end
  end
  table.insert(state.queue, 1, name)
end

-- The job, the level and the bags: a change rebuilds every set (a timer and item_change call it).
function M.refresh()
  if not M.H.on() then return end
  local p = M.H.player()
  if not p or not p.main_job or p.main_job == 0 then return end
  local job = M.H.job(p.main_job)
  local owned, elsewhere, sig = scan()
  state.elsewhere = elsewhere
  if job ~= state.job or p.main_level ~= state.level then state.need_cands = true end
  if job ~= state.job or p.main_level ~= state.level or sig ~= state.owned_sig then
    state.job, state.level, state.owned, state.owned_sig = job, p.main_level, owned, sig
    M.rebuild()
  end
end

-- One step of the work: a slice of the index; once it is whole, the job's candidates (at the cap and at our level)
-- when the job or the level changed; then one set from the queue. Returns true while there is work left.
function M.step()
  if not M.H.on() then return false end
  if not state.ready then
    index_step()
    return true
  end
  if not state.job then return false end
  if state.need_cands or not state.at_cap then
    state.need_cands = false
    local p = M.H.player() or {}
    local me = M.H.me() or {}
    state.at_cap = pick.candidates(state.index, p.main_job, me.race, M.cap())
    state.at_level = pick.candidates(state.index, p.main_job, me.race, state.level)
    state.built = {}
    return true
  end
  if #state.queue > 0 then
    M.build(table.remove(state.queue, 1))
    return true
  end
  return false
end
M.frame = M.step

-- The worker: steps the work in the background, yielding when its share of a frame is used.
local SHARE = 0.0015
function M.worker()
  local t0 = clock()
  pick.pace = function()
    if clock() - t0 > SHARE then
      sleep_frames(1)
      t0 = clock()
    end
  end
  while true do
    t0 = clock()
    local more = M.step()
    sleep_frames(more and 1 or 10)
  end
end

-- ---- Wearing sets ----

-- A set's pieces: the hand picks first, then the build's (a set not built yet is asked for first).
function M.pieces(name)
  local out = {}
  local r = state.built[name]
  if not r then M.want_first(name) end
  if r then
    for slot, o in pairs(r.slots) do
      if o.pick then
        local rec = pick.H.item(o.pick.id)
        out[slot] = { name = rec and rec.name, augments = o.pick.owned and o.pick.owned.augments or nil }
      end
    end
  end
  for slot_name, h in pairs(M.hand(state.job, name)) do
    for slot, n in pairs(pick.SLOT_NAMES) do
      if n == slot_name then out[slot] = h end
    end
  end
  return out
end

-- The owned piece named in `names` (the first we hold), for the element rules.
local function owned_named(names)
  for _, n in ipairs(names) do
    for _, o in ipairs(state.owned or {}) do
      local rec = pick.H.item(o.id)
      if rec and rec.name == n then return { name = n } end
    end
  end
end

-- The element rules over a casting set's pieces: the element's staff (affinity; up to level 75, mages), the obi when
-- the day or the weather is the element.
local OBI_SETS = { Nuke = true, Nuke_MB = true, Cure = true, DrainAspir = true, Divine = true, Dark = true,
  Ninjutsu_Nuke = true, BluMag = true, QuickDraw = true, MagicAcc = true, Enfeebling_MND = true,
  Enfeebling_INT = true }
-- game.time().weekday (0 Firesday .. 7 Darksday) -> the day's element.
local DAY_ELEMENT = { [0] = "fire", "earth", "water", "wind", "ice", "thunder", "light", "dark" }

local function element_gear(pieces, set_name, el)
  if not el or not OBI_SETS[set_name] then return end
  if sets.MAGE_JOBS[state.job] and (state.level or 0) <= 75 then
    local staves = sets.STAVES[el]
    local staff = owned_named({ staves[2], staves[1] })
    if staff then pieces[0] = staff; pieces[1] = nil end
  end
  local t = M.H.time() or {}
  local weather = M.H.weather()
  local day_el = DAY_ELEMENT[t.weekday or -1]
  local w_el = weather and res and res.weather_element and sets.ELEMENT_ID[res.weather_element(weather) or -1]
  if day_el == el or w_el == el then
    local obi = owned_named({ sets.OBIS[el], sets.OBI_ALL })
    if obi then pieces[10] = obi end
  end
end

-- A weapon skill's gorget and belt by its skillchain elements (Fotia for any).
local function ws_gear(pieces, ws)
  if not ws or not ws.sc then return end
  local els = {}
  for _, p in ipairs(ws.sc) do
    for _, e in ipairs(sets.SC_ELEMENTS[p] or {}) do els[e] = true end
  end
  local neck, waist = { sets.FOTIA.neck }, { sets.FOTIA.waist }
  for e in pairs(els) do
    neck[#neck + 1] = sets.GORGETS[e]
    waist[#waist + 1] = sets.BELTS[e]
  end
  pieces[9] = owned_named(neck) or pieces[9]
  pieces[10] = owned_named(waist) or pieces[10]
end

-- The pieces as core.resolve wants.
local function wants(pieces)
  local w = {}
  for slot, p in pairs(pieces) do
    if p and p.name then w[slot] = { name = p.name, augments = p.augments } end
  end
  return w
end

-- Ammo stays while a bow, gun or crossbow is worn, or while the ammo is thrown (shuriken): another piece there would
-- break the next shot or throw.
local function keep_ammo()
  local worn = core.worn()
  local range = worn[2] and pick.H.item(worn[2].id)
  local ammo = worn[3] and pick.H.item(worn[3].id)
  return (range and (range.skill == 25 or range.skill == 26)) or (ammo and ammo.skill == 27)
end

local function wear(pieces, style)
  if pieces[3] and keep_ammo() then pieces[3] = nil end
  local list = core.resolve(wants(pieces), { log_names = false })
  core.send(core.packets(list, style or "auto", 8))
end

-- Wears a set now (the window's Wear Now).
function M.wear_set(name)
  wear(M.pieces(name), "set")
end

-- The set our status and toggles want between actions.
function M.default_set()
  local p = M.H.player() or {}
  local job_sets = {}
  for _, s in ipairs(M.job_sets(state.job or "")) do job_sets[s] = true end
  local modes = M.modes()
  local function first(...)
    for _, s in ipairs({ ... }) do
      if job_sets[s] then return s end
    end
  end
  if modes.pdt and job_sets.PDT then return "PDT" end
  if modes.mdt and job_sets.MDT then return "MDT" end
  if p.status == 1 then
    if modes.hybrid then return first("TP_Hybrid", "TP_MNK", "TP") end
    if modes.acc then return first("TP_Acc", "TP_MNK", "TP") end
    if M.H.pet() then return first("TP_Pet", "Automaton", "TP_MNK", "TP") end
    return first("TP_MNK", "TP")
  end
  if p.status == 33 then return first("Resting", "Idle") end
  if M.H.pet() then return first("Idle_Avatar", "Idle_Luopan", "Idle_Pet", "Idle") end
  return "Idle"
end

-- ---- Actions ----

local busy_until = 0
-- An action wore its sets: the status set goes back on once it ends.
local restore = false

-- Wearing the sets: the settings, and the player turned it on in the window (nothing moves after an update until
-- they do).
function M.loaded()
  return M.H.on() and M.active() and state.job ~= nil and state.ready
end

-- The sets an action wears: {precast, midcast} names, and its element / weapon skill.
function M.route(act)
  local job_sets = {}
  for _, s in ipairs(M.job_sets(state.job)) do job_sets[s] = true end
  local modes = M.modes()
  if act.category == 3 then
    local sp = M.H.spell(act.param)
    if not sp then return nil end
    sp.id = sp.id or act.param
    local r = sets.spell_sets(sp, blu)
    if r[2] == "Nuke" and modes.mb and job_sets.Nuke_MB then r[2] = "Nuke_MB" end
    if r[2] == "Nuke" and modes.macc and job_sets.MagicAcc then r[2] = "MagicAcc" end
    return { job_sets[r[1]] and r[1] or nil, job_sets[r[2]] and r[2] or nil, element = sets.ELEMENT_ID[sp.element or -1],
      cast = sp.cast_time }
  elseif act.category == 7 then
    local id = act.param
    local ws = wslist[id]
    local own = "WS:" .. id
    if job_sets[own] then return { nil, own, ws = ws } end
    if ws and ws.kind == "ranged" and job_sets.RangedWS then return { nil, "RangedWS", ws = ws } end
    return { nil, job_sets.WS and "WS" or nil, ws = ws }
  elseif act.category == 9 then
    local a = M.H.ability(core.res_id(act))
    if not a then return nil end
    local r = sets.ability_sets(a.name, job_sets)
    return { nil, r[1] }
  elseif act.category == 16 then
    return { job_sets.Preshot and "Preshot" or nil, job_sets.Midshot and "Midshot" or nil }
  end
end

function M.action(act)
  if not M.loaded() or act.kind == "item" then return end
  local r = M.route(act)
  if not r or (not r[1] and not r[2]) then return end
  if r[1] then wear(M.pieces(r[1]), "auto") end
  core.H.inject(act.packet)
  if r[2] then
    local pieces = M.pieces(r[2])
    element_gear(pieces, r[2], r.element)
    if r.ws then
      if r.ws.kind == "magic" then element_gear(pieces, "Nuke", r.ws.element) end
      ws_gear(pieces, r.ws)
    end
    wear(pieces, "single")
  end
  -- Held until the action ends (its finish or interruption below), at most its cast time and a margin.
  busy_until = clock() + math.max(3, (r.cast or 0) + 2)
  restore = true
  return false
end

local DONE = { [2] = true, [3] = true, [4] = true, [5] = true, [6] = true, [14] = true, [15] = true }
function M.on_action(a)
  if not M.loaded() then return end
  local p = M.H.player()
  if not p or a.actor ~= p.id then return end
  -- A finish, or an interrupted cast / shot (param 28787).
  if DONE[a.category] or ((a.category == 8 or a.category == 12) and a.param == 28787) then busy_until = 0 end
end

-- Between actions: the status's set, and the Movement set's moving pieces while we run.
local last_default, last_pos
local function moving()
  local p = M.H.player()
  local pos = p and p.pos
  if not pos then return false end
  local moved = last_pos and ((pos.x - last_pos.x) ^ 2 + (pos.z - last_pos.z) ^ 2) > 0.01
  last_pos = { x = pos.x, z = pos.z }
  return moved or false
end

-- The status set is worn when it changes (the status, a toggle, the level's picks, running) or after an action, not
-- every tick: a piece changed by hand stays on until then.
local last_sig
function M.tick()
  if not M.loaded() or clock() < busy_until then return end
  local name = M.default_set()
  local pieces = M.pieces(name)
  if moving() then
    local mv = state.built.Movement
    if mv then
      for slot, o in pairs(mv.slots) do
        if o.pick and (o.pick.s.stats.move or 0) > 0 then pieces[slot] = { name = pick.H.item(o.pick.id).name } end
      end
    end
  end
  last_default = name
  local sig = {}
  for slot = 0, 15 do sig[#sig + 1] = pieces[slot] and pieces[slot].name or "" end
  sig = name .. "|" .. table.concat(sig, "|")
  if sig == last_sig and not restore then return end
  last_sig, restore = sig, false
  wear(pieces, "auto")
end

function M.current() return last_default end

-- //gearshift mode <hybrid|acc|pdt|mdt|mb|macc> toggles one; "off" clears them.
function M.toggle(mode)
  local modes = M.modes()
  if mode == "off" then
    for k in pairs(modes) do modes[k] = nil end
    return "toggles off"
  end
  modes[mode] = not modes[mode] or nil
  return mode .. (modes[mode] and " on" or " off")
end

return M
