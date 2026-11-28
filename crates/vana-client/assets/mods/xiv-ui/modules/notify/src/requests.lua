-- Recent invite reminders, not a claim about native pending-request state.
-- Respond only through the native Party menu; never call join/decline directly.
local X=require('src.xiv')
local style=require('src.style')
-- Invite reminder offsets and toggles are manifest keys (notify_* in mod.toml).
local panel_settings
local R=X.renderer()
local M={window='xn_requests'}
function M.bind_settings(s) panel_settings = s end
local function setting(key) return panel_settings and panel_settings.get(key) end
local latest,sequence,hovered=nil,0,false
local preview=false
local busy_until=0
local banner
local last_presentation,shown=nil,false
local function same_key(a,b)
  if not a or #a~=#b then return false end
  for i=1,#b do if a[i]~=b[i] then return false end end
  return true
end
local function hide()
  if shown or not R:ready() then R:hide() end
  last_presentation=nil;shown=false
end
local function current()
  if preview then return {from='PREVIEW player',kind='party',sequence=-1,at=clock(),expires=clock()+120,preview=true} end
  if latest and clock()>=latest.expires then latest=nil;ui.close(M.window) end
  return latest
end
function M.reset()latest=nil;banner=nil;hovered=false;sequence=sequence+1;busy_until=0;ui.close(M.window);R:hide();last_presentation=nil;shown=false end
function M.preview(on)preview=on;last_presentation=nil;ui.close(M.window) end
function M.busy()return ui.is_open(M.window) or clock()<busy_until end
function M.open()
  local info=game.info()
  if info.menu_open then chat.add('XIV Notify: close the current game menu first.');return end
  for _, owner in ipairs({ "target", "party" }) do
    if X.call(owner, "busy") then chat.add("XIV Notify: close the current detail window first."); return end
  end
  local request=current()
  if not request then chat.add('XIV Notify: no recent invite reminder. Use the native Party menu to check invitations.');return end
  local token=request.sequence
  ui.window(M.window,{title=request.preview and 'PREVIEW — invitation reminder' or 'Recent '..request.kind..' invitation',width=420,modal=true,elements={
    {type='text',id='from',text='Received from: '..request.from},
    {type='text',id='hint',text='This records a received invite, not verified pending state. The native Party menu checks the current invitation and handles Join/Decline.'},
    {type='buttons',id='respond_'..token,items={'Open native Party menu','Close'}},
  }})
  ui.open(M.window)
end
ui.on(M.window,function(_,e)
  if e.kind~='press' then return end
  local request=current()
  if not request or e.id~='respond_'..request.sequence then return end
  if e.index~=1 then ui.close(M.window);return end
  if request.preview then ui.close(M.window);return end
  -- A genuine user press only opens a menu. It cannot accept another inviter.
  local ok=pcall(function()actions.open_menu('party') end)
  if ok then busy_until=clock()+0.25;ui.close(M.window)
  else ui.update(M.window,'hint',{text='Opening the native menu is unavailable. Check the actions permission, or open Party through the game main menu. No invitation response was sent.'}) end
end)
events.on('party_invite',function(from,kind)
  if type(from)~='string' or from=='' or (kind~='party' and kind~='alliance') then return end
  from=X.short(text.strip(from),24)
  local duplicate=latest and latest.from==from and latest.kind==kind and clock()-latest.at<2
  sequence=sequence+1;latest={from=from,kind=kind,sequence=sequence,at=clock(),expires=clock()+120}
  ui.close(M.window)
  if not duplicate then banner={text=from..' sent a '..kind..' invite',at=clock()} end
end)
for _,event in ipairs({'login','logout','zone_change'}) do events.on(event,M.reset) end
events.on('party_change',function()ui.close(M.window) end)
function M.draw(show,scale)
  local request=current()
  if not show or not request then hide();return end
  local s=hud.screen();local u=s.ui_scale*scale
  local w,h=150*u,34*u
  if w>s.width-8 or h>s.height-8 then hide();return end
  local alpha=banner and not preview and clock()-banner.at<3 and math.floor(255*(1-(clock()-banner.at)/3)) or false
  local key={request.sequence,preview,hovered,s.width,s.height,s.ui_scale,scale,
    setting('request_offset_x') or 0,setting('request_offset_y') or 0,alpha}
  if same_key(last_presentation,key) and R:ready() then return end
  R:begin();shown=true
  -- Default centre placement avoids the fixed right-hand alliance panels.
  local x=X.clamp((s.width-w)/2+(setting('request_offset_x') or 0)*u,0,s.width-w)
  local y=X.clamp(s.height*0.38+(setting('request_offset_y') or 0)*u,0,math.max(0,s.height-h))
  local tip={title='Recent '..request.kind..' invite',text='Not verified pending state. Open the native Party menu to respond.'}
  local round=h*0.5
  R:rect('xr_pill',x,y,w,h,45,hovered and style.colour.bg or style.colour.bg_dim,
    {round=round,line=hovered and 1.5*scale or nil,tip=tip})
  R:text('xr_type',request.preview and 'PREVIEW invite' or 'Recent '..request.kind,x+12*u,y+3*u,9,scale,46)
  R:text('xr_from',X.short(request.from,19),x+12*u,y+16*u,10,scale,46)
  R:grip('xr_grip',x,y,w,h,47,function(_,e)
    if e.kind=='mouse_enter' then hovered=true elseif e.kind=='mouse_leave' then hovered=false
    elseif e.kind=='click' and (not e.button or e.button=='left') then M.open() end
  end,'Recent invitation; native Party menu handles the actual response')
  if alpha then
    R:text('xr_banner',X.short(banner.text,math.max(10,math.floor(s.width/(8*u)))),s.width/2,s.height*0.52,13,scale,46,
      {245,238,208,alpha},'center')
  end
  R:finish()
  last_presentation=R:ready() and key or nil
end
return M
