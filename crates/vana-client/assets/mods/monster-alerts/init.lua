-- Monster actions on screen (the monster alert add-ons): a monster's TP move readied (0x028 category 7) or spell
-- started (8), its first result's value the move or spell, in a box near the top for 8 seconds; watched moves sound a call;
-- watched monsters coming within 50 yalms get a line and a call once per appearance.

local SHOW_SECS, KEEP = 8, 5
local READY, CAST = 7, 8
local shown = {}   -- {text, colour, at}, newest last
local near = {}    -- watched monster ids in range
local last_sig

local function list(key)
  local out = {}
  for w in (settings.get(key) or ""):gmatch("[^,]+") do
    w = w:match("^%s*(.-)%s*$"):lower()
    if w ~= "" then out[w] = true end
  end
  return out
end

events.on("action", function(a)
  if not settings.get("on") or (a.category ~= READY and a.category ~= CAST) or a.param == 28787 then return end
  local actor = game.entity(a.actor)
  if not actor or actor.kind ~= "mob" then return end
  local t = a.targets and a.targets[1]
  local r = t and t.results and t.results[1]
  if not r or not r.message or r.message == 0 then return end
  -- The move's name from its id (the game's line fills it in the same way).
  local what
  if a.category == READY then
    what = res.mob_skill(r.value) or (res.ability(r.value) or {}).name
  else
    what = (res.spell(r.value) or {}).name
  end
  if not what or what == "" then return end
  local target = game.entity(t.id)
  -- The game's line (an action result's values: the action's param, then the result's value), else ours.
  local m = res.message and res.message(r.message, { actor = a.actor, target = t.id, values = { a.param, r.value } })
  local line = m and text.strip(m.text) or a.category == READY and ("%s readies %s."):format(actor.name, what)
    or ("%s starts casting %s%s."):format(actor.name, what, target and target.id ~= a.actor and (" on " .. target.name) or "")
  shown[#shown + 1] = { text = line, colour = a.category == READY and { 255, 190, 110 } or { 200, 160, 255 }, at = os.clock() }
  while #shown > KEEP do table.remove(shown, 1) end
  if list("moves")[what:lower()] then audio.play_ui(17) end
end)

timer.every(1, function()
  if not settings.get("on") then return end
  local watch = list("monsters")
  if not next(watch) then return end
  local seen = {}
  for _, e in ipairs(game.entities({ distance = 50, kinds = { "mob" } })) do
    if e.name and watch[e.name:lower()] then
      seen[e.id] = true
      if not near[e.id] then
        chat.add(("Monster Alerts: %s is near (%.0f yalms)."):format(e.name, e.distance or 0))
        audio.play_ui(18)
      end
    end
  end
  near = seen
end)

events.on("frame", function()
  local now = os.clock()
  while shown[1] and now - shown[1].at > SHOW_SECS do table.remove(shown, 1) end
  local sig = #shown .. ":" .. (shown[#shown] and shown[#shown].at or 0)
  if sig == last_sig then return end
  last_sig = sig
  hud.clear()
  if not settings.get("on") or #shown == 0 then return end
  local scr = hud.screen()
  local u = scr.ui_scale
  for i, s in ipairs(shown) do
    hud.text("l" .. i, { x = scr.width / 2, y = scr.height * 0.2 + (i - 1) * 22 * u, text = s.text, size = 1.0,
      align = "center", colour = s.colour, bg = { 0, 0, 0, 150 }, pad = 4 * u, outline = { 0, 0, 0, 200 } })
  end
end)
