-- NPC talk in a speech bubble (the balloon add-ons): the speaker's face, name and the page being read in a rounded
-- bubble whose tail points at the speaker's head, over them or at the bottom of the screen. The log keeps its copy;
-- keys are the game's.

local BODY = { 240, 236, 224, 255 }
local NAME = { 232, 200, 120, 255 }
local BG = { 14, 16, 26, 235 }
local FACE_BG = { 30, 34, 50, 255 }
local FRAME = { 196, 160, 96, 255 }
local CLEAR = { 0, 0, 0, 0 }
-- over other mods' HUD (the hotbars use 0..4)
local LAYER = 20

local shown = false
-- The page whose texts were measured last frame (drawn unseen), so this frame can fit the bubble round them.
local measured

local function hide()
  if shown or measured then
    hud.clear()
    shown, measured = false, nil
  end
end

-- "Name : text" opens NPC talk; narration has no name. Later lines lose the log's continuation indent.
local function split(lines)
  local name, first = lines[1]:match("^(.-) : (.*)$")
  local body = { first or lines[1] }
  for i = 2, #lines do body[#body + 1] = (lines[i]:gsub("^%s+", "")) end
  return name, table.concat(body, "\n")
end

-- A world point on screen, when it is in view.
local function screen(e, up)
  local p = draw.to_screen({ x = e.pos.x, y = e.pos.y - up, z = e.pos.z })
  if p and p.on_screen then return p end
end

local function frame()
  local ev = settings.get("on") and game.event()
  local t = ev and not ev.menu and ev.text
  if not (t and t.lines and #t.lines > 0) then return hide() end
  -- Live NPCs only: an object talking (a casket, a ??? point, a door) keeps the game's window alone.
  local speaker = t.actor and game.entity(t.actor)
  if speaker and not speaker.me and (speaker.bones or 0) < 8 then return hide() end
  local name, body = split(t.lines)
  local scr = hud.screen()
  local u = scr.ui_scale
  local size = settings.get("size")
  -- This page's texts unseen, for their sizes next frame; until then the last page stays up as it is.
  local key = (name or "") .. "\n" .. body
  hud.text("m_body", { x = 0, y = 0, text = body, size = size, colour = CLEAR, layer = -100 })
  if name then
    hud.text("m_name", { x = 0, y = 0, text = name, size = size * 0.9, font = "bold", colour = CLEAR, layer = -100 })
  else
    hud.remove("m_name")
  end
  local ready = measured == key
  measured = key
  local bb, nb = hud.bounds("m_body"), name and hud.bounds("m_name")
  if not ready or not bb or (name and not nb) then return end

  local pad = 12 * u
  local text_w = math.max(bb.w, nb and nb.w or 0)
  local name_h = nb and nb.h + 3 * u or 0
  local text_h = name_h + bb.h + 10 * u
  local e = t.actor and game.entity(t.actor)
  local fh = settings.get("portrait") and e and math.max(80 * u, math.min(112 * u, text_h))
  local fw = fh and math.floor(fh * 0.8)
  local text_x = pad + (fw and fw + 12 * u or 0)
  local bw = text_x + text_w + pad
  local bh = 2 * pad + math.max(fh or 0, text_h)

  -- Over the speaker, clear of their name plate, when their head is in view; else above the log. The tail points at
  -- the head wherever the bubble is.
  local height = e and e.height or 1.8
  local head = e and e.pos and screen(e, height * 0.85)
  local over = settings.get("place") == "Over the Speaker" and e and e.pos and screen(e, height + 0.9)
  local cx, bottom
  if over then
    cx, bottom = over.x, over.y - 30 * u
  else
    cx, bottom = scr.width / 2, scr.height * 0.66
  end
  local bx = math.max(8 * u, math.min(scr.width - 8 * u - bw, cx - bw / 2))
  local by = math.max(8 * u, math.min(scr.height - 8 * u - bh, bottom - bh))

  hud.bubble("bubble", { x = bx, y = by, w = bw, h = bh, round = 14 * u, colour = BG, edge = FRAME, edge_w = 2 * u,
    tail = head and { x = head.x, y = head.y } or false, tail_w = 24 * u, tail_len = 26 * u, layer = LAYER })
  if fh then
    hud.portrait("face", { entity = t.actor, x = bx + pad, y = by + (bh - fh) / 2, w = fw, h = fh, bg = FACE_BG,
      frame = FRAME, frame_w = 2 * u, layer = LAYER + 1 })
  else
    hud.remove("face")
  end
  local ty = by + (bh - text_h) / 2
  if name then
    hud.text("name", { x = bx + text_x, y = ty, text = name, size = size * 0.9, font = "bold", colour = NAME,
      layer = LAYER + 1 })
  else
    hud.remove("name")
  end
  hud.text("body", { x = bx + text_x, y = ty + name_h, text = body, size = size, colour = BODY, layer = LAYER + 1 })
  -- The arrow, a small triangle, blinking while the page waits for a key (none when it closes by itself).
  if t.waiting then
    local a = math.floor(64 + 191 * math.abs(math.sin(os.clock() * 4)))
    local aw, ax, ay = 12 * u, bx + bw - pad - 6 * u, by + bh - pad - 8 * u
    hud.bubble("more", { x = ax - aw / 2, y = ay, w = aw, h = 1, round = 0, colour = { NAME[1], NAME[2], NAME[3], a },
      tail = { x = ax, y = ay + 100 }, tail_w = aw, tail_len = 7 * u, layer = LAYER + 1 })
  else
    hud.remove("more")
  end
  shown = true
end

events.on("frame", frame)
events.on("settings_changed", hide)
