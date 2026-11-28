-- Coordinated XIV target suite. Artwork, data tracking and input are separate.
local X = require("src.xiv")
local C = require("src.combat")
local palette = require("src.target_palette")
local R = X.renderer()
local enabled, preview, layout, dragging = true, false, nil, nil
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
  capture=function()return {enabled=enabled,preview=preview} end,
  preview=function(on)enabled=true;preview=on end,
  restore=function(s)enabled=s.enabled;preview=s.preview end,
  base=function(key)local p=storage.account.place or {};if key=='place_x' then return p.x or 0 elseif key=='place_y' then return p.y or 0 end end,
  busy=function()return ui.is_open('xtb_details') end,
})
local settings=cfg.settings
local last_hp, trail, last_id = nil, nil, nil
local detail_rows, detail_at = {}, 1
local WIN = "xtb_details"
-- The status slot takes the weakness icons' width and runs 4:3 in height, so the two rows on one
-- line are the same width with the squares shorter. icon_units is the weakness side in UI units
-- (18 at the 100% size, 23.9 at the default 133%), so the slot is 18 x 24 there and 24 x 32 at 133%.
local function icon_px(u)
  local units = 24 -- weakness slot width in UI units at default scale (4:3 art)
  return math.max(1, math.floor(units * u))
end
local function status_columns(w,u,pitch)pitch=pitch or 24*u;return math.max(1,math.floor((w-math.min(56*u,math.max(0,w-pitch)))/pitch)) end
-- The first status row shares its line with the distance text on the right; row_reserve()
-- keeps the first row's status columns left of that text. Later rows use the full bar width.
local function row_reserve(u)
  local dist = 7 * 6.3 * u + 4 * u
  return dist + 6 * u
end
local function first_row_cols(w,pitch,reserve,columns)
  return math.max(0, math.min(columns, math.floor((w - reserve) / pitch)))
end
local function status_slot(i,c1,columns)
  if i <= c1 then return i - 1, 0 end
  local j = i - c1 - 1
  return j % columns, 1 + math.floor(j / columns)
end
local function status_rows(n,c1,columns)
  if n <= c1 then return 1 end
  return 1 + math.ceil((n - c1) / columns)
end
local function scale() return (settings.get("scale") or 100) / 100 end
local function origin(t,other)
  local s, p = hud.screen(), {x=settings.get('place_x'),y=settings.get('place_y')}
  local u = s.ui_scale * scale()
  local w = math.min(settings.get("width") * u, math.max(80, s.width - 16))
  local x=X.clamp((s.width-w)/2+(p.x or 0),0,s.width-w)
  local count=math.floor(X.clamp(settings.get('status_count') or 12,1,32))
  local columns=status_columns(w,u)
  local reserve=math.max(110,55+math.ceil(count/columns)*25)*u
  if other and x+w+12*u>=s.width-170*u then reserve=reserve+52*u end
  return s,u,w,x,X.clamp(s.height*0.12+(p.y or 0),20*u,math.max(20*u,s.height-reserve))
end
local effects_cache,effects_at,effects_id
local function effects(id)
  if effects_id~=id or not effects_at or clock()-effects_at>=0.1 then
    effects_cache=X.statuses(C.effects(id), storage.account.status_hidden, storage.account.status_order)
    effects_at,effects_id=clock(),id
  end
  return effects_cache
end
local cast_cache,cast_at,cast_id
local function cast(id)
  if cast_id~=id or not cast_at or clock()-cast_at>=0.015 then
    cast_cache=C.cast(id);cast_at,cast_id=clock(),id
  end
  return cast_cache
end
-- Presentation caches only; combat's independent observers/timers/services keep
-- running with the HUD hidden. Invalidate before the next bounded native drain.
local generation,presented_id,presented_preview,layout_revision=0,nil,nil,0
for _,event in ipairs({'action','battle_message','party_change','party_buffs','buff_change','settings_changed'}) do
  events.on(event,function() effects_at,cast_at=nil,nil end)
end
for _,event in ipairs({'zone_change','login','logout'}) do
  events.on(event,function()
    generation=generation+1;presented_id=nil;effects_at,cast_at=nil,nil
    last_id,last_hp,trail=nil,nil,nil;layout=nil;dragging=nil;R.incomplete=true;X.native('target',enabled and settings.get("on"))
  end)
end
mods.provide("layout", function() return layout end)
mods.provide("cast", C.cast)
mods.provide("casts_for", C.casts_for)
mods.provide("effects", C.effects)
mods.provide("busy", function() return ui.is_open(WIN) end)
local function refresh_detail()
  if not ui.is_open(WIN) then return end
  local t = game.target()
  detail_rows = t and X.statuses(C.effects(t.id)) or {}
  local rows = {}
  for _, e in ipairs(detail_rows) do
    rows[#rows + 1] = { e.name, e.left and X.fmt_time(e.left) or (e.untimed and "—" or "?"),
      icon = { kind = "status", id = e.id } }
  end
  detail_at = math.min(detail_at, math.max(1, #rows))
  ui.update(WIN, "rows", { rows = rows, at = detail_at })
  local selected=detail_rows[detail_at];local info=selected and X.status_info(selected.id)
  ui.update(WIN,'description',{text=selected and (selected.name..'\n'..(info and info.description or '')) or ''})
  ui.update(WIN, "who", { text = t and t.name or "No target" })
end
local function details()
  ui.window(WIN, { title = "XIV Target Statuses", width = 410, elements = {
    { type = "text", id = "who", text = "" },
    { type = "list", id = "rows", rows = {}, height = 8,
       columns = { { title = "Status", width = 0 }, { title = "Time", width = 60 } } },
    { type = "text", id = "description", text = "" },
    { type = "text", id = "hint", text = "? means duration unknown. Tracked enemy times are estimates." },
    { type = "buttons", id = "buttons", items = { "Prioritise", "Hide", "Clear filters", "Close" } },
  } }); ui.open(WIN); refresh_detail()
end
ui.on(WIN, function(_, e)
  if e.kind == "select" then detail_at = e.index
  elseif e.kind == "press" and e.id == "buttons" then
    local row = detail_rows[detail_at]
    if e.index == 1 and row then
      local order = { row.id }; for _, id in ipairs(storage.account.status_order or {}) do
        if id ~= row.id then order[#order + 1] = id end
      end; storage.account.status_order = order; storage.save()
    elseif e.index == 2 and row then
      storage.account.status_hidden = storage.account.status_hidden or {}
      storage.account.status_hidden[tostring(row.id)] = true; storage.save()
    elseif e.index == 3 then storage.account.status_hidden, storage.account.status_order = {}, {}; storage.save()
    elseif e.index == 4 then ui.close(WIN) end
  end
end)
local preview_effects={ { id=3,name='Poison',left=25,source='PREVIEW' } }
local membership_list,membership_layout,membership_revision
local function status_membership(list,revision)
  if membership_list~=list or membership_layout~=revision then
    local key={revision,#list}
    for _,row in ipairs(list) do
      key[#key+1]={row.id,row.name,row.status_name or false,row.source,row.untimed or false}
    end
    membership_list,membership_layout,membership_revision=list,revision,key
  end
  return membership_revision
end
local function draw(dt)
  R:enter_frame('target')
  local on = enabled and settings.get("on")
  local t = on and game.target()
  if preview and on then t = { id = -1, name = "PREVIEW — Enemy", kind = "mob", hpp = 73, distance = 12, gauge = true } end
  local current_id=t and not t.hidden and t.id or nil
  if presented_id~=current_id or presented_preview~=preview then
    generation=generation+1;presented_id,presented_preview=current_id,preview;dragging=nil
  end
  R:begin(generation)
  if not t or t.hidden then
    -- Keep the native target window suppressed while this mod owns the display, even with
    -- nothing selected. Un-hiding it here showed it for a frame when the next target appeared.
    layout, last_id, last_hp, trail = nil, nil, nil, nil; X.native("target", on); R:finish(); R:leave_frame(); return
  end
  -- One snapshot serves both origin's bottom reservation and the ToT presenter.
  local other=not preview and t.target_id and game.entity(t.target_id) or nil
  -- Enemies never report a facetarget, so fall back to our own last-hit tracker (independent of Aggro).
  if not other and not preview and t.kind=='mob' then
    local lh=C.last_hit and C.last_hit[t.id]
    if lh and clock()-lh.at<8 then other=game.entity(lh.id) end
  end
  local s, u, w, x, y = origin(t,other)
  local z=scale()
  local count = math.floor(X.clamp(settings.get("status_count") or 12,1,32))
  local icon = icon_px(u)
  local iw = X.status_w(icon)            -- status art is 3:4, so slots are narrower than tall
  -- The row's own gap follows the icon: a ninth of its width, so it is 2 px when the icon is 18 wide
  -- (weakness 100%) and 3 px at 27 (150%) — the same ratio as the weakness row's flat 2 px at 100%.
  local px, py = iw + iw / 9, icon + 5 * u
  local columns = status_columns(w,u,px)
  local reserve = row_reserve(u)
  local c1 = first_row_cols(w,px,reserve,columns)
  local rows = status_rows(count,c1,columns)
  local tot_dx = 12 * u
  local beside=x+w+tot_dx<s.width-170*u
  local distance_text=('%.1f y'):format(t.distance or 0)
  local bottom=y+13*u+rows*py+26*u
  if other and not other.hidden and not beside then bottom=bottom+52*u end
  if not layout or layout.x~=x or layout.y~=y or layout.w~=w or layout.ui_scale~=u or
      layout.scale~=z or layout.bottom~=bottom or layout.status_right~=x+columns*px or layout.row_reserve~=reserve or layout.icon~=icon or layout.preview~=preview then
    layout_revision=layout_revision+1
    layout = { version = 1, visible = true, x = x, y = y, w = w, h = 12 * u,
      bottom = bottom, ui_scale = u, scale = z, preview = preview, status_right=x+columns*px, row_reserve = reserve, icon = icon, dist_text = distance_text, entity = current_id }
  elseif layout.dist_text~=distance_text or layout.entity~=current_id then
    -- The distance text and which entity this box is for change constantly (the text while moving,
    -- the entity on every swap). Publishing them must not bump the layout revision: that
    -- invalidated identity/vitals/status/distance together and the status row (and weakness
    -- icons) repainted a frame or more late. Readers use them to stand their own
    -- elements on this box, so they must read the current ones, not the ones a revision old.
    layout.dist_text=distance_text
    layout.entity=current_id
  end
  local p, hp = palette(t), X.clamp(t.hpp, 0, 100) / 100
  if last_id ~= t.id then trail, last_hp, last_id = hp, hp, t.id end
  if hp < last_hp then trail = math.max(trail, last_hp) elseif hp > last_hp then trail = hp end
  trail = math.max(hp, trail - math.max(0.005, tonumber(dt) or 0.016) * 0.35); last_hp = hp
  R:group('identity',{layout_revision,t.name or 'Target',p,t.gauge~=false},function()
    R:text('tb_name',X.short(t.name or 'Target',math.max(8,math.floor(w/u/9)-8)),x,y-20*u,14,z,15,p.text)
    local identity=t.id
    R:grip('tb_grip',x,y-22*u,w,35*u,17,function(_,e)
      local current=game.target()
      if preview or not current or current.id~=identity or current.hidden then return end
      if hud_config.active() then return end
      if e.kind=='drag_start' then local place=storage.account.place or {};dragging={place.x or 0,place.y or 0}
      elseif (e.kind=='drag' or e.kind=='drag_end') and dragging then
        storage.account.place={x=dragging[1]+e.dx,y=dragging[2]+e.dy}
        local bucket=storage.account.xiv_ui_target_v1
        if type(bucket)=='table' and type(bucket.values)=='table' then bucket.values.place_x=nil;bucket.values.place_y=nil end
        if e.kind=='drag_end' then dragging=nil;storage.save() end
      elseif e.kind=='click' then details() end
    end,'Click for status details; drag to move')
  end)
  R:group('vitals',{layout_revision,p,t.gauge~=false,hp,trail},function()
    if t.gauge~=false then
      if trail>hp then R:bar('tb_trail',x,y,w,12*u,11,trail,{180,110,100,170}) end
      R:bar('tb_hp',x,y,w,12*u,12,hp,p.fill)
      R:text('tb_hpp',math.floor(hp*100)..'%',x+w,y-20*u,14,z,15,p.text,'right')
    end
  end)
  local distance=distance_text
  R:group('distance',{layout_revision,distance},function()
    R:text('tb_dist',distance,x+w,y+13*u,10,z,15,nil,'right')
  end)
  local list = preview and preview_effects or effects(t.id)
  R:group('status_membership',{status_membership(list,layout_revision),count},function()
    for i=1,math.min(count,#list) do
      local col,row=status_slot(i,c1,columns)
      R:status('tb_status'..i,list[i],x+col*px,y+13*u+row*py,icon,z,25,true,false)
    end
    if #list>count then
      local col,row=status_slot(math.min(count,#list)+1,c1,columns)
      R:text('tb_more','+'..(#list-count),x+col*px+2*u,y+13*u+row*py+math.max(0,(icon-12*u)/2),10,z,27)
    end
  end)
  local times={layout_revision,count}
  for i=1,math.min(count,#list) do times[i+2]=not list[i].untimed and X.fmt_time(list[i].left) or false end
  R:group('status_countdowns',times,function()
    for i=1,math.min(count,#list) do
      if times[i+2] then
        local col,row=status_slot(i,c1,columns)
        R:text('tb_status'..i..'_time',times[i+2],x+col*px+iw/2,y+13*u+row*py+icon-5*z,8,z,26,nil,'center',true)
      end
    end
  end)
  local c = not preview and cast(t.id)
  -- A quarter of the hp bar's width, right-aligned to its right edge, below the weakness
  -- icons / status block; the spell name sits to its left so it never covers the icons.
  local cw = w * 0.25
  local cx = x + w - cw
  local cy = y + 13 * u + rows * py + 4 * u
  if c then
    R:group('cast_identity',{layout_revision,c.name,c.source},function()
      R:text('tb_castname',X.short(c.name,math.max(8,math.floor(w/u/8)-4))..
        (c.source:find('Estimated',1,true) and ' ~' or ''),cx-4*u,cy-4*u,10,z,23)
    end)
    R:group('cast_progress',{layout_revision,c.progress},function()
      R:bar('tb_cast',cx,cy,cw,3*u,18,c.progress,'cast',{title=c.name,text=c.source})
    end)
  end
  if other and not other.hidden then
    local tw=math.min(160*u,s.width)
    local tx=beside and x+w+tot_dx or X.clamp(x,0,s.width-tw)
    local ty=beside and y or bottom-52*u+28*u
    local op,frac=palette(other),X.clamp(other.hpp,0,100)/100
    R:group('tot_identity',{layout_revision,tx,ty,tw,other.id,other.name,op},function()
      R:text('tb_tot_name',other.name,tx,ty-14*u,11,z,23,op.text)
      local identity,other_id=t.id,(t.target_id or other.id)
      R:grip('tb_tot_grip',tx,ty-16*u,tw,26*u,24,function(_,e)
        local main=game.target()
        if not preview and main and not main.hidden and main.id==identity and
            (main.target_id==other_id or (not main.target_id and C.last_hit[identity] and C.last_hit[identity].id==other_id)) and
            e.kind=='click' and (not e.button or e.button=='left') then X.target(other_id) end
      end)
    end)
    R:group('tot_vitals',{layout_revision,tx,ty,tw,op,frac},function()
      R:bar('tb_tot',tx,ty,tw,8*u,21,frac,op.fill)
    end)
  end
  R:finish()
  -- Do not suppress the native target until the CURRENT presentation, including
  -- pending handlers/hides, has completed. A clean producer is not readiness.
  X.native('target',true)
  R:leave_frame()
end
events.on("frame", draw)
timer.every(0.25, refresh_detail)
commands.register("xtb", function(arg)
  arg = tostring(arg or ""):lower()
  if arg == "details" then details()
  elseif arg == "preview" then preview = not preview
  elseif arg == "off" then enabled = false
  elseif arg == "on" then enabled = true
  elseif arg == "status" then chat.add("XIV Target Bar: " .. (enabled and "on" or "off"))
  elseif arg == "" then enabled = not enabled
  else chat.add("Usage: //xtb [on|off|details|preview|status]") end
  if not enabled then preview = false; R:hide(); layout = nil; ui.close(WIN); X.native("target", false)
  else X.native("target", settings.get("on"), true) end
end)

-- Suppress the native target window from the first frame so a target that is already selected
-- when the mod loads never shows through for a frame.
X.native('target', enabled and settings.get("on"), true)
