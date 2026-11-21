-- The set builder's window (//gearshift): the job's sets, each slot's piece and how much of the best-in-slot
-- reference it gives, what is worth getting; a slot picked by hand stays through rebuilds ("Automatic" frees it).

local run = require("src.build.run")
local pick = require("src.build.pick")
local sets = require("src.build.sets")

local M = {}
local WIN = "gsbuilder"
local SLOT_LABEL = { [0] = "Main", "Sub", "Range", "Ammo", "Head", "Body", "Hands", "Legs", "Feet", "Neck", "Waist",
  "L. Ear", "R. Ear", "L. Ring", "R. Ring", "Back" }
local BAG_LABEL = { [1] = "Mog Safe", [2] = "Storage", [4] = "Mog Locker", [5] = "Mog Satchel", [6] = "Mog Sack",
  [7] = "Mog Case", [9] = "Mog Safe 2" }
-- Score bands against the reference.
local GOOD, FAIR, LOW = { 240, 170, 60, 255 }, { 230, 220, 110, 255 }, { 120, 170, 230, 255 }

local view = { set = 1, slot = nil }

local function item_name(id)
  local r = id and pick.item(id)
  return r and r.name or "-"
end

-- The stats a piece gives the set, most valued first: "MAB+3 INT+2".
local LABEL = { mab = "MAB", macc = "Mag. Acc.", acc = "Acc.", att = "Atk.", racc = "Rng. Acc.", ratt = "Rng. Atk.",
  stp = "Store TP", da = "Dbl. Atk.", ta = "Tpl. Atk.", qa = "Quad. Atk.", wsd = "WS Dmg.", fc = "Fast Cast",
  dt = "DT", pdt = "PDT", mdt = "MDT", mdb = "MDB", meva = "Mag. Eva.", haste = "Haste", crit = "Crit.",
  cure = "Cure Pot.", cure2 = "Cure Pot. II", refresh = "Refresh", regen = "Regen", enmity = "Enmity",
  mbb = "MB Dmg.", mbb2 = "MB Dmg. II", mdmg = "Mag. Dmg.", move = "Move" }
local PCT = { da = true, ta = true, qa = true, wsd = true, fc = true, dt = true, pdt = true, mdt = true, haste = true,
  crit = true, cure = true, cure2 = true, move = true }
local function summary(s, def, n)
  if not s then return "" end
  local list = {}
  for k, w in pairs(def.weights) do
    local v = k:find(".", 1, true) and 0 or (s.stats[k] or 0)
    if v ~= 0 and w * v > 0 then list[#list + 1] = { k = k, v = v, score = w * v } end
  end
  table.sort(list, function(a, b) return a.score > b.score end)
  local out = {}
  for i = 1, math.min(n or 4, #list) do
    local x = list[i]
    out[#out + 1] = (LABEL[x.k] or x.k:upper()) .. (x.v > 0 and "+" or "") .. x.v .. (PCT[x.k] and "%" or "")
  end
  return table.concat(out, " ")
end

local function job_sets()
  return run.state.job and run.job_sets(run.state.job) or {}
end

local function set_label(name)
  local def = run.set_def(name)
  return def and def.label or name
end

local function current()
  local list = job_sets()
  local name = list[view.set] or list[1]
  return name, name and run.set_def(name)
end

-- We hold a piece of that name in the bags gear is worn from.
local function held(name)
  for _, o in ipairs(run.state.owned or {}) do
    local rec = pick.item(o.id)
    if rec and rec.name == name then return true end
  end
  return false
end

local function slot_rows(name, def, r)
  local hand = run.hand(run.state.job, name)
  local rows = {}
  for slot = 0, 15 do
    local o = r and r.slots[slot]
    local h = hand[pick.SLOT_NAMES[slot]]
    local wearing, share, ref, worth = "-", "", "", ""
    if h then
      wearing = h.name .. " *" .. (held(h.name) and "" or " (not held)")
    elseif o and o.pick then
      wearing = item_name(o.pick.id)
    elseif not (sets.MAGE_JOBS[run.state.job] and not sets.MELEE_SETS[name] and not def.ws) and slot <= 2 then
      wearing = "(kept)"
    end
    if o then
      if o.share and o.pick and not h then share = ("%d%%"):format(math.floor(o.share * 100 + 0.5)) end
      if o.ref then ref = item_name(o.ref.id) end
      if o.worth and o.worth_bag then
        worth = item_name(o.worth.id) .. " (in " .. (BAG_LABEL[o.worth_bag] or "storage") .. ")"
      elseif o.worth then
        worth = item_name(o.worth.id) .. " (" .. ((pick.item(o.worth.id) or {}).level or "?") .. ")"
      end
    end
    local icon_id = (o and o.pick and o.pick.id)
    rows[#rows + 1] = { SLOT_LABEL[slot], wearing, share, ref, worth,
      icon = icon_id and { kind = "item", id = icon_id } or nil }
  end
  return rows
end

-- Better pieces kept where they cannot be worn from.
local function warnings(def, r)
  local out = {}
  local p = run.H.player() or {}
  local me = run.H.me() or {}
  for _, e in ipairs(run.state.elsewhere or {}) do
    local rec = pick.item(e.id)
    for slot = 0, 15 do
      local o = r and r.slots[slot]
      if o and pick.fits(rec, slot, p.main_job or 0, me.race, run.state.level) then
        local s = pick.stats_of(e.id, e.augments)
        local mine = o.pick and pick.score(o.pick.s, def) or 0
        if pick.score(s, def) > mine and #out < 3 then
          out[#out + 1] = ("%s is in your %s (%s)."):format(rec.name, BAG_LABEL[e.bag] or "bags", SLOT_LABEL[slot])
          break
        end
      end
    end
  end
  return table.concat(out, " ")
end

-- The set's score against the best this level allows (the colour), and the cap's best in slot.
local function score_line(r)
  if not r then return "Reading your gear and the item list...", nil end
  local reach = r.reach_score or 0
  if reach <= 0 then return ("Score %d"):format(math.floor(r.score + 0.5)), nil end
  local pct = r.score / reach * 100
  local colour = pct >= 90 and GOOD or (pct >= 70 and FAIR or LOW)
  return ("Score %d: %d%% of the best for level %d (%d); best in slot at %d: %d"):format(math.floor(r.score + 0.5),
    math.floor(pct + 0.5), run.state.level or 0, math.floor(reach + 0.5), run.cap(), math.floor(r.ref_score + 0.5)), colour
end

local function detail(name, def, r)
  if not view.slot or not r then return "Pick a slot to see its piece next to the best in slot, and its alternatives." end
  local o = r.slots[view.slot]
  if not o then return "" end
  local parts = { SLOT_LABEL[view.slot] .. ":" }
  if o.pick then parts[#parts + 1] = item_name(o.pick.id) .. " " .. summary(o.pick.s, def) end
  if o.ref then parts[#parts + 1] = "| best in slot " .. item_name(o.ref.id) .. " " .. summary(o.ref.s, def) end
  if o.worth and o.worth_bag then
    parts[#parts + 1] = "| in your " .. (BAG_LABEL[o.worth_bag] or "storage") .. " (move it to a wardrobe to wear it): "
      .. item_name(o.worth.id) .. " " .. summary(o.worth.s, def)
  elseif o.worth then
    parts[#parts + 1] = "| worth getting " .. item_name(o.worth.id) .. " " .. summary(o.worth.s, def)
  end
  return table.concat(parts, " ")
end

function M.draw()
  local list = job_sets()
  local name, def = current()
  local labels = {}
  for i, s in ipairs(list) do labels[i] = set_label(s) end
  local r = name and run.state.built[name]
  if name and not r then run.want_first(name) end
  local score, colour = score_line(r)
  local title = run.state.job and ("Gearshift: %s %d"):format(run.state.job, run.state.level or 0) or "Gearshift"
  ui.window(WIN, {
    title = title, width = 800,
    help = "Your sets, built from the gear you hold for your level. Pick a slot to choose its piece yourself.",
    elements = {
      { type = "choice", id = "active", label = "Wear These Sets", items = { "Off", "On" }, at = run.active() and 2 or 1,
        tip = "On: Gearshift wears these sets around your actions and between them. Off: they are only shown." },
      { type = "dropdown", id = "set", label = "Set", items = #labels > 0 and labels or { "-" }, at = view.set,
        tip = "The job's sets: what each is worn for is in its line below." },
      { type = "text", id = "when", text = def and ("Worn " .. (def.when or "") .. ".") or "", dim = true },
      { type = "text", id = "score", text = score, colour = colour or false },
      { type = "list", id = "slots", rows = name and def and slot_rows(name, def, r) or {}, height = 16,
        at = view.slot and (view.slot + 1) or nil,
        columns = { { title = "Slot", width = 110 }, { title = "Wearing", width = 190 }, { title = "Match", width = 80,
          align = "right" }, { title = "Best in slot", width = 180 }, { title = "Worth getting", width = 200 } } },
      { type = "text", id = "detail", text = name and def and detail(name, def, r) or "", dim = true },
      { type = "text", id = "warn", text = name and def and warnings(def, r) or "", colour = FAIR },
      { type = "buttons", id = "actions", items = { "Rebuild", "Wear Now", "Close" } },
    },
  })
end

-- The alternatives for a slot: the owned pieces that fit, best share first, then "Automatic".
local function alternatives(name, def, slot)
  local r = run.state.built[name]
  local o = r and r.slots[slot]
  local p = run.H.player() or {}
  local me = run.H.me() or {}
  local seen, cands = {}, {}
  for _, e in ipairs(run.state.owned or {}) do
    local rec = pick.item(e.id)
    if pick.fits(rec, slot, p.main_job or 0, me.race, run.state.level) and not seen[e.key] then
      seen[e.key] = true
      local s = pick.stats_of(e.id, e.augments)
      local sh = o and o.ref and (pick.share(s, o.ref.s, def)) or 0
      cands[#cands + 1] = { name = rec.name, augments = e.augments, share = sh, score = pick.score(s, def) }
    end
  end
  table.sort(cands, function(a, b)
    if a.share ~= b.share then return a.share > b.share end
    return a.score > b.score
  end)
  return cands
end

local function choose_slot(slot)
  local name, def = current()
  if not name then return end
  local cands = alternatives(name, def, slot)
  local items = { "Automatic" }
  for _, c in ipairs(cands) do
    items[#items + 1] = ("%s (%d%%)"):format(c.name, math.floor(c.share * 100 + 0.5))
  end
  ui.menu(items, function(i)
    if not i then return end
    local hand = run.hand(run.state.job, name)
    if i == 1 then
      hand[pick.SLOT_NAMES[slot]] = nil
    else
      local c = cands[i - 1]
      hand[pick.SLOT_NAMES[slot]] = { name = c.name, augments = c.augments }
    end
    M.draw()
  end)
end

function M.toggle()
  if ui.is_open(WIN) then ui.close(WIN) return end
  M.draw()
  ui.open(WIN)
end

function M.redraw()
  if ui.is_open(WIN) then M.draw() end
end

function M.install()
  ui.on(WIN, function(_, e)
    if e.kind == "set" and e.id == "active" then
      run.set_active(e.index == 2)
      M.draw()
    elseif e.kind == "set" and e.id == "set" then
      view.set, view.slot = e.index, nil
      M.draw()
    elseif e.kind == "select" and e.id == "slots" then
      view.slot = e.index - 1
      M.draw()
      choose_slot(view.slot)
    elseif e.kind == "press" and e.id == "actions" then
      if e.index == 1 then
        run.rebuild()
        M.draw()
      elseif e.index == 2 then
        local name = current()
        if name then run.wear_set(name) end
      else
        ui.close(WIN)
      end
    end
  end)
end

return M
