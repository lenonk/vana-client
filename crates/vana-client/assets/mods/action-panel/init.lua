-- The job's actions in one window (Rhombus, wizardui, smnbar): magic by school, job abilities, weapon skills, pet
-- commands, each with its cost and recast; a pick runs the action's text command as a macro line would, so the
-- game's checks and its target prompt apply.

local WIN = "actions"
local TABS = { "Magic", "Abilities", "Weapon Skills", "Pet" }
local SCHOOLS = { "White Magic", "Black Magic", "Summoning Magic", "Ninjutsu", "Songs", "Blue Magic", "Geomancy", "Trust" }
local DIM = { 150, 150, 150, 255 }
local BLU = 16

local view = { tab = 1, school = nil, at = 1, schools = {} }
local shown = {}   -- the rows' actions as listed: {kind, id}

local function say(t) chat.add("Action Panel: " .. t) end

local function level_ok(levels, p)
  if not levels then return false end
  local m, s = levels[p.main_job or 0], levels[p.sub_job or 0]
  return (m and m <= (p.main_level or 0)) or (s and s <= (p.sub_level or 0)) or false
end

-- Blue magic is usable only while set.
local function blue_set()
  local out = {}
  local d = game.job_data()
  for _, side in ipairs({ "main", "sub" }) do
    local j = d and d[side]
    if j and j.job == BLU then
      for _, id in ipairs(j.blue_spells or {}) do out[id] = true end
    end
  end
  return out
end

-- The spells we can cast now, by school (res.spell's kind 1..8).
local function spells_by_school()
  local p = game.player()
  local out = {}
  if not p then return out end
  local blue = blue_set()
  for _, id in ipairs(game.spells()) do
    local r = res.spell(id)
    local k = r and r.name and r.kind
    if k and SCHOOLS[k] and (k == 8 or (k == 6 and blue[id]) or (k ~= 6 and level_ok(r.levels, p))) then
      out[k] = out[k] or {}
      table.insert(out[k], { kind = "spell", id = id, name = r.name, cost = r.mp, unit = "MP" })
    end
  end
  for _, list in pairs(out) do table.sort(list, function(a, b) return a.name < b.name end) end
  return out
end

local TRAIT, MONSTROSITY, READY = 4, 0x14, 0x12
-- The cost the game's ability list shows by the record's kind (the client's table 0x10386159): TP for the dances and
-- other TP moves, MP for blood pacts. Ready moves show their charges.
local TP_COST = { [11] = true, [12] = true, [13] = true, [16] = true }
local MP_COST = { [6] = true, [10] = true }

local function cost_of(r)
  if TP_COST[r.kind] and r.tp ~= 255 then return r.tp, "TP" end
  if MP_COST[r.kind] then return r.cost, "MP" end
  if r.kind == READY then return r.cost, r.cost == 1 and "charge" or "charges" end
  return 0, ""
end

-- Abilities, weapon skills and pet commands we can use now: not traits, Monstrosity moves (in their list) or the
-- rows that only open a sub-list (Phantom Roll; its rolls are listed themselves); pet commands only with a pet.
local function abilities_by_tab()
  local out = { [2] = {}, [3] = {}, [4] = {} }
  local pet = game.info().pet_index ~= nil
  for _, id in ipairs(game.abilities()) do
    local r = res.ability(id)
    if r and r.name and r.name ~= "" and r.kind ~= TRAIT and r.kind ~= MONSTROSITY and (r.sub or 0) == 0 then
      local tab = id < 0x200 and 3 or ((id < 0x400 and r.kind ~= READY) and 2 or 4)
      if tab ~= 4 or pet then
        local cost, unit = cost_of(r)
        table.insert(out[tab], { kind = tab == 3 and "ws" or (tab == 4 and "pet" or "ja"), id = id, name = r.name,
          cost = cost, unit = unit, recast_id = r.recast_id })
      end
    end
  end
  for _, list in pairs(out) do table.sort(list, function(a, b) return a.name < b.name end) end
  return out
end

local function recast_of(a, rc)
  if a.kind == "spell" then return rc.spells[a.id] or 0 end
  if a.recast_id then return rc.abilities[a.recast_id] or 0 end
  return 0
end

local function clock_text(s)
  if s <= 0 then return "" end
  s = math.ceil(s)
  return s >= 60 and ("%d:%02d"):format(s // 60, s % 60) or tostring(s)
end

local function list_now()
  if view.tab == 1 then
    local by = spells_by_school()
    local schools = {}
    for k = 1, #SCHOOLS do if by[k] then schools[#schools + 1] = k end end
    if not view.school or not by[view.school] then view.school = schools[1] end
    return view.school and by[view.school] or {}, schools
  end
  return abilities_by_tab()[view.tab] or {}, nil
end

local function rows(list)
  local rc = game.recasts()
  local out = {}
  for _, a in ipairs(list) do
    local left = recast_of(a, rc)
    local cost = (a.cost and a.cost > 0) and (a.cost .. (a.unit ~= "" and (" " .. a.unit) or "")) or ""
    out[#out + 1] = { a.name, cost, clock_text(left), colour = left > 0 and DIM or nil,
      icon = { kind = a.kind == "spell" and "spell" or "ability", id = a.id } }
  end
  return out
end

local function show()
  local list, schools = list_now()
  shown, view.schools = list, schools or {}
  local els = { { type = "tabs", id = "tabs", items = TABS, at = view.tab } }
  if schools and #schools > 0 then
    local names, at = {}, 1
    for i, k in ipairs(schools) do
      names[i] = SCHOOLS[k]
      if k == view.school then at = i end
    end
    els[#els + 1] = { type = "choice", id = "school", label = "School", items = names, at = at, page = 1 }
  end
  els[#els + 1] = { type = "list", id = "list", rows = rows(list), height = 12, at = math.min(view.at, math.max(1, #list)),
    columns = { { title = "Action", width = 0 }, { title = "Cost", width = 70, align = "right" }, { title = "Recast", width = 64, align = "right" } } }
  if #list == 0 then els[#els + 1] = { type = "text", text = "Nothing to use here now.", dim = true } end
  els[#els + 1] = { type = "buttons", id = "close", items = { "Close" } }
  -- A HUD window: it never takes the keys, so the game's keys (the main menu, Escape, targeting, macros) keep working
  -- while it is open; its rows and tabs work with the mouse.
  ui.window(WIN, { title = "Actions", width = 420, hud = true, elements = els })
end

-- Its macro line: self-only actions on us, the rest through the target prompt.
local function line_of(a)
  local r = a.kind == "spell" and res.spell(a.id) or res.ability(a.id)
  local tag = (r and r.targets == 1) and "<me>" or "<st>"
  local cmd = ({ spell = "/ma", ja = "/ja", ws = "/ws", pet = "/pet" })[a.kind]
  return ('%s "%s" %s'):format(cmd, a.name, tag)
end

ui.on(WIN, function(_, e)
  if e.kind == "set" and e.id == "tabs" then
    view.tab, view.at = e.index, 1
    show()
  elseif e.kind == "set" and e.id == "school" then
    view.school, view.at = view.schools[e.index], 1
    show()
  elseif e.kind == "select" and e.id == "list" then
    view.at = e.index
    local a = shown[e.index]
    if a then commands.run(line_of(a)) end
  elseif e.kind == "press" and e.id == "close" then
    ui.close(WIN)
  end
end)

-- The actions' ids in order, to see when the list changes.
local function ids(list)
  local out = {}
  for i, a in ipairs(list) do out[i] = a.kind .. a.id end
  return table.concat(out, ",")
end

-- The recasts count down while it is open; a changed list (a pet called or gone, a level gained, a blue spell set)
-- is shown again.
timer.every(0.5, function()
  if not ui.is_open(WIN) then return end
  local list, schools = list_now()
  if ids(list) ~= ids(shown) or table.concat(schools or {}, ",") ~= table.concat(view.schools, ",") then
    show()
  else
    ui.update(WIN, "list", { rows = rows(shown) })
  end
end)

events.on("job_change", function() if ui.is_open(WIN) then show() end end)

-- Switched off: the window goes too.
events.on("settings_changed", function() if not settings.get("on") then ui.close(WIN) end end)

commands.register("actions", function()
  if not settings.get("on") then return end
  if ui.is_open(WIN) then ui.close(WIN) return end
  show()
  ui.show(WIN)
  if #shown == 0 and view.tab == 1 then say("no magic to use now; the other tabs hold abilities and weapon skills.") end
end)
