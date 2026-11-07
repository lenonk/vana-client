-- A bar of spells and abilities. Click a slot to use it. Drag a slot onto another with the left button to swap them,
-- or drag the bar with the right button to move it. Rest the pointer on a slot for its name and recast; the strip
-- under each slot fills as its recast runs out. /bar <slot> spell|ability <id> puts something in a slot.

local SIZE, GAP = 40, 6

storage.character.slots = storage.character.slots or {
  { kind = "ability", id = 0x200 + 35 }, -- Provoke
  { kind = "ability", id = 0x200 + 31 }, -- Berserk
  { kind = "spell", id = 1 }, -- Cure
  { kind = "spell", id = 2 }, -- Cure II
}
storage.character.at = storage.character.at or { x = 400, y = 120 }
local slots, at = storage.character.slots, storage.character.at

local function name_of(s)
  local r = s.kind == "spell" and res.spell(s.id) or res.ability(s.id)
  return r and r.name or "?"
end

-- Seconds left on a slot's recast, and the recast's whole length (nil when it is ready).
local function recast(s, r)
  if s.kind == "spell" then
    return r.spells[s.id] or 0, r.totals.spells[s.id]
  end
  local a = res.ability(s.id)
  if not a then return 0, nil end
  return r.abilities[a.recast_id] or 0, r.totals.abilities[a.recast_id]
end

-- Every slot's elements, at the bar's place (dx, dy while it is dragged).
local function build(dx, dy)
  for i, s in ipairs(slots) do
    local id = "slot" .. i
    local x, y = at.x + (dx or 0) + (i - 1) * (SIZE + GAP), at.y + (dy or 0)
    hud.rect(id .. "bg", { x = x - 2, y = y - 2, w = SIZE + 4, h = SIZE + 11, colour = { 0, 0, 0, 160 } })
    hud.icon(id, { x = x, y = y, kind = s.kind, id = s.id, size = SIZE, tip = name_of(s) })
    hud.bar(id .. "cd", { x = x, y = y + SIZE + 2, w = SIZE, h = 5, kind = "recast", value = 1 })
  end
end

local function on_slot(id, e)
  local i = tonumber(id:match("^slot(%d+)$"))
  local s = slots[i]
  if not s then return end
  if e.kind == "click" and e.button == "left" then
    if s.kind == "spell" then actions.cast(s.id) else actions.ability(s.id) end
  elseif e.kind == "drop" and e.button == "left" then
    -- A slot dropped on this one: swap them.
    local j = tonumber(e.from:match("^slot(%d+)$"))
    if j then
      slots[i], slots[j] = slots[j], slots[i]
      build()
    end
  elseif e.button == "right" and (e.kind == "drag" or e.kind == "drag_end") then
    -- The right button moves the whole bar.
    build(e.dx, e.dy)
    if e.kind == "drag_end" then
      at.x, at.y = at.x + e.dx, at.y + e.dy
    end
  end
end

local function setup()
  hud.clear()
  build()
  for i = 1, #slots do
    hud.on("slot" .. i, on_slot)
  end
end

-- The recast strips and the tooltips, five times a second.
timer.every(0.2, function()
  local r = game.recasts()
  for i, s in ipairs(slots) do
    local left, total = recast(s, r)
    local busy = left > 0 and total and total > 0
    hud.set("slot" .. i .. "cd", { value = busy and 1 - left / total or 1 })
    local text = busy and ("Ready in %d s (of %d)"):format(math.ceil(left), math.floor(total)) or "Ready"
    hud.set("slot" .. i, { tip = { title = name_of(s), text = text .. "\nClick to use; drag onto a slot to swap." } })
  end
end)

commands.register("bar", function(n, kind, id)
  n, id = tonumber(n), tonumber(id)
  if not n or not id or (kind ~= "spell" and kind ~= "ability") then
    chat.add("Usage: /bar <slot> spell|ability <id>")
    return
  end
  slots[math.min(n, #slots + 1)] = { kind = kind, id = id }
  setup()
end)

if game.player() then setup() end
events.on("login", setup)
