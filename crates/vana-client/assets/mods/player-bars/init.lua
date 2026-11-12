-- Our HP, MP and TP as large bars near the middle of the screen (the player bar add-ons), in the HUD's gauges.

local KINDS = { { "hp", "HP" }, { "mp", "MP" }, { "tp", "TP" } }
local last

local function values()
  local p = game.player()
  if not p or not p.hp_max or not p.hp then return nil end
  return {
    hp = { p.hp, p.hp_max }, mp = { p.mp, p.mp_max }, tp = { p.tp, 3000 },
  }
end

local function draw()
  local v = settings.get("on") and values()
  if not v then
    if last then hud.clear() end
    last = nil
    return
  end
  local scr = hud.screen()
  local sig = table.concat({ v.hp[1], v.hp[2], v.mp[1], v.mp[2], v.tp[1], scr.width, scr.height, scr.ui_scale,
    settings.get("width"), settings.get("offset_y") }, ",")
  if sig == last then return end
  last = sig
  hud.clear()
  local u = scr.ui_scale
  local w, h, gap = settings.get("width") * u, 7 * u, 14 * u
  local shown = {}
  for _, k in ipairs(KINDS) do
    if k[1] ~= "mp" or v.mp[2] > 0 then shown[#shown + 1] = k end
  end
  local total = #shown * w + (#shown - 1) * gap
  local x0 = math.floor((scr.width - total) / 2)
  local y = math.floor(scr.height * 0.56 + settings.get("offset_y") * u)
  for i, k in ipairs(shown) do
    local cur, max = v[k[1]][1], v[k[1]][2]
    local x = x0 + (i - 1) * (w + gap)
    hud.bar(k[1], { x = x, y = y, w = w, h = h, value = max > 0 and cur / max or 0, kind = k[1] })
    hud.text(k[1] .. "_l", { x = x, y = y - 18 * u, text = k[2], size = 0.85, colour = { 230, 225, 210 },
      outline = { 0, 0, 0, 220 } })
    hud.text(k[1] .. "_v", { x = x + w, y = y - 18 * u, text = tostring(cur), size = 0.85, align = "right",
      colour = { 255, 255, 255 }, outline = { 0, 0, 0, 220 } })
  end
end

events.on("frame", draw)
