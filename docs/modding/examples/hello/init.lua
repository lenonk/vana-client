-- The mod built in docs/modding/getting-started.md, step by step.

-- Step 1: a line in the chat log when the mod loads.
print("Hello is loaded!")

-- Step 2 and step 5: greet each zone, and count the visits (saved per character).
storage.character.visits = storage.character.visits or {}

events.on("zone_change", function(zone)
  local name = res.zone(zone) or "somewhere new"
  local key = tostring(zone)
  local visits = (storage.character.visits[key] or 0) + 1
  storage.character.visits[key] = visits
  chat.add(("Welcome to %s (visit %d)."):format(name, visits))
end)

-- Step 3: a command, /hp.
commands.register("hp", function()
  local me = game.player()
  chat.add(("%s has %d of %d HP (%d%%)."):format(me.name, me.hp, me.hp_max, me.hpp))
end)

-- Step 4: our HP on the screen, kept up to date.
hud.text("hp", { x = 20, y = 200, size = 1.2, colour = { 255, 255, 255 }, bg = { 0, 0, 0, 140 } })

local function show_hp(hpp)
  hud.set("hp", { text = ("HP %d%%"):format(hpp) })
end

-- Mods load once your character is in, so this is the current value.
local hpp = game.player().hpp
if hpp then
  show_hp(hpp)
end

-- Step 6: a warning below the threshold the player sets on the mod's Extras page.
events.on("hpp_change", function(hpp, previous)
  show_hp(hpp)
  local limit = settings.get("warn_below")
  if hpp < limit and previous >= limit then
    chat.add(("Careful: your HP is down to %d%%!"):format(hpp))
  end
end)

-- Step 7: a window listing who is nearby; picking a row targets them.
local nearby = {}

ui.window("nearby", {
  title = "Nearby",
  width = 320,
  elements = {
    { type = "list", id = "who", height = 8, rows = {},
      columns = { { title = "Name" }, { title = "Kind", width = 60 }, { title = "Yalms", width = 60, align = "right" } } },
    { type = "buttons", id = "buttons", items = { "Refresh", "Close" } },
  },
})

local function refresh()
  nearby = {}
  for _, e in ipairs(game.entities()) do
    if not e.me and e.distance < 20 then
      table.insert(nearby, e)
    end
  end
  table.sort(nearby, function(a, b) return a.distance < b.distance end)
  local rows = {}
  for _, e in ipairs(nearby) do
    table.insert(rows, { e.name, e.kind, ("%.1f"):format(e.distance) })
  end
  ui.update("nearby", "who", { rows = rows })
end

ui.on("nearby", function(_, e)
  if e.kind == "select" and e.id == "who" then
    local picked = nearby[e.index]
    if picked then
      actions.target(picked.id)
    end
  elseif e.kind == "press" and e.id == "buttons" then
    if e.index == 1 then
      refresh()
    else
      ui.close("nearby")
    end
  end
end)

commands.register("nearby", function()
  refresh()
  ui.open("nearby")
end)
