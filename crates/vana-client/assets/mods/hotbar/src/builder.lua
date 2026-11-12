-- What the macro builder offers: the actions of each type you can use now, found by name, and the targets (or, to
-- equip, the slots) a built line can name.

local M = {}

-- Builder types, in the order the editor offers them; `kind` is the action's (nil: a macro of typed lines only).
M.TYPES = {
  { kind = "ma", label = "Spell", search = "Search your spells", column = "Spell" },
  { kind = "ja", label = "Job ability", search = "Search your job abilities", column = "Ability" },
  { kind = "ws", label = "Weapon skill", search = "Search your weapon skills", column = "Weapon skill" },
  { kind = "pet", label = "Pet command", search = "Search your pet commands", column = "Command" },
  { kind = "item", label = "Item", search = "Search your usable items", column = "Item" },
  { kind = "equip", label = "Equip", search = "Search your equipment", column = "Equipment" },
  { kind = "cmd", label = "Command", search = "Search the commands", column = "Command" },
  { kind = nil, label = "Macro", search = "(not used for a macro: type its lines)", column = "" },
}

-- Game icons that stand in for the commands: an item's, a status effect's or a spell's.
local ITEM_ICON = function(id) return { kind = "item", id = id } end
local STATUS_ICON = function(id) return { kind = "status", id = id } end

-- The common system commands, as typed (arguments included), with the target each starts on (nil: it takes none)
-- and the icon a slot shows for it (none for some).
M.COMMANDS = {
  { line = "/attack", title = "Attack", target = "t", does = "Engage the target, or switch to it",
    icon = ITEM_ICON(16535) },                                          -- Bronze Sword
  { line = "/attackoff", title = "Disengage", does = "Stop attacking", icon = ITEM_ICON(12289) }, -- Lauan Shield
  { line = "/ra", title = "Ranged attack", target = "t", does = "Shoot or throw at the target",
    icon = ITEM_ICON(17152) },                                          -- Shortbow
  { line = "/assist", title = "Assist", target = "t", does = "Target what they are targeting",
    icon = ITEM_ICON(515) },                                            -- Linkpearl
  { line = "/target", title = "Target", target = "stnpc", does = "Pick a target", icon = STATUS_ICON(58) }, -- Aggressor
  { line = "/targetbnpc", title = "Nearest enemy", does = "Target the nearest enemy", icon = STATUS_ICON(58) },
  { line = "/targetnpc", title = "Nearest NPC", does = "Target the nearest NPC" },
  { line = "/lockon", title = "Lock on", does = "Lock on to the target, or let go", icon = STATUS_ICON(11) }, -- Bind
  { line = "/check", title = "Check", target = "t", does = "Check the target", icon = STATUS_ICON(59) }, -- Focus
  { line = "/follow", title = "Follow", target = "t", does = "Follow the target", icon = STATUS_ICON(71) }, -- Sneak
  { line = "/heal", title = "Heal", does = "Rest, or stop resting", icon = STATUS_ICON(42) },     -- Regen
  { line = "/sit", title = "Sit", does = "Sit down, or stand" },
  { line = "/dismount", title = "Dismount", does = "Get off your mount", icon = ITEM_ICON(15533) }, -- Chocobo Whistle
  { line = "/refa all", title = "Release trusts", does = "Release all your trusts",
    icon = { kind = "spell", id = 896 } },                              -- a trust's (Shantotto)
  { line = "/lockstyle on", title = "Lockstyle", does = "Lock your look to what you wear" },
  { line = "/wait 1", title = "Wait", does = "Wait a second before the next line" },
}

local NO_TARGET = { { "", "(none)" } }

-- A command of M.COMMANDS by its line.
function M.command(line)
  for _, c in ipairs(M.COMMANDS) do
    if c.line == line then return c end
  end
  return nil
end

M.TYPE_LABELS = {}
for i, t in ipairs(M.TYPES) do
  M.TYPE_LABELS[i] = t.label
end

function M.type_index(kind)
  for i, t in ipairs(M.TYPES) do
    if t.kind == kind then return i end
  end
  return #M.TYPES
end

M.TARGETS = {
  { "t", "<t> Current target" },
  { "me", "<me> Yourself" },
  { "bt", "<bt> Battle target" },
  { "stpc", "<stpc> Pick a player" },
  { "stpt", "<stpt> Pick a party member" },
  { "stal", "<stal> Pick an alliance member" },
  { "stnpc", "<stnpc> Pick an NPC or enemy" },
  { "st", "<st> Pick anyone" },
  { "lastst", "<lastst> Last picked" },
  { "p1", "<p1> Party member 2" },
  { "p2", "<p2> Party member 3" },
  { "p3", "<p3> Party member 4" },
  { "p4", "<p4> Party member 5" },
  { "p5", "<p5> Party member 6" },
}

-- Equipment slots by bit (res.item's `slots`), with the names /equip takes.
M.EQUIP_SLOTS = {
  { "main", "Main" }, { "sub", "Sub" }, { "range", "Range" }, { "ammo", "Ammo" }, { "head", "Head" },
  { "body", "Body" }, { "hands", "Hands" }, { "legs", "Legs" }, { "feet", "Feet" }, { "neck", "Neck" },
  { "waist", "Waist" }, { "ear1", "Left ear" }, { "ear2", "Right ear" }, { "ring1", "Left ring" },
  { "ring2", "Right ring" }, { "back", "Back" },
}

-- The choices of the target menu for an action kind (and, for a command, the one picked): { {value, label}, ... }.
-- A command that takes no target has only "(none)" (value "").
function M.targets_for(kind, name)
  if kind == "equip" then return M.EQUIP_SLOTS end
  if kind == "cmd" then
    local c = name and M.command(name)
    if c and not c.target then return NO_TARGET end
  end
  return M.TARGETS
end

-- The label of a target value for an action kind.
function M.target_label(kind, value, name)
  for _, t in ipairs(M.targets_for(kind, name)) do
    if t[1] == value then return t[2] end
  end
  return value and value ~= "" and ("<" .. value .. ">") or "(none)"
end

local SPELL_KINDS = { "White", "Black", "Summoning", "Ninjutsu", "Song", "Blue", "Geomancy", "Trust" }

-- A target from valid-target bits (spells and items): an enemy for one that can hit enemies, yourself for one that
-- only takes you, a party (or alliance) member for one that takes no other player (Refresh: 0x05), else a player.
function M.default_target(targets)
  local t = targets or 0
  if t & 0x20 ~= 0 then return "t" end
  if t & ~1 == 0 then return "me" end
  if t & 0x12 == 0 then return t & 8 ~= 0 and "stal" or "stpt" end
  return "stpc"
end

-- The first slot an item fits, by its `slots` bits.
local function first_slot(bits)
  for n = 0, 15 do
    if bits & (1 << n) ~= 0 then return M.EQUIP_SLOTS[n + 1][1] end
  end
  return "main"
end

local ABILITY_KIND = { ja = 1, pet = 2, ws = 3 }
local DEFAULT_TARGET = { ja = "me", pet = "t", ws = "t" }

-- The rows that open a sub-list instead of acting (Rune Enchantment, Phantom Roll, Blood Pact: Rage, Ready...), by the
-- kind of the rows in their list: { [kind] = { name, kind = the opener's kind } }. /ja cannot use an opener; the
-- rows of its list are what a macro names (/ja "Ignis" <me>).
local openers_by_kind

local function openers()
  if not openers_by_kind then
    openers_by_kind = {}
    for id = 0, 0x7ff do
      local a = res.ability(id)
      if a and (a.sub or 0) ~= 0 and a.name and a.name ~= "" then
        openers_by_kind[a.sub] = { name = a.name, kind = a.kind }
      end
    end
  end
  return openers_by_kind
end

-- Whether blue magic `id` is set (only set blue magic can be cast), from the BLU side's job data; cached on `me`.
local function blue_set(id, me)
  if not me._blue then
    me._blue = {}
    local d = game.job_data and game.job_data() or {}
    for _, side in ipairs({ d.main, d.sub }) do
      for _, s in ipairs(side and side.blue_spells or {}) do me._blue[s] = true end
    end
  end
  return me._blue[id] == true
end

-- Whether spell `id` is one the magic list offers: a job that has it at our level (as main for SCH's and GEO's
-- main-only spells and gifts), blue magic set. An unknown spell or player counts as castable.
function M.castable(id, me)
  local s = res.spell(id)
  if not s or not me then return true end
  if s.kind ~= 8 and s.levels then
    if s.main_only and s.main_only ~= me.main_job then return false end
    local jp = s.job_points or {}
    local lv = function(job, level, main)
      if not (job and s.levels[job]) then return false end
      -- A gift: main job at 99 (the job points it takes are not known here).
      if jp[job] then return main and (level or 0) >= 99 end
      return s.levels[job] <= (level or 0)
    end
    if jp[me.main_job] then
      if not lv(me.main_job, me.main_level, true) then return false end
    elseif not (lv(me.main_job, me.main_level, true) or lv(me.sub_job, me.sub_level, false)) then
      return false
    end
  end
  return not (id >= 0x200 and id < 0x2e0) or blue_set(id, me)
end

-- Every choice of an action kind: { { name, cells = {...}, icon = {kind, id}, target, title?, tip? }, ... } where
-- `target` is the one a new pick starts on, `title` the macro name it suggests (else `name`) and `tip` its description.
local function all_of(kind)
  local out = {}
  if kind == "cmd" then
    -- In the list's order, not by name: the common ones first.
    for _, c in ipairs(M.COMMANDS) do
      out[#out + 1] = { name = c.line, title = c.title, target = c.target or "", icon = c.icon,
        cells = { c.target and ("<" .. c.target .. ">") or "", c.does }, search = c.title .. " " .. c.does }
    end
    return out
  elseif kind == "ma" then
    -- Learned spells our jobs can cast at their levels, as the magic list shows.
    local me = game.player()
    for _, id in ipairs(game.spells()) do
      local s = res.spell(id)
      if s and s.name and s.name ~= "" and M.castable(id, me) then
        out[#out + 1] = { name = s.name, cells = { tostring(s.mp or 0), SPELL_KINDS[s.kind] or "" },
          icon = { kind = "spell", id = id }, target = M.default_target(s.targets), tip = s.description }
      end
    end
  elseif ABILITY_KIND[kind] then
    -- An opener's row stands for its list (`list` its rows' kind); a row of a list names its opener (`under`).
    local seen, op = {}, openers()
    for _, id in ipairs(game.abilities()) do
      local a = res.ability(id)
      local under = a and op[a.kind]
      local of = under and under.kind or (a and a.kind)
      if a and of == ABILITY_KIND[kind] and a.name and a.name ~= "" and not seen[a.name] then
        seen[a.name] = true
        local icon = { kind = "ability", id = id }
        if (a.sub or 0) ~= 0 then
          out[#out + 1] = { name = a.name, list = a.sub, cells = { "", "" }, icon = icon, tip = a.description }
        else
          local cost = (a.cost or 0) > 0 and tostring(a.cost) or ""
          local recast = a.recast and ("%ds"):format(a.recast) or ""
          local target = (a.targets or 0) ~= 0 and M.default_target(a.targets) or DEFAULT_TARGET[kind]
          out[#out + 1] = { name = a.name, cells = { cost, recast }, icon = icon, target = target,
            under = under and under.name, kind = a.kind, tip = a.description }
        end
      end
    end
  elseif kind == "item" or kind == "equip" then
    -- Usable items from the inventory and the temporary items; equipment from the inventory and the wardrobes.
    local bags = kind == "item" and { 0, 3 } or { 0, 8, 10, 11, 12, 13, 14, 15, 16 }
    local by_id = {}
    for _, bag in ipairs(bags) do
      for _, it in ipairs(game.items(bag)) do
        local r = it.id and res.item(it.id)
        local fits = r and (kind == "equip" and (r.slots or 0) ~= 0 or kind == "item" and r.kind == "usable")
        if fits and r.name and r.name ~= "" then
          local e = by_id[it.id]
          if not e then
            e = { name = r.name, count = 0, icon = { kind = "item", id = it.id }, tip = r.description,
              target = kind == "equip" and first_slot(r.slots) or M.default_target(r.targets) }
            by_id[it.id] = e
            out[#out + 1] = e
          end
          e.count = e.count + (it.count or 1)
        end
      end
    end
    for _, e in ipairs(out) do
      e.cells = { tostring(e.count), kind == "equip" and M.target_label("equip", e.target) or "" }
    end
  end
  table.sort(out, function(a, b) return a.name < b.name end)
  return out
end

M.MAX_RESULTS = 150

-- Whether action `name` of a kind can take only yourself (a rune, Utsusemi...): a group's Target leaves it on <me>.
function M.self_only(kind, name)
  if not ABILITY_KIND[kind] and kind ~= "ma" then return false end
  for _, e in ipairs(all_of(kind)) do
    if e.name == name and e.icon then
      local r = e.icon.kind == "spell" and res.spell(e.icon.id) or e.icon.kind == "ability" and res.ability(e.icon.id)
      local t = r and r.targets or 0
      return t ~= 0 and t & ~1 == 0
    end
  end
  return false
end

-- The choices of an action kind whose names hold `query` (any case), by name; `more` is true when there were more
-- than MAX_RESULTS. With `list` (an opener's), the rows of that list; else the openers and the other rows, and with a
-- query the rows of every list too.
function M.find(kind, query, list)
  local q = tostring(query or ""):lower():gsub("^%s+", ""):gsub("%s+$", "")
  local found = {}
  for _, e in ipairs(all_of(kind)) do
    local here
    if list then
      here = e.kind == list and not e.list
    else
      here = q ~= "" or not e.under
    end
    if here and (q == "" or e.name:lower():find(q, 1, true) or (e.search and e.search:lower():find(q, 1, true))) then
      found[#found + 1] = e
    end
  end
  -- An opener's rows in the game's order (Ignis, Gelus, ... Tenebrae), as its menu lists them.
  if list then
    table.sort(found, function(a, b) return (a.icon and a.icon.id or 0) < (b.icon and b.icon.id or 0) end)
  end
  local more = #found > M.MAX_RESULTS
  while #found > M.MAX_RESULTS do
    found[#found] = nil
  end
  return found, more
end

-- The column headings of the results list for an action kind.
function M.columns(kind)
  local t = M.TYPES[M.type_index(kind)]
  local second, third = "", ""
  if kind == "ma" then
    second, third = "MP", "Type"
  elseif ABILITY_KIND[kind] then
    second, third = "Cost", "Recast"
  elseif kind == "item" then
    second = "Held"
  elseif kind == "equip" then
    second, third = "Held", "Slot"
  elseif kind == "cmd" then
    return { { title = t.column }, { title = "Target", width = 60 }, { title = "What it does", width = 230 } }
  end
  return { { title = t.column }, { title = second, width = 50, align = "right" }, { title = third, width = 90 } }
end

return M
