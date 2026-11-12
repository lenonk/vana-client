-- The minimap (the minimap add-ons): the zone's map around us in the minimap's edge window (panel "minimap": its
-- place, fold and size; in its corner in the classic UI, Wide Scan under it in the modern UI). The client draws the map
-- and our marker; this mod picks the dots (what the compass radar shows, monsters included, in its colours, the target
-- ringed and each named in its tooltip), the zoom (the wheel over it: the client option minimap_span) and the heading.

-- The wheel's range: map pixels across (the whole map is 512); a notch is a quarter more or less.
local SPAN_MIN, SPAN_MAX = 48, 512

local shown = false
local heading = ""
local heading_at = -math.huge
-- An older client knows no map element: it draws its minimap in the bare window, as before.
local legacy = false

-- Only what stands within reach of the map shown: its corners (half the span's map pixels, as yalms, and the diagonal)
-- and a margin for height, which the distance filter counts. Nothing where no map is drawn.
local function dots()
  local m = game.map()
  if not m or not m.px_per_unit or m.px_per_unit <= 0 then return {} end
  local span = tonumber(client.option("minimap_span")) or 160
  local reach = span / 2 / m.px_per_unit * 1.5 + 10
  local me = game.player()
  local out = {}
  for _, e in ipairs(game.entities({ distance = reach })) do
    if e.radar and e.id ~= me.id then
      out[#out + 1] = { x = e.pos.x, z = e.pos.z, colour = e.radar, target = e.id == me.target, tip = e.name }
    end
  end
  return out
end

-- The zone and the grid square, as the location panel words them.
local function words()
  local z = game.zone()
  local name = z and res.zone(z) or ""
  local grid = game.info().map_grid or ""
  if grid == "" then return name end
  return name .. "  " .. grid
end

local function window()
  if legacy then return end
  local ok, err = pcall(ui.window, "minimap", { edge = true, panel = "minimap", title = "Map", heading = heading,
    elements = { { type = "map", id = "map", dots = dots() } } })
  if not ok then
    -- Only an older client's refusal of the map element means legacy; any other error is a bug to show.
    if not tostring(err):find("unknown element type", 1, true) then error(err, 0) end
    legacy = true
    ui.window("minimap", { edge = true, panel = "minimap", title = "Map" })
  end
end

local function show()
  if settings.get("on") then
    heading = words()
    window()
    ui.open("minimap")
    shown = true
  else
    ui.close("minimap")
    shown = false
  end
end

ui.on("minimap", function(_, ev)
  if ev.kind == "wheel" then
    local span = tonumber(client.option("minimap_span")) or 160
    span = math.max(SPAN_MIN, math.min(SPAN_MAX, span * 1.25 ^ -ev.notches))
    client.set_option("minimap_span", span)
  end
end)

-- The dots follow every frame; the heading, read twice a second, when the place or the grid changes.
events.on("frame", function()
  if not shown or legacy then return end
  if os.clock() - heading_at >= 0.5 then
    heading_at = os.clock()
    local h = words()
    if h ~= heading then
      heading = h
      window()
      return
    end
  end
  ui.update("minimap", "map", { dots = dots() })
end)

show()
events.on("settings_changed", show)
