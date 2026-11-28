-- Client-authoritative casting, independent observed melee/ranged interval gauges.
local X = require("src.xiv")
local R = X.renderer()
local enabled, preview, preview_at = true, false, 0
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
 capture=function()return {enabled=enabled,preview=preview,at=preview_at} end,
 preview=function(on)enabled=true;preview=on;preview_at=clock() end,
 restore=function(s)enabled=s.enabled;preview=s.preview;preview_at=s.at end,
})
local settings=cfg.settings
R.drag=require('src.hud_drag').new(cfg,'offset_x','offset_y',function()return R.move_units or 1 end)
mods.provide('busy',function()return hud_config.active() end)
local swing, ranged = {}, {}
local last_live, interrupted_until, complete_until = nil, 0, 0
local weapon_key
local layout
local geometry,geometry_key,last_presentation
local function same_key(a,b)
  if not a or #a~=#b then return false end
  for i=1,#b do if a[i]~=b[i] then return false end end
  return true
end
local spells={}
local function spell_record(id)
  local value=spells[id];if not value then value=res.spell(id);if value then spells[id]=value end end
  return value
end
mods.provide('layout',function()return layout end)
local interrupts = { [16] = true, [29] = true, [84] = true, [106] = true, [49] = true,
  [47] = true, [71] = true, [78] = true, [328] = true, [313] = true, [700] = true }
local function interrupt()
  if last_live then interrupted_until = clock() + 0.84; complete_until = 0 end
end
local function observe(g, a)
  local now = clock()
  if g.last and now - g.last < 0.3 then return end
  if g.last then
    local elapsed = now - g.last
    if elapsed <= 30 then g.interval = g.interval and g.interval * 0.6 + elapsed * 0.4 or elapsed end
  end
  g.last, g.hits = now, 0
  for _, t in ipairs(a.targets or {}) do for _, r in ipairs(t.results or {}) do
    if (r.value or 0) > 0 and r.message ~= 15 and r.message ~= 63 then g.hits = g.hits + 1 end
  end end
end
local function equipment_key()
  local eq=game.equipment and game.equipment() or {}
  return tostring(eq[0] and eq[0].id or 0)..':'..tostring(eq[2] and eq[2].id or 0)
end
events.on("action", function(a)
  local me = game.player(); if not me or me.id ~= a.actor then return end
  -- With no history, idle draws need no equipment snapshot. Associate the
  -- first observed round with its weapon NOW, not after a possible swap before
  -- the next draw. Existing histories retain their per-tick weapon validation.
  if (a.category==1 or a.category==2) and not weapon_key then weapon_key=equipment_key() end
  if a.category == 1 then observe(swing, a)
  elseif a.category == 2 then observe(ranged, a)
  elseif a.category == 4 then
    local t = a.targets and a.targets[1]; local r = t and t.results and t.results[1]
    if r and (interrupts[r.message] or r.message == 28) then interrupt()
    elseif last_live then complete_until, interrupted_until = clock() + 0.15, 0 end
  end
end)
events.on("battle_message", function(actor, _, message)
  local me = game.player(); if me and actor == me.id and interrupts[message] then interrupt() end
end)
events.on("buff_gain", function(id)
  if id == 2 or id == 7 or id == 10 or id == 19 or id == 28 then interrupt() end
end)
events.on("zone_change", function()
  swing, ranged, last_live, interrupted_until, complete_until, weapon_key = {}, {}, nil, 0, 0, nil
end)
local function interval(g)
  if not g.last or not g.interval or clock() - g.last > math.max(30, g.interval * 3) then return nil end
  return X.clamp((clock() - g.last) / g.interval, 0, 1)
end
local GAUGE_TIP = { title = "Estimated attack interval",
  text = "Based on observed rounds; not an exact next-hit deadline." }
local function gauge(id, g, frac, x, y, w, h, u, sc, kind)
  R:bar(id, x, y, w, h, 20, frac, kind, GAUGE_TIP)
  if settings.get("swing_text") then
    R:text(id .. "_text", ("~%.1fs  %d hits"):format(g.interval or 0, g.hits or 0), x + w, y - 12 * u,
      9, sc, 23, nil, "right")
  end
end
local last_tick=0
local function draw()
  if not (enabled and settings.get("on")) then layout=nil;last_presentation=nil;X.native("cast", false); R:hide(); return end
  local s = hud.screen(); local sc = (settings.get("width") or 180) / 180; local u = s.ui_scale * sc
  R.move_units=s.ui_scale
  local cast_on,swing_on,ranged_on=settings.get('cast')~=false,settings.get('swing')~=false,settings.get('ranged')~=false
  local text=settings.get('swing_text') and true or false
  local key={s.width,s.height,s.ui_scale,sc,settings.get('offset_x') or 0,settings.get('offset_y') or 0,
    cast_on,swing_on,ranged_on,text,preview}
  if not same_key(geometry_key,key) then
    local w,h=180*u,10*u
    if w>s.width-8 then local fit=(s.width-8)/w;sc,u,w,h=sc*fit,u*fit,w*fit,h*fit end
    local x=X.clamp((s.width-w)/2+key[5]*s.ui_scale,0,math.max(0,s.width-w))
    local y=X.clamp(s.height*.62+key[6]*s.ui_scale,38*u,math.max(38*u,s.height-35*u))
    local top,bottom=y,y
    if cast_on then top=y-13*u;bottom=y+7*u end
    if swing_on then top=math.min(top,y-(text and 38 or 26)*u) end
    if ranged_on then top=math.min(top,y-(text and 50 or 38)*u) end
    geometry={x=x,y=y,w=w,u=u,sc=sc,top=top,bottom=bottom}
    geometry.layout=bottom>top and {version=1,preview=preview,reserve={x=x,y=top,w=w,h=bottom-top}} or nil
    geometry_key=key
  end
  local x,y,w,u,sc,top,bottom=geometry.x,geometry.y,geometry.w,geometry.u,geometry.sc,geometry.top,geometry.bottom
  local me = game.player()
  -- Stable reservation from the exact geometry used by gauge()/cast rendering.
  -- It does not claim idle gauges are visible, and avoids combat-time HUD jumps.
  layout=me and geometry.layout or nil
  if not me then swing, ranged = {}, {}
  elseif me.status ~= 1 then swing = {} end -- ranged shots need not engage melee
  -- Expiry hides a gauge; it must not erase learned interval/hit history.
  -- Validate retained histories even while expired so later attacks preserve
  -- B27's smoothing/reappearance behavior and swaps still invalidate them.
  if swing.last or ranged.last then
    local current=equipment_key()
    if weapon_key and current~=weapon_key then swing,ranged={},{} end
    weapon_key=current
  else weapon_key=nil end
  local raw = game.cast()
  if raw and raw.event then raw = nil end
  local data, red = nil, false
  if preview then
    local f = ((clock() - preview_at) % 6) / 6
    data = { name = "PREVIEW — Cure", progress = f, left = 6 * (1 - f) }
  else
    if raw and last_live and raw.spell ~= last_live.spell then interrupted_until, complete_until = 0, 0 end
    if clock() < interrupted_until then
      red = true
      if math.floor((interrupted_until - clock()) / 0.14) % 2 == 0 then
        data = { name = "Interrupted", progress = last_live and last_live.progress or 0, left = nil }
      end
    elseif raw then
      local spell = X.id(raw.spell) and spell_record(raw.spell)
      data = { spell = raw.spell, name = spell and spell.name or "Casting", progress = X.clamp(raw.progress, 0, 1),
        left = raw.left }; last_live = data
    elseif clock() < complete_until and last_live then data = { name = last_live.name, progress = 1 }
    else last_live = nil end
  end
  local sf = preview and ((clock() - preview_at) % 2) / 2 or interval(swing)
  local rf = preview and ((clock() - preview_at) % 3) / 3 or interval(ranged)
  local shown=cast_on and data or nil
  local presentation={geometry,me~=nil,shown and shown.name or false,shown and shown.progress or false,
    shown and shown.left or false,red,swing_on and sf or false,ranged_on and rf or false,
    text and swing_on and swing.interval or false,text and swing_on and swing.hits or false,
    text and ranged_on and ranged.interval or false,text and ranged_on and ranged.hits or false}
  if same_key(last_presentation,presentation) and R:ready() then X.native('cast',not preview and cast_on);return end
  R:begin()
  if data and cast_on then
    R:bar("xcb_cast", x, y, w, 7*u, 20, data.progress,
      red and { 245, 80, 80, 255 } or "cast")
    R:text("xcb_name", data.name, x, y-13*u, 11, sc, 23,
      red and { 245, 80, 80, 255 } or nil)
    if data.left then R:text("xcb_time", ("%.1f"):format(math.max(0, data.left)), x + w - 2 * u,
      y-13*u, 11, sc, 23, nil, "right") end
  end
  if sf and swing_on then gauge("xcb_swing", swing, sf, x + 90 * u,
    y - 26 * u, 90 * u, 5 * u, u, sc, "tp") end
  if rf and ranged_on then gauge("xcb_ranged", ranged, rf, x + 90 * u,
     y - 38 * u, 90 * u, 5 * u, u, sc, "tp") end
  if (data and settings.get('cast')~=false) or (sf and settings.get('swing')~=false) or (rf and settings.get('ranged')~=false) then
    R:grip('xcb_move_grip',x,top,w,bottom-top,30,function()end,'Drag to move cast/attack HUD; use Mods settings or //xcb')
  end
  R:finish()
  last_presentation=R:ready() and presentation or nil
  X.native("cast", not preview and cast_on)
end
local function tick()
  local now=clock()
  last_tick=now
  -- The cheap hidden branch must continue draining budgeted hides.
  draw()
end
events.on("settings_changed", function()
  if not enabled or not settings.get("on") then
    layout=nil;preview=false;last_presentation=nil;R:hide();X.native("cast",false)
  end
end)
timer.every(0.05, tick)
commands.register("xcb", function(arg)
  arg = tostring(arg or ""):lower()
  if arg == "preview" then preview = not preview; preview_at = clock()
  elseif arg == "on" then enabled = true
  elseif arg == "off" then enabled = false
  elseif arg == "reset" then swing, ranged = {}, {}; interrupted_until, complete_until = 0, 0
  elseif arg == "status" then chat.add("XIV Cast Bar: " .. (enabled and "on" or "off"))
  elseif arg == "" then enabled = not enabled
  else chat.add("Usage: //xcb [on|off|preview|reset|status]") end
  if not enabled then layout=nil;preview = false;last_presentation=nil; R:hide(); X.native("cast", false) end
end)
