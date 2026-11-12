-- Targeting helpers (the targeting add-ons): //face [away], //tgt <name | id>, and /check on each new monster target.

local checked = {}

-- The heading from us toward a point: x east, z with north negative (the client's face math).
local function heading_to(me, p)
  local h = math.atan(-(p.z - me.pos.z), p.x - me.pos.x)
  return h % (2 * math.pi)
end

commands.register("face", function(arg)
  if not settings.get("on") then return end
  local me, t = game.player(), game.target()
  if not me or not t then
    chat.add("Targeting Helpers: no target.")
    return
  end
  local h = heading_to(me, t.pos)
  if (arg or ""):lower() == "away" then h = (h + math.pi) % (2 * math.pi) end
  actions.turn(h)
end)

commands.register("tgt", function(...)
  if not settings.get("on") then return end
  local want = table.concat({ ... }, " "):match("^%s*(.-)%s*$")
  if want == "" then
    chat.add("Targeting Helpers: //tgt <name or server id>")
    return
  end
  local id = tonumber(want)
  if id then
    actions.target(id)
    return
  end
  local best, dist
  local low = want:lower()
  for _, e in ipairs(game.entities({ distance = 50 })) do
    if e.name and e.name:lower():sub(1, #low) == low and not e.me and (not dist or e.distance < dist) then
      best, dist = e, e.distance
    end
  end
  if best then actions.target(best.id) else chat.add("Targeting Helpers: no " .. want .. " nearby.") end
end)

events.on("target_change", function(id)
  if not settings.get("on") or not settings.get("check") or not id or id == 0 or checked[id] then return end
  local e = game.entity(id)
  if e and e.kind == "mob" then
    checked[id] = true
    commands.run("/check <t>")
  end
end)

events.on("zone_change", function() checked = {} end)
