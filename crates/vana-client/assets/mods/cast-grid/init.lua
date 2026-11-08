-- Click-to-cast over the party list (the click-to-cast add-ons): a row per party member, a button per chosen spell.
-- In the modern UI the grid stands on the party panel; in the classic UI at the right edge above the party list.

local SIZE, GAP = 30, 3
local last_sig
-- The chosen spells and the known ones, kept between frames (every spell's resource was read every frame): again on
-- a settings, job, level or zone change, and once a second for a spell just learned.
local list, have

local function spells()
  local out = {}
  for name in (settings.get("spells") or ""):gmatch("[^,]+") do
    name = name:match("^%s*(.-)%s*$")
    if name ~= "" then out[#out + 1] = name end
  end
  return out
end

-- Known spells by lower-case name -> id.
local function known()
  local out = {}
  for _, id in ipairs(game.spells()) do
    local s = res.spell(id)
    if s and s.name then out[s.name:lower()] = id end
  end
  return out
end

local function refresh()
  list, have = nil, nil
end

local function short(name)
  local base, tier = name:match("^(.-)%s+([IV]+)$")
  base = base or name
  return base:sub(1, 2) .. (tier or "")
end

local function draw()
  if not settings.get("on") then
    if last_sig then hud.clear() end
    last_sig = nil
    return
  end
  local members = {}
  for _, m in ipairs(game.party()) do
    if m.party == 0 and m.name and m.name ~= "" then members[#members + 1] = m end
  end
  list = list or spells()
  have = have or known()
  local recasts = game.recasts().spells
  local scr = hud.screen()
  local u = scr.ui_scale
  local panel = ui.native.rect("party")
  local chat = ui.native.rect("chat")
  chat = chat and (chat.room or chat)
  local sig = { scr.width, scr.height, u, panel and panel.x or 0, panel and panel.y or 0, panel and panel.h or 0,
    chat and chat.y or 0 }
  for _, m in ipairs(members) do sig[#sig + 1] = m.id end
  for _, s in ipairs(list) do
    local id = have[s:lower()]
    sig[#sig + 1] = s .. ":" .. (id or 0) .. ":" .. ((id and (recasts[id] or 0) > 0) and 1 or 0)
  end
  sig = table.concat(sig, "|")
  if sig == last_sig then return end
  last_sig = sig
  hud.clear()
  if #members == 0 or #list == 0 then return end
  local size, gap = SIZE * u, GAP * u
  local name_w = 80 * u
  local w = name_w + #list * (size + gap)
  local rows_h = #members * (size + gap)
  -- Over the party panel, right-aligned with it (HUD elements draw under the panels, so never on one).
  local x0, y0
  if panel and panel.shown then
    -- Above both the party panel and the chat window's full height (it may run under the party panel).
    local top = panel.y
    local chat = ui.native.rect("chat")
    chat = chat and (chat.room or chat)
    if chat and chat.x + chat.w > panel.x then top = math.min(top, chat.y) end
    x0, y0 = panel.x + panel.w - w, top - rows_h - 8 * u
  else
    x0, y0 = scr.width - w - 16 * u, scr.height - rows_h - 150 * u
  end
  for r, m in ipairs(members) do
    local y = y0 + (r - 1) * (size + gap)
    hud.text(("n%d"):format(r), { x = x0 + name_w - 6 * u, y = y + size * 0.25, text = m.name, size = 0.8,
      align = "right", colour = { 235, 230, 215 }, outline = { 0, 0, 0, 220 } })
    for c, s in ipairs(list) do
      local id = have[s:lower()]
      local x = x0 + name_w + (c - 1) * (size + gap)
      local key = ("b%d_%d"):format(r, c)
      local ready = id and (recasts[id] or 0) <= 0
      hud.rect(key, { x = x, y = y, w = size, h = size, round = 4 * u,
        colour = ready and { 30, 38, 58, 220 } or { 20, 20, 24, 150 },
        tip = { title = s, text = "Cast on " .. m.name } })
      hud.text(key .. "_t", { x = x + size / 2, y = y + size * 0.22, text = short(s), size = 0.72, align = "center",
        colour = ready and { 235, 235, 240 } or { 120, 120, 125 }, outline = { 0, 0, 0, 200 } })
      if id then
        local me = game.player()
        local target, spell = (me and m.id == me.id) and "<me>" or m.name, s
        hud.on(key, function(_, e)
          if e.kind == "click" and e.button == "left" then
            commands.run(('/ma "%s" %s'):format(spell, target))
          end
        end)
      end
    end
  end
end

events.on("frame", draw)
for _, e in ipairs({ "settings_changed", "job_change", "level_up", "level_down", "zone_change" }) do
  events.on(e, refresh)
end
timer.every(1, function() have = nil end)
