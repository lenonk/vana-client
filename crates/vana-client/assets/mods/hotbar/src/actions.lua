-- What a macro does first, for its slot's icon and recast: the first line naming a spell (/ma, /magic, /so, /song,
-- /nin), a job ability or pet command (/ja, /pet), a weapon skill (/ws), an item (/item, /equip) or one of the
-- Builder's system commands (/attack, /heal, ...: the game icon standing in for it).

local builder = require("src.builder")
local data = require("src.data")

local M = {}

local spells, abilities, weaponskills

local function index()
  if spells then return end
  spells, abilities, weaponskills = {}, {}, {}
  for id = 1, 1023 do
    local s = res.spell(id)
    if s and s.name and s.name ~= "" then spells[s.name:lower()] = spells[s.name:lower()] or id end
  end
  for id = 0, 0x7ff do
    local a = res.ability(id)
    if a and a.name and a.name ~= "" then
      local t = id < 0x200 and weaponskills or abilities
      t[a.name:lower()] = t[a.name:lower()] or id
    end
  end
end

local KIND = { ["/ma"] = "spell", ["/magic"] = "spell", ["/so"] = "spell", ["/song"] = "spell", ["/nin"] = "spell",
  ["/ninjutsu"] = "spell", ["/ja"] = "ability", ["/jobability"] = "ability", ["/pet"] = "ability",
  ["/ws"] = "weaponskill", ["/weaponskill"] = "weaponskill", ["/item"] = "item", ["/equip"] = "item" }

local cache = {}

-- {kind = 'spell' | 'ability' | 'item', id, recast_id} for the macro's first action, or false. A macro built on an
-- action whose own line names none (a line this does not read) takes the action's.
function M.of(macro)
  -- A group shows the member picked last.
  if macro.group then
    local m = data.current_member(macro)
    return m and M.of(m) or false
  end
  local key = table.concat(macro.lines or {}, "\n")
  local a = type(macro.action) == "table" and macro.action or nil
  if a then key = key .. "\0" .. tostring(a.kind) .. "\0" .. tostring(a.name) end
  local hit = cache[key]
  if hit ~= nil then return hit end
  hit = false
  for _, line in ipairs(macro.lines or {}) do
    -- /equip names its slot before the piece: /equip ring1 "Warp Ring".
    local cmd, name = line:match('^(/[Ee][Qq][Uu][Ii][Pp])%s+%w+%s+"([^"]+)"')
    if not cmd then cmd, name = line:match('^(/%a+)%s+"([^"]+)"') end
    if not cmd then cmd, name = line:match("^(/%a+)%s+(%S+)") end
    local kind = cmd and KIND[cmd:lower()]
    if kind then
      index()
      name = name:lower()
      if kind == "spell" and spells[name] then
        hit = { kind = "spell", id = spells[name] }
      elseif kind == "ability" and abilities[name] then
        local r = res.ability(abilities[name])
        hit = { kind = "ability", id = abilities[name], recast_id = r and r.recast_id }
      elseif kind == "weaponskill" and weaponskills[name] then
        hit = { kind = "ability", id = weaponskills[name], ws = true }
      elseif kind == "item" then
        local held = game.find_items(name)
        if held[1] then hit = { kind = "item", id = held[1].id } end
      end
      break
    end
    -- A system command (/attack <t>, /heal) shows the icon that stands in for it, with no recast.
    local bare = line:gsub("%s*<[^>]*>%s*$", ""):gsub("%s+$", ""):lower()
    local c = bare:find("^/") and builder.command(bare)
    if c then
      if c.icon then hit = { kind = c.icon.kind, id = c.icon.id, no_recast = true } end
      break
    end
  end
  if not hit and a and data.trim(a.line) ~= "" then
    local built = data.command({ kind = a.kind, name = a.name, target = a.target })
    if built then hit = M.of({ lines = { built } }) end
  end
  cache[key] = hit
  return hit
end

-- Seconds left on its recast and the fraction of the whole still to go (0 when ready).
function M.recast(a, recasts)
  if not a or a.no_recast then return 0, 0 end
  local left, total = 0, 0
  if a.kind == "spell" then
    left, total = recasts.spells[a.id] or 0, recasts.totals.spells[a.id] or 0
  elseif a.recast_id then
    left, total = recasts.abilities[a.recast_id] or 0, recasts.totals.abilities[a.recast_id] or 0
  end
  if left <= 0 then return 0, 0 end
  return left, total > 0 and math.min(1, left / total) or 1
end

-- Whether we can use it now, apart from its recast (shaded separately): a weapon skill needs 1000 TP, a pet command a
-- pet, a spell what the magic list asks (builder.castable) and its MP. Nil `a` (a macro with no known action) counts
-- as usable.
function M.usable(a, me)
  if not a or not me or a.no_recast then return true end
  if a.kind == "spell" then
    local s = res.spell(a.id)
    if not s then return true end
    if not builder.castable(a.id, me) then return false end
    return (s.mp or 0) <= (me.mp or 0)
  elseif a.kind == "ability" then
    if a.ws or a.id < 0x200 then return (me.tp or 0) >= 1000 end
    if a.id >= 0x400 then
      local s = game.info()
      return s ~= nil and (s.pet_index or 0) ~= 0
    end
  end
  return true
end

return M
