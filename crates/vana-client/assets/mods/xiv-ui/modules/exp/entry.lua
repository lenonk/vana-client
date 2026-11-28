-- Thin XP/limit/capacity display using native-tracker-derived read-only counting.
local X=require('src.xiv')
local points=require('src.points')
local tracker=points.new()
local R=X.renderer()
local WIN='xxp_details'
local enabled,preview,reset_armed=true,false,false
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
 capture=function()return {enabled=enabled,preview=preview} end,
 preview=function(on)enabled=true;preview=on;reset_armed=false end,
 restore=function(s)enabled=s.enabled;preview=s.preview end,
 busy=function()return ui.is_open('xxp_details') end,
})
local settings=cfg.settings
R.drag=require('src.hud_drag').new(cfg,'offset_x','offset_y',function()return R.move_units or 1 end,function()if settings.get('attach_vitals') then chat.add('XIV XP: attached to Player Bars; move player bars or turn off Attach to vitals in Mods.');return false end;return true end)
local choices={'auto','exp','limit','capacity'}
local cached_reading,read_at
local cached_row,row_at
local last_presentation
local function same_key(a,b)
  if not a or #a~=#b then return false end
  for i=1,#b do if a[i]~=b[i] then return false end end
  return true
end
local function reading(force)
  if preview then return {job=5,level=99,exp=12000,exp_next=56000,limit=2500,merits=6,merit_max=75,capacity=18500,jp=12,jp_spent=20} end
  if not force and cached_reading and clock()-read_at<1 then return cached_reading end
  local p=game.player();local s=game.stats()
  if not p or not s or not X.id(p.main_job) then return nil end
  local jp=(game.job_points() or {})[p.main_job]
  local info=game.info() or {}
  local time_ok,t=pcall(function()return game.time() end)
  local value={job=p.main_job,level=p.main_level,exp=s.exp,exp_next=s.exp_next,
    limit=s.limit_points,merits=s.merit_points,merit_max=s.merit_max,
    capacity=jp and jp.capacity,jp=jp and jp.points,jp_spent=jp and jp.spent,
    master_level=s.master_level,exemplar=s.exemplar,exemplar_next=s.exemplar_next,
    mog=info.mog_house and true or false,vana_time=time_ok and t or nil}
  cached_reading,read_at=value,clock();return value
end
for _,event in ipairs({'job_change','level_up','level_down','zone_change','logout','settings_changed'}) do
  events.on(event,function()cached_reading=nil;cached_row=nil end)
end
local function value(n)return n~=nil and tostring(math.floor(n)) or '?' end
local function selected(r)
  local choice=choices[math.floor(X.clamp(settings.get('mode') or 1,1,4))]
  local row=points.select(r,choice)
  if preview and row then row.rate=36000;row.eta=(row.total-row.current)/10;return row end
  return tracker:detail(row,clock())
end
local function duration(n)
  if not n then return '?' end
  n=math.max(0,math.floor(n));local h=math.floor(n/3600);local m=math.floor(n%3600/60)
  return h>0 and ('%dh%02dm'):format(h,m) or m>0 and (m..'m') or (n..'s')
end
local function view()
  local r=reading()
  if not r then return nil end
  if not cached_row or not row_at or clock()-row_at>=1 then
    cached_row=selected(r);row_at=clock()
  end
  return r,cached_row
end
local function detail_refresh()
  if not ui.is_open(WIN) then return end
  local r,row=view()
  local rows={}
  if r then
    rows={{'Job',(res.job(r.job) or '?')..' '..value(r.level)},
      {'EXP',value(r.exp)..' / '..value(r.exp_next)}, {'Limit',value(r.limit)..' / 10000'},
      {'Merits',value(r.merits)..' / '..value(r.merit_max)}, {'Capacity',value(r.capacity)..' / 30000'},
      {'Job points',value(r.jp)..' · spent '..value(r.jp_spent)}}
    if r.master_level~=nil then rows[#rows+1]={'Master level',value(r.master_level)} end
    if r.exemplar~=nil then rows[#rows+1]={'Exemplar (no rate)',value(r.exemplar)..' / '..value(r.exemplar_next)} end
  else rows={{'Points','Not yet supplied by client stats'}} end
  if row then
    rows[#rows+1]={'Observed rate',row.rate and (value(row.rate)..'/h') or 'Warming up: 60s after first gain'}
    rows[#rows+1]={'Estimated next point',row.eta and duration(row.eta) or '?'}
    if row.incomplete then rows[#rows+1]={'Rate caveat','Some gains may be missing; ETA withheld'} end
  end
  local ok,c=pcall(function()return game.clock() end)
  if ok and c and not preview then rows[#rows+1]={'Earth time',c.earth or '?'} end
  local time_ok,t=pcall(function()return game.time() end)
  if time_ok and t and not preview then
    if t.hour~=nil and t.minute~=nil then rows[#rows+1]={'Vana clock',('%02d:%02d'):format(t.hour,t.minute)} end
    if t.weekday~=nil and X.id(t.weekday) and res.day then rows[#rows+1]={'Vana day',res.day(t.weekday) or '?'} end
    if t.moon~=nil then rows[#rows+1]={'Moon',value(t.moon)..'% (not rested XP)'} end
  end
  ui.update(WIN,'rows',{rows=rows})
  ui.update(WIN,'hint',{text=preview and 'PREVIEW — synthetic values; reset cannot affect real rates.' or
    'Auto is level/data based, not the game EXP/merit switch (not exposed). Rates are observed samples, reset per job; multi-level jumps/spending can be incomplete.'})
end
local function details()
  reset_armed=false
  ui.window(WIN,{title='XIV points / job / time',width=450,elements={
    {type='list',id='rows',rows={},height=8,columns={{title='Metric',width=155},{title='Client / observed value',width=0}}},
    {type='text',id='hint',text=''}, {type='buttons',id='buttons',items={'Reset rates','Close'}},
  }})
  ui.open(WIN);detail_refresh()
end
ui.on(WIN,function(_,e)
  if e.kind~='press' or e.id~='buttons' then return end
  if e.index==1 and not preview then
    if not reset_armed then reset_armed=true;ui.update(WIN,'buttons',{items={'Confirm reset','Cancel'}})
    else tracker:reset(reading());reset_armed=false;ui.update(WIN,'buttons',{items={'Reset rates','Close'}});detail_refresh() end
  elseif reset_armed then reset_armed=false;ui.update(WIN,'buttons',{items={'Reset rates','Close'}})
  else ui.close(WIN) end
end)
mods.provide('busy',function()return hud_config.active() or ui.is_open(WIN) end)
mods.provide('points',function()local r=reading();return r and selected(r) end)
local function draw()
  if not enabled or not settings.get('on') then last_presentation=nil;X.native('experience',false);R:hide();return end
  local r,row=view()
  if not row then last_presentation=nil;X.native('experience',false);R:hide();return end
  local s=hud.screen();local scale=X.clamp(settings.get('scale') or 100,50,200)/100;local u=s.ui_scale*scale
  local attach=settings.get('attach_vitals')
  local anchor=attach and X.call('player','layout') or nil
  local stats=settings.get('stats_text') and true or false
  local eta=stats and row.eta
  local key={s.width,s.height,s.ui_scale,scale,tonumber(settings.get('offset_x')) or 0,
    tonumber(settings.get('offset_y')) or 6,settings.get('dark_thin')~=false,preview,stats,
    r.level or false,row.label,row.current,row.total,r.mog or false,
    r.vana_time and r.vana_time.moon or false,
    stats and row.rate and math.floor(row.rate) or false,
    eta and eta>=60 or false,eta and math.floor(math.max(0,eta)/(eta>=60 and 60 or 1)) or false,
    stats and row.incomplete or false,
    anchor and anchor.visible or false,anchor and anchor.preview or false,
    anchor and anchor.x or false,anchor and anchor.y or false,anchor and anchor.w or false,anchor and anchor.h or false,settings.get('attach_below')~=false,
    anchor and anchor.ui_scale or false,anchor and anchor.scale or false,anchor and anchor.vx or false,anchor and anchor.vw or false}
  if same_key(last_presentation,key) and R:ready() then X.native('experience',not preview);return end
  R:begin()
  local fit=math.min(1,math.max(1,s.width-16)/(472*u),math.max(1,s.height-8)/(38*u))
  scale,u=scale*fit,u*fit
  R.move_units=u
  local w=472*u
  local x=X.clamp((s.width-w)/2+(tonumber(settings.get('offset_x')) or 0)*u,0,math.max(0,s.width-w))
  local y=X.clamp((tonumber(settings.get('offset_y')) or 6)*u,0,math.max(0,s.height-30*u))
  local text_y=y+8*u
  if attach then
    if anchor and anchor.visible and not (anchor.preview and not preview) then
      if settings.get('attach_below')~=false then
        local au=anchor.ui_scale
        x,w,u,scale=anchor.vx or anchor.x,anchor.vw or anchor.w,au,anchor.scale
        if not anchor.vx then
          local lw=math.floor(anchor.w/au+0.5);local il=(lw==421 or lw==279) and 10 or 11
          x,w=anchor.x+il*au,anchor.w-(il+1)*au
        end
        y=anchor.y+(anchor.h or 28*au)+4*u;text_y=y+8*u
        local bar_h=(settings.get('dark_thin')~=false and 6 or 5)*u
        if text_y+14*u>s.height then
          text_y=math.max(0,s.height-14*u);y=text_y-8*u
          if y+bar_h>s.height then y=math.max(0,s.height-bar_h) end
        end
      elseif anchor.y>=25*anchor.ui_scale then
        x,w,u,scale=anchor.vx or anchor.x,anchor.vw or anchor.w,anchor.ui_scale,anchor.scale
        if not anchor.vx then
          local lw=math.floor(anchor.w/anchor.ui_scale+0.5);local il=(lw==421 or lw==279) and 10 or 11
          x,w=anchor.x+il*u,anchor.w-(il+1)*u
        end
        y=anchor.y-8*u;text_y=y-15*u
      end
    end
  end
  if w<=8*u or text_y<0 or text_y+14*u>s.height then last_presentation=nil;X.native('experience',false);R:finish();return end
  local thin=settings.get('dark_thin')~=false
  local h=(thin and 6 or 5)*u
  local frac=X.clamp(row.current/row.total,0,1)
  R:bar('xxp_bar',x,y,w,h,8,frac,'experience')
  local label=(preview and 'PREVIEW ' or '')..'Lv '..value(r.level)..' · '..row.label..' '..value(row.current)..'/'..value(row.total)
  if stats then
    label=label..' · '..(row.rate and (value(row.rate)..'/h') or 'rate warming')
    if row.eta then label=label..' · ~'..duration(row.eta) end
    if row.incomplete then label=label..' *' end
  end
  R:text('xxp_text',X.short(label,math.max(12,math.floor(w/(6*u)))),x,text_y,11,scale,10,{255,245,191,255})
  R:grip('xxp_grip',x,y,w,h,12,function(_,e)if e.kind=='click' then details() end end,'Observed XP/limit/capacity; click or //xxp details')
  if not preview and r.mog and x+w+28*u<=s.width then
    local t=r.vana_time
    local mw,mh=24*u,24*u
    local mx,my=x+w+4*u,math.max(0,y-8*u)
    local pct=X.clamp(tonumber(t and t.moon) or 100,0,100)
    local tip={title='Mog House indicator',text='Shows Vana’diel mog-house moon fill, not a foreign rested-XP meter.'..(t and t.moon~=nil and ('\nMoon: '..t.moon..'%') or '')}
    R:rect('xxp_mog_moon',mx,my,mw,mh,10,{255,245,191,math.floor(50+205*pct/100)},{round=mh*0.5,line=1.2*scale,tip=tip})
  end
  X.native('experience',not preview);R:finish()
  last_presentation=R:ready() and key or nil
end
timer.every(1,function()
  -- Track real state even during preview; never count synthetic points.
  local was=preview;preview=false;local r=reading(true);preview=was
  if r then tracker:sample(r,clock());cached_row=nil;row_at=nil end
  draw();detail_refresh()
end)
-- Settled gate: once drawn and ready, re-check at ~15 Hz instead of every frame (the view/anchor reads and key build
-- are the cost; the bar's data changes slowly). Anything unsettled or hidden still runs every frame.
local draw_at=-1
-- Startup stagger: this HUD builds ~8 s after the mod loads, so it does not compete with the party list and the
-- other HUDs while everything is loading at login. The native experience bar stays visible until then.
local start_at=clock()+8
events.on('frame',function()
  local now=clock()
  if now<start_at then return end
  if last_presentation and R:ready() and now-draw_at<.066 then return end
  draw_at=now;draw()
end)
events.on('job_change',function()tracker:reset();cached_row=nil;row_at=nil end)
events.on('zone_change',function()reset_armed=false end)
commands.register('xxp',function(arg)
  arg=tostring(arg or ''):lower()
  if arg=='on' then enabled=true elseif arg=='off' then enabled=false;preview=false;last_presentation=nil;X.native('experience',false);R:hide();ui.close(WIN)
  elseif arg=='preview' then preview=not preview;cached_row=nil;row_at=nil;reset_armed=false
  elseif arg=='details' then details()
  elseif arg=='status' then chat.add('XIV XP: '..(enabled and 'on' or 'off')..'; display modes do not change game EXP/merit allocation')
  else chat.add('Usage: //xxp [on|off|preview|details|status]') end
end)
