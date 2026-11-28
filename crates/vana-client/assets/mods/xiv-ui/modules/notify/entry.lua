-- B09 first slice: bounded rising inventory notifications, never auto-actions.
local X=require('src.xiv')
local inventory=require('src.inventory')
local requests=require('src.requests')
local tracker=inventory.new()
local R=X.renderer()
local enabled,preview=true,false
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
 capture=function()return {enabled=enabled,preview=preview} end,
 preview=function(on)enabled=true;preview=on;requests.preview(on) end,
 restore=function(s)enabled=s.enabled;preview=s.preview;requests.preview(s.preview) end,
 busy=requests.busy,
})
local settings=cfg.settings
requests.bind_settings(settings)
local queue,active={},{}
local toasts_shown=false
local preview_toast={text='PREVIEW · Inventory + Potion × 3',source='Synthetic sample'}
local function hide_toasts()
  if toasts_shown or not R:ready() then R:hide() end
  toasts_shown=false
end
local warm_until,last_change=clock()+8,clock()
local party_names
local seed
local inventory_revision=0
local function reset()
  tracker=inventory.new();warm_until,last_change=clock()+8,clock();queue,active={},{};party_names=nil;seed=nil;inventory_revision=inventory_revision+1
end
events.on('login',reset);events.on('logout',reset);events.on('zone_change',reset)
local function enqueue(text,source)
  if not enabled or not settings.get('on') or preview then return end
  queue[#queue+1]={text=text,source=source}
  while #queue>30 do table.remove(queue,1) end
end
events.on('item_add',function(bag,slot,id,count)
  inventory_revision=inventory_revision+1
  last_change=clock();tracker:set(bag,slot,id,count,clock())
  if not enabled or not settings.get('on') or preview then tracker.pending={} end
end)
events.on('item_change',function(bag,slot,id,count,previous)
  inventory_revision=inventory_revision+1
  last_change=clock();tracker:set(bag,slot,id,count,clock(),previous)
  if not enabled or not settings.get('on') or preview then tracker.pending={} end
end)
events.on('item_remove',function(bag,slot,id,count)
  inventory_revision=inventory_revision+1
  last_change=clock();tracker:remove(bag,slot,id,count,clock())
  if not enabled or not settings.get('on') or preview then tracker.pending={} end
end)
local function item_name(id)
  local record=X.id(id) and res.item(id)
  return record and record.name or ('Item #'..tostring(id))
end
local function party_snapshot()
  local names={}
  for _,m in ipairs(game.party() or {}) do if m.party==0 then names[m.id]=m.name end end
  return names
end
events.on('party_change',function()
  local names=party_snapshot();local me=game.player()
  if party_names and tracker.ready and settings.get('party_notices') then
    for id,name in pairs(names) do if not party_names[id] and (not me or id~=me.id) then enqueue('Party: '..name..' joined','Observed party membership') end end
    for id,name in pairs(party_names) do if not names[id] and (not me or id~=me.id) then enqueue('Party: '..name..' left','Observed party membership') end end
  end
  party_names=names
end)
timer.every(0.25,function()
  if not enabled or not settings.get('on') then
    tracker.pending={}
    return
  end
  if not tracker.ready then
    local me=game.player();local capacity=game.bag_size and game.bag_size(0)
    if me and type(me.id)=='number' and me.id>0 and capacity and capacity>0 and clock()>=warm_until and clock()-last_change>=2 then
      if not seed or seed.change~=inventory_revision then seed={change=inventory_revision,bag=0,rows={}} end
      for _=1,2 do
        local bag=seed.bag
        for _,it in ipairs(game.items(bag) or {}) do seed.rows[#seed.rows+1]={bag=bag,slot=it.slot,id=it.id,count=it.count} end
        seed.bag=bag+1
      end
      if seed.bag>=18 then tracker:seed(seed.rows);party_names=party_snapshot();seed=nil end
    end
    return
  end
  for _,gain in ipairs(tracker:flush(clock())) do
    enqueue('Inventory + '..item_name(gain.id)..' × '..gain.count,gain.source)
  end
end)
local function draw()
  if not enabled or not settings.get('on') then queue,active={},{};requests.draw(false,1);hide_toasts();return end
  local now=clock();local duration=X.clamp(settings.get('seconds') or 3,1,10)
  for i=#active,1,-1 do if now-active[i].start>=duration then table.remove(active,i) end end
  while #active<4 and #queue>0 do local toast=table.remove(queue,1);toast.start=now;active[#active+1]=toast end
  local base_scale=X.clamp(settings.get('scale') or 100,50,200)/100
  if #active==0 and not preview then
    hide_toasts();requests.draw(settings.get('invite_reminders')~=false,base_scale);return
  end
  R:begin();toasts_shown=true
  local s=hud.screen()
  local scale=base_scale;local u=s.ui_scale*scale
  -- Reserve the whole four-row rise footprint. Clamping each row separately
  -- to zero made multiple toasts overlap after extreme offset/scale settings.
  local footprint=(3*25+20+14)*u
  if footprint>s.height-8 then
    local fit=math.max(0.01,(s.height-8)/footprint);scale,u=scale*fit,u*fit
  end
  local x=X.clamp(s.width/2+(tonumber(settings.get('offset_x')) or 0)*u,0,s.width)
  local y=X.clamp(s.height*0.3+(tonumber(settings.get('offset_y')) or 0)*u,95*u,math.max(95*u,s.height-14*u))
  local visible=active
  if preview then preview_toast.start=now-.4;visible={preview_toast} end
  for i,toast in ipairs(visible) do
    local progress=X.clamp((now-toast.start)/duration,0,1)
    local alpha=progress<0.65 and 255 or math.floor(255*(1-progress)/0.35)
    -- Oldest at top: its greater rise increases the gap from newer messages,
    -- rather than rising into a newer row above it.
    local yy=y-(#visible-i)*25*u-20*u*progress
    local label_width=math.max(10,math.floor(s.width/(7*u)))
    if toast.label_width~=label_width then toast.label_width=label_width;toast.label=X.short(toast.text,label_width) end
    R:text('xn_toast'..i,toast.label,x,math.max(0,yy),12,scale,40,
      {240,255,255,alpha},'center')
  end
  R:finish()
  requests.draw(settings.get('invite_reminders')~=false,base_scale)
end
events.on('frame',draw)
commands.register('xn',function(arg)
  arg=tostring(arg or ''):lower()
  if arg=='preview' then preview=not preview;queue,active={},{};requests.preview(preview)
  elseif arg=='requests' then requests.open()
  elseif arg=='off' then enabled=false;preview=false;queue,active={},{};tracker.pending={};requests.preview(false);requests.reset();R:hide()
  elseif arg=='on' then enabled=true
  elseif arg=='status' then chat.add('XIV Notify: '..(enabled and 'on' or 'off')..'; '..(tracker.ready and 'settled inventory tracking' or 'warming up')..'; gains are not attributed loot')
  else chat.add('Usage: //xn [on|off|preview|requests|status]') end
end)
mods.provide('status',function()return {ready=tracker.ready,queued=#queue,active=#active,preview=preview} end)
mods.provide('busy',requests.busy)
