-- XIV player vitals strip: native hud.bar meters (vana-client pattern; see assets/mods/player-bars/).
local X=require('src.xiv')
local model=require('src.vitals')
local R=X.renderer()
local enabled,preview=true,false
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
 capture=function()return {enabled=enabled,preview=preview} end,
 preview=function(on)enabled=true;preview=on end,
 restore=function(s)enabled=s.enabled;preview=s.preview end,
 busy=function()return ui.is_open('xpb_details') end,
})
local settings=cfg.settings
R.drag=require('src.hud_drag').new(cfg,'offset_x','offset_y',function()return R.move_units or 1 end)
local layout,smooth=nil,{}
local geometry,geometry_key,last_presentation
local function same_key(a,b)
  if not a or #a~=#b then return false end
  for i=1,#b do if a[i]~=b[i] then return false end end
  return true
end
local WIN='xpb_details'
local function player()
  if preview then return {id=-1,hp=2350,hp_max=3000,mp=650,mp_max=1000,tp=1250,main_job=5,main_level=99,sub_job=4,sub_level=49} end
  return game.player()
end
local function format(value)return value~=nil and tostring(math.floor(value)) or '?' end
local function job(id)return X.id(id) and res.job(id) or '?' end
local function detail_refresh()
  if not ui.is_open(WIN) then return end
  local p=player();local v=model.read(p);local rows={}
  if v then
    rows={{'HP',format(v.hp.current)..' / '..format(v.hp.total)},
      {'MP',v.mp_less and 'No MP pool' or format(v.mp.current)..' / '..format(v.mp.total)},
      {'TP',format(v.tp.current)..' / 3000 · '..(v.tp.ready and '1000+ ready' or 'below readiness')},
      {'Main job',job(v.job)..' '..format(v.level)}, {'Sub job',job(v.sub_job)..' '..format(v.sub_level)}}
  else rows={{'Vitals','Not yet provided by the client'}} end
  local ok,time=pcall(function()return game.clock() end)
  if ok and time and not preview then rows[#rows+1]={'Earth time',time.earth or '?'} end
  local time_ok,t=pcall(function()return game.time() end)
  if time_ok and t and not preview and t.hour~=nil and t.minute~=nil then rows[#rows+1]={'Vana clock',('%02d:%02d'):format(t.hour,t.minute)} end
  ui.update(WIN,'rows',{rows=rows})
  ui.update(WIN,'hint',{text=preview and 'PREVIEW — synthetic vitals; no targeting or saves.' or
    'Values are client state. TP fills at 1000 readiness, while the number continues to 3000.'})
end
local function details()
  ui.window(WIN,{title='XIV player vitals',width=400,elements={
    {type='list',id='rows',rows={},height=6,columns={{title='Metric',width=105},{title='Client value',width=0}}},
    {type='text',id='hint',text=''}, {type='buttons',id='buttons',items={'Refresh','Close'}},
  }})
  ui.open(WIN);detail_refresh()
end
ui.on(WIN,function(_,e)if e.kind=='press' and e.id=='buttons' then if e.index==1 then detail_refresh() else ui.close(WIN) end end end)
mods.provide('busy',function()return hud_config.active() or ui.is_open(WIN) end)
mods.provide('layout',function()return layout end)
local function draw(dt)
  local active=enabled and settings.get('on')
  local p=active and player() or nil
  local v=active and model.read(p)
  if not v then layout=nil;smooth={};last_presentation=nil;R:hide();return end
  local s=hud.screen();local scale=X.clamp(settings.get('scale') or 100,50,200)/100;local u=s.ui_scale*scale
  R.move_units=s.ui_scale
  local style=settings.get('compact') and 'compact' or 'normal'
  local cast=settings.get('avoid_cast')~=false and X.call('cast','layout')
  local c=cast and not cast.preview and cast.reserve or nil
  local key={s.width,s.height,s.ui_scale,scale,style,v.mp_less,preview,
    tonumber(settings.get('offset_x')) or 0,tonumber(settings.get('offset_y')) or 0,
    c and c.x or false,c and c.y or false,c and c.w or false,c and c.h or false}
  if not same_key(geometry_key,key) then
    local step=style=='compact' and 142 or 160
    local logical_w=style=='compact' and (v.mp_less and 279 or 421) or (v.mp_less and 312 or 472)
    local fill_w=style=='compact' and 116 or 132
    local fit=math.min(1,math.max(1,s.width-8)/(logical_w*u),math.max(1,s.height-8)/(28*u))
    scale,u=scale*fit,u*fit
    local w,h=logical_w*u,28*u
    local x=X.clamp((s.width-w)/2+key[8]*s.ui_scale,0,math.max(0,s.width-w))
    local y=X.clamp(s.height*0.56+key[9]*s.ui_scale,0,math.max(0,s.height-h))
    local overlap=false
    if c and x<c.x+c.w and x+w>c.x and y<c.y+c.h and y+h>c.y then
      local above,below=c.y-h-4*u,c.y+c.h+4*u
      if above>=0 then y=above elseif below+h<=s.height then y=below else overlap=true end
    end
    geometry={x=x,y=y,w=w,h=h,u=u,scale=scale,fill_w=fill_w,rows={}}
    for i,kind in ipairs(v.mp_less and {'hp','tp'} or {'hp','mp','tp'}) do
      geometry.rows[i]={kind=kind,x=x+(15+(i-1)*step)*u}
    end
    local il,ir=(style=='compact' and 10 or 11),1
    geometry.layout={version=1,visible=true,x=x,y=y,w=w,h=h,scale=scale,ui_scale=u,preview=preview,overlap=overlap,mp_less=v.mp_less,
      vx=x+il*u,vw=w-(il+ir)*u}
    geometry_key=key
  end
  layout=geometry.layout
  local presentation={geometry}
  for _,slot in ipairs(geometry.rows) do
    local kind=slot.kind;local row=v[kind]
    -- Continue exactly the existing time-normalized smoothing even on frames
    -- whose quantized image/text output needs no reconstruction.
    smooth[kind]=model.smooth(smooth[kind],row.fraction,dt)
    local fraction=smooth[kind]
    slot.frac=fraction
    presentation[#presentation+1]=math.floor(fraction*400+0.5)
    presentation[#presentation+1]=row.current or false
    presentation[#presentation+1]=row.total or false
    presentation[#presentation+1]=row.ready or false
  end
  if same_key(last_presentation,presentation) and R:ready() then return end
  R:begin()
  local x,y,w,h,u,scale=geometry.x,geometry.y,geometry.w,geometry.h,geometry.u,geometry.scale
  local bar_h,bar_y,bar_w=8*u,y+2*u,geometry.fill_w*u
  for _,slot in ipairs(geometry.rows) do
    local kind,fx,frac=slot.kind,slot.x,slot.frac or 0;local row=v[kind]
    R:bar('pb_'..kind..'_bar',fx,bar_y,bar_w,bar_h,10,frac,kind,
      {title=kind:upper(),text=format(row.current)..' / '..format(row.total)..
        (kind=='tp' and '\nFill reaches full at 1000 TP readiness.' or '')})
    R:put('text','pb_'..kind..'_value',{x=fx+92*u,y=y+9.5*u,text=format(row.current),size=scale,
      font='bold',align='right',colour={253,252,250,255},
      outline={50,50,50,255},outline_width=1.4*scale/math.max(1,scale),layer=12})
  end
  if preview then R:text('pb_preview','PREVIEW',x,y-15*u,10,scale,13) end
  R:grip('pb_grip',x,y,w,h,14,function(_,e)if e.kind=='click' then details() end end,'Player vitals details; //xpb details')
  R:finish()
  last_presentation=R:ready() and presentation or nil
end
events.on('frame',draw);timer.every(0.25,detail_refresh)
events.on('zone_change',function()smooth={};last_presentation=nil end)
commands.register('xpb',function(arg)
  arg=tostring(arg or ''):lower()
  if arg=='on' then enabled=true elseif arg=='off' then enabled=false;preview=false;layout=nil;last_presentation=nil;R:hide();ui.close(WIN)
  elseif arg=='preview' then preview=not preview;smooth={}
  elseif arg=='details' then details()
  elseif arg=='status' then
    chat.add(('XIV Player Bars: %s (panel %s); try //xpb preview'):format(
      enabled and 'on' or 'off', settings.get('on') and 'on' or 'off'))
  elseif arg=='debug' then
    local p=game.player();local v=p and model.read(p)
    chat.add(('XIV player debug: panel=%s vitals=%s hp=%s/%s'):format(
      tostring(settings.get('on')), v and 'ok' or 'nil',
      p and tostring(p.hp), p and tostring(p.hp_max)))
  else chat.add('Usage: //xpb [on|off|preview|details|status]') end
end)
