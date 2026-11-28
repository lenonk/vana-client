-- Party, alliances, pets; hud.bar vitals and hud.icon status strips.
local X = require("src.xiv")
local R = X.renderer()
local trust_book = require("src.trusts")
local prep_at,prep_steps,prep_pending,prep_exhausted
-- Small, finite timing window. A phase left open on the next draw is an
-- unfinished boundary, not the native interior or duration of a failed call.
local probe={}
do
  local expires,serial,started,boundary,phase,phase_at,totals,frames,aborts,defers,slowest
  local last_manual,visible,ready_frames,first_visible,first_ready
  local order={'visibility','snapshots','pet_resolve','cast_batch','layout','row_entity',
    'row_cast','row_buffs','row_status_prep','row_status_name','row_status_info','row_art',
    'row_status_specs','row_base','row_specs','drain','native'}
  function probe.report(reason)
    if not totals or not log or not log.warn then return end
    local fields={('VANA_PARTY_RUNTIME reason=%s window=%d frames=%d visible=%d ready_frames=%d first_ready_us=%d unfinished=%d deferred=%d max_frame_us=%d renderer_pending=%d preparation_pending=%d renderer_ready=%d phase=%s'):
      format(reason,serial,frames,visible,ready_frames,first_ready or 0,aborts+(started and 1 or 0),defers,slowest,
        R.pending,prep_pending and 1 or 0,R:ready() and 1 or 0,phase or boundary or 'none')}
    for _,name in ipairs(order) do
      local item=totals[name]
      if item then fields[#fields+1]=('%s=%d,%d'):format(name,item[1],item[2]) end
    end
    log.warn(table.concat(fields,' '))
  end
  function probe.arm()
    if not log or not log.warn then return end
    serial=(serial or 0)+1
    started,boundary,phase,frames,aborts,defers,slowest,totals=nil,nil,nil,0,0,0,0,{}
    visible,ready_frames,first_visible,first_ready=0,0,nil,nil
    expires=clock()+45
    local window=serial
    timer.after(45,function()
      if serial==window and expires then expires=nil;probe.report('end') end
    end)
    probe.report('start')
  end
  function probe.manual()
    if not log or not log.warn then return end
    local now=clock()
    if last_manual and now-last_manual<2 then return end
    last_manual=now;probe.report('manual');probe.arm()
  end
  function probe.begin(at)
    if not expires or at>=expires then return end
    if started then
      aborts=aborts+1;boundary=phase;started=nil
      if aborts<=2 then probe.report('unfinished') end
    end
    started=at;phase=nil
    probe.mark('visibility',at)
  end
  function probe.mark(name,at)
    if not expires or not started or name==phase then return end
    at=at or clock()
    if phase then
      local us=math.floor(math.max(0,at-phase_at)*1000000+.5)
      local item=totals[phase]
      if item then item[1]=item[1]+1;item[2]=math.max(item[2],us) else totals[phase]={1,us} end
    end
    phase,phase_at=name,at
  end
  function probe.visible()
    if expires and started then first_visible=first_visible or started end
  end
  function probe.done(deferred,drawing,ready)
    if not expires or not started then return end
    local now=clock()
    probe.mark(nil,now);slowest=math.max(slowest,math.floor(math.max(0,now-started)*1000000+.5))
    if drawing then
      visible=visible+1
      if ready then
        ready_frames=ready_frames+1
        first_ready=first_ready or math.floor(math.max(0,now-first_visible)*1000000+.5)
      end
    end
    frames=frames+1;started=nil
    if deferred then defers=defers+1 end
  end
end
local enabled, preview = true, false
local hud_config=require('src.hud_config')
local cfg=hud_config.new(settings,require('src.config_schema'),{
 capture=function()return {enabled=enabled,preview=preview} end,
 preview=function(on)enabled=true;preview=on end,
 restore=function(s)enabled=s.enabled;preview=s.preview end,
 busy=function()return ui.is_open('xpl_details') end,
})
local settings=cfg.settings
R.drag=require('src.hud_drag').new(cfg,'offset_x','offset_y',function()return R.move_units or 1 end)
local current, details_members, details_status, member_at, status_at = {}, {}, {}, 1, 1
local WIN = "xpl_details"
local detail_id
local smooth = {}
local frame_player
local frame_cast_ids,frame_casts
local cast_cache,cast_at,cast_key
-- Stop admitting more synchronous row leaves once the draw's preparation
-- allowance is spent. Always allow one to make progress; a single blocking
-- native call is not preemptible by this check or by the HUD writer's cap.
local function admit_row_leaf()
  if prep_exhausted then return false end
  if prep_steps>0 and clock()-prep_at>=.004 then prep_exhausted=true;return false end
  prep_steps=prep_steps+1
  return true
end
-- Display samples are taken about every frame (0.015 s) so the cast fill glides. Batch capability failures and each individual
-- ID's failures expire in 2s; one bad row must not suppress later healthy rows.
local CAST_REFRESH,CAST_RETRY=0.015,2
local batch_retry_at=0
local single_casts,single_retries={},{}
local single_window_at,single_attempts
local function invalidate_casts()
  cast_cache,cast_at,cast_key,frame_casts=nil,nil,nil,nil;single_casts={}
  -- Roster/settings/zone changes discard values, not the transport backoff.
end
local function cast_id(id)
  return type(id)=='number' and id>0 and id<=4294967295 and id%1==0
end
local function valid_cast(c)
  return type(c)=='table' and type(c.name)=='string' and type(c.progress)=='number' and
    c.progress>=0 and c.progress<=1 and (c.source==nil or type(c.source)=='string')
end
local function cast_call(method, ...)
  if type(X.call) ~= "function" then return false end
  local ok, got = pcall(X.call, "target", method, ...)
  if ok then return true, got end
  return false
end
local function prepare_casts()
  if preview then frame_casts=nil;return end
  local now=clock()
  local key=table.concat(frame_cast_ids or {},',')
  if key~=cast_key or not cast_at or now-cast_at>=CAST_REFRESH or
      (#frame_cast_ids>0 and batch_retry_at>0 and now>=batch_retry_at) then
    if key~=cast_key then single_casts={} end
    cast_cache={};cast_at,cast_key=now,key
    if #frame_cast_ids>0 and now>=batch_retry_at then
      local ok,got=cast_call('casts_for',frame_cast_ids)
      local known=0
      if ok and type(got)=='table' then
        for _,id in ipairs(frame_cast_ids) do
          local c=got[id]
          -- Native copied maps stringify non-sequence numeric keys. Do not use
          -- `or` here: false is authoritative, unlike an omitted/bad entry.
          if c==nil then c=got[tostring(id)] end
          if c==false or valid_cast(c) then cast_cache[id]=c;known=known+1 end
        end
      end
      if known>0 then
        batch_retry_at=0
      else batch_retry_at=now+CAST_RETRY end
    end
  end
  frame_casts=cast_cache
end
local function member_cast(id)
  if preview or not cast_id(id) then return nil end
  if not frame_casts then prepare_casts() end
  if frame_casts[id]~=nil then return frame_casts[id] or nil end
  -- Missing batch entries (including partial/malformed maps) still support an
  -- older provider. Cache successful no-cast separately from failed transport.
  local now=clock();local old=single_casts[id]
  if old and now-old.at<CAST_REFRESH then return old.value or nil end
  if single_retries[id] and now<single_retries[id] then return nil end
  -- Admission budget, not a global negative capability: all <=24 current IDs
  -- fit a fresh 0.2s window. Roster churn can defer a new ID by at most a window,
  -- not let an early failing ID renew a deadline that starves later IDs.
  -- Pruning on admission bounds retained failures to <=11 windows * 24 IDs,
  -- even with unlimited distinct IDs under churn. Invalidation resets neither.
  if not single_window_at or now-single_window_at>=CAST_REFRESH then
    single_window_at,single_attempts=now,0
    for key,at in pairs(single_retries) do if now>=at then single_retries[key]=nil end end
  end
  if single_attempts>=24 then return nil end
  if not admit_row_leaf() then
    local known=old and now-old.at<2*CAST_REFRESH
    return known and old.value or nil,true,known
  end
  single_attempts=single_attempts+1
  probe.mark('row_cast')
  local ok,c=cast_call('cast',id)
  probe.mark('row_specs')
  if ok and (c==nil or c==false or valid_cast(c)) then
    single_retries[id]=nil
    c=c or false;single_casts[id]={at=now,value=c};return c or nil
  end
  single_retries[id]=now+CAST_RETRY
  return nil
end
-- Native res.job is a fixed 1..22 table. Reuse it as data; no 22-RPC scan for
-- each unmapped trust hint and no job/resource calls per cold badge (see src/trusts.lua).
local job_names={'war','mnk','whm','blm','rdm','thf','pld','drk','bst','brd','rng',
  'sam','nin','drg','smn','blu','cor','pup','dnc','sch','geo','run'}
local trust_index,job_ids={},{}
for id,name in ipairs(job_names) do job_ids[name:upper()]=id end
local function trust_index_add(name,entry)
  local key=(name or ''):lower()
  if key=='' then return end
  local bucket=trust_index[key] or {}
  for _,existing in ipairs(bucket) do
    if existing==entry then trust_index[key]=bucket;return end
  end
  bucket[#bucket+1]=entry
  trust_index[key]=bucket
end
for _,entry in ipairs(trust_book) do
  trust_index_add(entry.name,entry)
  if entry.spell and res.spell then
    local spell=res.spell(entry.spell)
    if spell and spell.name and spell.name~='' then trust_index_add(spell.name,entry) end
  end
end
local pet_ids,pet_at={},-1
local pet_token,pet_scanning,pet_started=0,false,0
local status_cache={}
local status_fast={}
local status_prepared={}
local self_buffs
local generation,roster_key,layout_key,layout_revision,presenting=0,nil,nil,0,false
local roster,layout
local pet_task,pet_fallback
-- Visibility only needs settings, event state and (for Hide Solo) membership.
-- Do not fetch player/target/entities/statuses before this gate.
local function visible_members()
  if not enabled or not settings.get('on') then return nil,false end
  if not preview and settings.get('hide_cutscene') and game.info().event then return nil,false end
  if preview then return {},false end
  local members=game.party() or {}
  if settings.get('hide_solo') and #members==1 then return nil,true end
  return members,false
end
local function clear_pets()
  pet_ids,pet_at={},-1;pet_token=pet_token+1;pet_scanning=false
end
local function set_pet_demand(on)
  if not on then
    if pet_task~=nil and not pet_fallback and type(timer.cancel)=='function' then
      local ok,cancelled=pcall(timer.cancel,pet_task)
      if ok and cancelled~=false then pet_task=nil else pet_fallback=true end
    end
    if pet_at~=-1 or pet_scanning then clear_pets() end
  end
end
events.on('zone_change',function()
  set_pet_demand(false);invalidate_casts();status_cache={};status_prepared={};status_fast={};smooth={};self_buffs=nil
  generation=generation+1;roster_key=nil;current={}
end)
events.on('buff_gain',function()self_buffs=nil end)
events.on('buff_lose',function()self_buffs=nil end)
events.on('settings_changed',function()
  set_pet_demand(false);invalidate_casts();status_fast={}
  if not settings.get('on') then
    generation=generation+1;roster_key=nil;layout_key=nil;presenting=false;current={}
    invalidate_casts();smooth={};self_buffs=nil
    R:hide();ui.close(WIN);X.native('party',false)
  end
end)
-- Only DRG, BST, SMN, PUP and GEO bring pets (job ids by job_names order).
local PET_JOBS={[9]=true,[14]=true,[15]=true,[18]=true,[21]=true}
local pet_idle=0
local function discover_pets()
  local members=visible_members()
  if not members or #members==0 or preview or settings.get('pets')==false then set_pet_demand(false);return end
  if pet_scanning and clock()-pet_started<5 then return end
  -- The scan copies every entity within 50 yalms. Parties with no possible pet
  -- owner (and no known pets) are probed at a sixth of the rate (every 3 s).
  if #pet_ids==0 then
    local likely=false
    for _,m in ipairs(members) do
      local j=m.main_job
      if not j or j==0 or PET_JOBS[j] or PET_JOBS[m.sub_job or 0] then likely=true;break end
    end
    if not likely then
      pet_idle=pet_idle+1
      if pet_idle<6 then return end
    end
  end
  pet_idle=0
  pet_token=pet_token+1;local token=pet_token;pet_scanning=true;pet_started=clock()
  local owners={};for _,m in ipairs(members) do if m.index and m.index>0 then owners[m.index]=true end end
  -- The native snapshot is synchronous; yielding below only slices Lua work.
  -- Retain observed party/alliance pets, not just game.player().pet.
  local fresh={};local entries=game.entities({distance=50})
  for i,e in ipairs(entries) do
    if token~=pet_token then return end
    if (e.pet or (e.master and e.master>0 and owners[e.master])) and not e.hidden and not e.trust then fresh[#fresh+1]=e.id end
    if i%32==0 then sleep(0) end
  end
  if token==pet_token then pet_ids,pet_at=fresh,clock();pet_scanning=false end
end
local function demand_pets(on)
  set_pet_demand(on)
  if on and pet_task==nil and not pet_fallback then
    pet_task=timer.every(0.5,discover_pets)
    -- Older hosts/fixtures have no cancellation or no timer handle. Keep one
    -- guarded callback, never install duplicates on every visible frame.
    if pet_task==nil then pet_fallback=true end
  end
end
if type(timer.cancel)~='function' then
  pet_task=timer.every(0.5,discover_pets);pet_fallback=true
else
  local members=visible_members()
  demand_pets(members~=nil and #members>0 and not preview and settings.get('pets')~=false)
end
local function code(id) return job_names[id] end
local function badge_label(main_job)
  local job = code(main_job)
  if job then return job:upper() end
  if X.id(main_job) and res.job then
    local name = res.job(main_job)
    if type(name) == "string" and name ~= "" then return name:upper():sub(1, 3) end
  end
  return "?"
end
local cold_steps
local function cold_step()
  if cold_steps>=12 or not admit_row_leaf() then return false end
  cold_steps=cold_steps+1
  return true
end
local metadata_revision=0
local function metadata(key,phase)
  local original=X[key]
  local cache={}
  X[key]=function(id)
    local old=cache[id]
    if old then return old.value end
    return original(id)
  end
  return function(id,now,raw)
    local old=cache[id]
    -- Details retain their independent live queries: the display's short
    -- negative cache must not block a lookup from the details window.
    if raw and (not old or old.value~=nil) then return true end
    if old and (old.value~=nil or (not raw and now<old.retry)) then return true end
    if not raw and not cold_step() then return false end
    if not raw then probe.mark(phase) end
    local value=original(id)
    if not raw then probe.mark('row_status_prep') end
    if old and value~=nil then metadata_revision=metadata_revision+1 end
    cache[id]={value=value,retry=now+2}
    return true
  end
end
local status_name_ready=metadata('status_name','row_status_name')
local status_info_ready=metadata('status_info','row_status_info')
local function sample_buffs()
  local list={}
  for _,e in ipairs(game.buffs() or {}) do
    list[#list+1]={id=e.id,remaining=e.remaining,source='Client status state'}
  end
  return list
end
-- Rebuilding a member's status signature (a table, four strings and a sort per
-- buff) every frame only to learn nothing changed is pure garbage. Reuse the
-- last result for 100 ms; the details window (raw) always queries live.
local function fast_status(id,rows,key,render_key,count)
  status_fast[id]={at=clock(),rows=rows,key=key,render_key=render_key,count=count}
  return rows,key,render_key
end
local function statuses(m, raw, count)
  if m.in_zone == false then return {},'offzone' end
  if not raw and not preview then
    local fast=status_fast[m.id]
    if fast and fast.count>=count and clock()-fast.at<.1 then return fast.rows,fast.key,fast.render_key end
  end
  local list = m.buffs or {}
  local me = not preview and (raw and game.player() or frame_player)
  if me and m.id == me.id then
    if raw then list=sample_buffs()
    elseif self_buffs and self_buffs.id==m.id and clock()-self_buffs.at<.5 then list=self_buffs.list
    else return nil end
  elseif raw and not preview then
    local tracked = X.call("target", "effects", m.id)
    if tracked then list = tracked end
  end
  local hidden = not raw and not preview and storage.account.status_hidden or nil
  if not raw and not preview and settings.get("filter_whitelist") then
    local include = {}; hidden = {}
    for _, id in ipairs(storage.account.status_order or {}) do include[tonumber(id) or -1] = true end
    for _, e in ipairs(list) do
      local id = type(e) == "table" and (e.id or e.status) or e
      if id and not include[id] then hidden[tostring(id)] = true end
    end
  end
  local order=not raw and not preview and storage.account.status_order or nil
  local cache_key,timers
  if not raw then
    local parts={}
    timers={}
    for _,e in ipairs(list) do
      local id=type(e)=='table' and (e.id or e.status) or e
      if X.id(id) and not (hidden and hidden[tostring(id)]) and timers[id]==nil then
        if type(e)~='table' then e={id=id} end
        local left=e.left or e.remaining
        if left==nil or left>0 then
          timers[id]={left=left,total=e.secs or e.total}
          -- Countdown labels are not drawn on rows: only active membership,
          -- names and status sources belong to this static icon revision.
          parts[#parts+1]=table.concat({tostring(id),tostring(e.name or ''),tostring(e.source or ''),tostring(left~=nil)},'\30')
        end
      end
    end
    table.sort(parts)
    parts[#parts+1]=table.concat(order or {},",")
    cache_key=table.concat(parts,"\31")
    -- Native lists are at most 32 effects/member. Probe metadata in original
    -- list order, one admitted lookup at a time. The cache cursor is the
    -- committed ID entry, so an escaping lookup retries on a fresh draw.
    local now=clock()
    for _,e in ipairs(list) do
      local id=type(e)=='table' and (e.id or e.status) or e
      if timers[id] and (not status_name_ready(id,now) or not status_info_ready(id,now)) then return nil,cache_key end
    end
    local old=status_cache[m.id]
    if old and old.key==cache_key and old.revision==metadata_revision and count<=old.art_count then
      for _,row in ipairs(old.rows) do
        local live=timers[row.id]
        if live then row.left,row.total=live.left,live.total end
      end
      return fast_status(m.id,old.rows,cache_key,old.render_key,count)
    elseif old and old.key==cache_key and old.revision==metadata_revision then
      -- Native status icons need no cold image lookup when the visible limit grows.
      old.art_count=count
      for _,row in ipairs(old.rows) do
        local live=timers[row.id]
        if live then row.left,row.total=live.left,live.total end
      end
      return fast_status(m.id,old.rows,cache_key,old.render_key,count)
    end
  end
  if raw then
    local now=clock();local seen={}
    for _,e in ipairs(list) do
      local id=type(e)=='table' and (e.id or e.status) or e
      local left=type(e)=='table' and (e.left or e.remaining) or nil
      if X.id(id) and not seen[id] and (not left or left>0) then
        seen[id]=true;status_name_ready(id,now,true);status_info_ready(id,now,true)
      end
    end
    return X.statuses(list,hidden,order)
  end
  local staged=status_prepared[m.id]
  local rows
  if staged and staged.key==cache_key and staged.revision==metadata_revision then
    rows=staged.rows
  else
    if not admit_row_leaf() then return nil,cache_key end
    rows=X.statuses(list, hidden, order)
    -- Retain normalized rows until this roster's bounded preparation completes.
    status_prepared[m.id]={key=cache_key,revision=metadata_revision,rows=rows}
  end
  local render_key=cache_key..'\29'..metadata_revision
  status_cache[m.id]={key=cache_key,revision=metadata_revision,render_key=render_key,rows=rows,art_count=count}
  status_prepared[m.id]=nil
  return fast_status(m.id,rows,cache_key,render_key,count)
end
local function detail_refresh()
  if not ui.is_open(WIN) then return end
  details_members = {}; local names = {}
  for _, m in ipairs(game.party()) do
    details_members[#details_members + 1] = m; names[#names + 1] = m.name or "Member"
  end
  for slot = 19, 24 do
    local m = current[slot]
    if m then details_members[#details_members + 1] = m; names[#names + 1] = "Pet: " .. m.name end
  end
  for i, m in ipairs(details_members) do if m.id == detail_id then member_at = i end end
  member_at = math.min(member_at, math.max(1, #names))
  detail_id = details_members[member_at] and details_members[member_at].id
  details_status = details_members[member_at] and statuses(details_members[member_at], true) or {}
  local rows = {}
  for _, e in ipairs(details_status) do rows[#rows + 1] = { e.name, e.left and X.fmt_time(e.left) or (e.untimed and "—" or "?"),
    icon = { kind = "status", id = e.id } } end
  status_at = math.min(status_at, math.max(1, #rows))
  ui.update(WIN, "member", { items = #names > 0 and names or { "No party" }, at = member_at })
  ui.update(WIN, "statuses", { rows = rows, at = status_at })
  local selected=details_status[status_at];local info=selected and X.status_info(selected.id)
  ui.update(WIN,'description',{text=selected and (selected.name..'\n'..(info and info.description or '')) or ''})
  local m = details_members[member_at]
  local casting = m and X.call("target", "cast", m.id)
  ui.update(WIN, "vitals", { text = m and ("HP " .. tostring(m.hpp or "?") .. "% · MP " ..
    tostring(m.mpp or "?") .. "% · TP " .. tostring(m.tp or "?") ..
    (casting and (" · " .. casting.name .. " ~") or "")) or "No member data" })
end
local function open_details(id)
  detail_id = id
  for i, m in ipairs(game.party()) do if m.id == id then member_at = i end end
  ui.window(WIN, { title = "XIV Party Statuses", width = 420, elements = {
    { type = "dropdown", id = "member", label = "Member", items = { "Party" }, at = 1 },
    { type = "text", id = "vitals", text = "" },
    { type = "list", id = "statuses", rows = {}, height = 8,
       columns = { { title = "Status", width = 0 }, { title = "Time", width = 55 } } },
    { type = "text", id = "description", text = "" },
    { type = "text", id = "hint", text = "All known statuses. ? means time unknown. Hidden statuses still appear here." },
    { type = "buttons", id = "buttons", items = { "Prioritise", "Hide", "Clear filters", "Close" } },
  } }); ui.open(WIN); detail_refresh()
end
ui.on(WIN, function(_, e)
  if e.kind == "set" and e.id == "member" then
    member_at, status_at = e.index, 1; detail_id = details_members[member_at] and details_members[member_at].id; detail_refresh()
  elseif e.kind == "select" and e.id == "statuses" then status_at = e.index
  elseif e.kind == "press" and e.id == "buttons" then
    local row = details_status[status_at]
    if e.index == 1 and row then
      local order = { row.id }; for _, id in ipairs(storage.account.status_order or {}) do
        if id ~= row.id then order[#order + 1] = id end
      end; storage.account.status_order = order; status_fast={}; storage.save()
    elseif e.index == 2 and row then
      storage.account.status_hidden = storage.account.status_hidden or {}
      storage.account.status_hidden[tostring(row.id)] = true; status_fast={}; storage.save()
    elseif e.index == 3 then storage.account.status_hidden, storage.account.status_order = {}, {}; status_fast={}; storage.save()
    elseif e.index == 4 then ui.close(WIN) end
  end
end)
mods.provide("busy", function() return hud_config.active() or ui.is_open(WIN) end)
local function grip(slot, x, y, w, h)
  R:grip("xpl" .. slot .. "_grip", x, y, w, h, 35, function(_, e)
    if e.kind ~= "click" or preview or not enabled or not settings.get('on') then return end
    if hud_config.active() then return end
    local m = current[slot]
    if not m then return end
    local members=visible_members()
    if not members then return end
    if slot<=18 then
      local fresh
      for _,member in ipairs(members) do if member.slot==slot-1 and member.id==m.id then fresh=member;break end end
      if not fresh then return end
      m=fresh
    elseif settings.get('pets')==false then return end
    local entity = game.entity(m.id)
    if slot>18 then
      if not entity or entity.hidden or entity.trust or not entity.distance or entity.distance>50 then return end
      local owned=entity.pet
      for _,member in ipairs(members) do if member.index and member.index>0 and member.index==entity.master then owned=true;break end end
      if not owned then return end
    end
    if e.button == "right" then open_details(m.id); return end
    if entity and m.in_zone ~= false and (not e.button or e.button=='left') then X.target(m.id) end
  end, "Click to target; right-click or //xpl details for all statuses")
end
-- Same shape language as the Hotbar mod slots (filled round rect + thin gold stroke).
local HOTBAR_FILL = { 10, 17, 24, 255 } -- slotview.KEY: filled slot background
local ROLE_TINT_ALPHA = 175 -- role hue over the dark fill (filled slots are opaque KEY)
local JOB_GOLD = { 228, 192, 119, 255 }
local SELECT_GLOW = { 255, 214, 120, 90 }
local VITAL_BAR = { 255, 255, 255, 255 }
local function row_select(id, x, y, w, h, u, layer)
  local round = math.max(3, h * u * 0.08)
  R:rect(id .. "_sel", x - u, y - u, w + 2 * u, h * u + 2 * u, layer, SELECT_GLOW, { round = round + 1 })
  R:rect(id .. "_seledge", x - u, y - u, w + 2 * u, h * u + 2 * u, layer + 1, JOB_GOLD, { line = 1.5, round = round + 1 })
end
local function leader_mark(id, x, y, sc, layer, alliance)
  R:text(id .. "_leader", alliance and "A" or "L", x, y, 8, sc, layer, JOB_GOLD, nil, true)
end
local function role_surface(job)
  local c = X.role(job)
  return { c[1], c[2], c[3], ROLE_TINT_ALPHA }
end
local function job_round(size) return math.max(4, size * 0.17) end
local function job_highlight(id, x, y, size, layer)
  local round = job_round(size) + 1
  R:rect(id .. "_jhi", x - 1, y - 1, size + 2, size + 2, layer,
    { 255, 214, 120, 175 }, { line = 1, round = round })
end
local function badge(id, m, x, y, size, sc, selected)
  local job = code(m.main_job)
  local round = job_round(size)
  if selected then job_highlight(id, x, y, size, 10) end
  R:rect(id .. "_jbg", x, y, size, size, 12, HOTBAR_FILL, { round = round })
  R:rect(id .. "_jrole", x, y, size, size, 13, role_surface(job), { round = round })
  local inset = math.max(2, size * 0.08)
  local isize = math.max(8, size - 2 * inset)
  local ix, iy = x + inset, y + inset
  local path = job and ("img/jobs/" .. job .. ".png") or nil
  if path and X.exists(path) then
    R:image(id .. "_jicon", path, ix, iy, isize, isize, 14)
  else
    local px = math.max(7, isize / math.max(1, sc) * 2.8)
    R:text(id .. "_jlabel", badge_label(m.main_job), x + size / 2, y + size * 0.38, px, sc, 14,
      X.text_colour, "center", true)
  end
  R:rect(id .. "_jframe", x, y, size, size, 15, JOB_GOLD, { line = 1, round = round })
end
-- Main-row HP/MP/TP bars are 25% shorter than the original layout (110 -> 82.5 units),
-- and the three blocks are packed at the same proportional spacing. Change BAR_K to resize.
local BAR_K = 0.75
-- Gap between the HP/MP/TP frames cut by 30% (15.75 -> 11.025 units): step = bar width + 0.7 * old gap.
local BAR_STEP = 110 * BAR_K + (131 * BAR_K - 110 * BAR_K) * 0.7
local BAR_X = { 19, 19 + BAR_STEP, 19 + 2 * BAR_STEP }
local function main_row(slot, m, x, y, u, sc, h, selected, rev)
  local id = "xpl" .. slot
  R:group(id..':identity',{rev,m.id,m.in_zone,m.name,m.main_job,m.sub_job,m.main_level,m.sub_level,m.inferred_job,m.leader,m.alliance_leader},function()
  badge(id, m, x - 11 * u, y - 2 * u, 36 * u, sc, false)
  local job, sub = code(m.main_job), code(m.sub_job)
  R:text(id .. "_j1", (job or "?"):upper() .. (m.inferred_job and "~" or "") .. " " ..
    (m.main_level and m.main_level > 0 and m.main_level or "?"), x + 30 * u, y + u, 9, sc, 16)
  R:text(id .. "_j2", (sub or "?"):upper() .. " " ..
    (m.sub_level and m.sub_level > 0 and m.sub_level or "?"), x + 39 * u, y + 10 * u, 9, sc, 16)
  R:text(id .. "_name", X.short(m.name, 22), x + 95 * u, y + u, 15, sc, 16,
    m.in_zone == false and { 145, 145, 145, 255 } or nil, nil, true)
  if m.leader or m.alliance_leader then
    leader_mark(id, x + 80 * u, y + u, sc, 26, m.alliance_leader)
  end
  grip(slot, x, y, 410 * u, h * u)
  end)
  R:group(id..':selection',{rev,selected},function()
    if selected then
      row_select(id, x, y, 410, h, u, 6)
      job_highlight(id, x - 11 * u, y - 2 * u, 36 * u, 10)
    end
  end)
  for i, k in ipairs({ "hp", "mp", "tp" }) do
    if k ~= "mp" or m.has_mp ~= false then
    local bx = x + (k == "tp" and m.has_mp == false and BAR_X[2] or BAR_X[i]) * u
    local frac = k == "hp" and X.clamp(m.hpp, 0, 100) / 100 or
      (k == "mp" and X.clamp(m.mpp, 0, 100) / 100 or X.clamp(m.tp, 0, 1000) / 1000)
    if k == "hp" then
      frac = smooth[m.id] or frac
    end
    R:group(id..':fill:'..k,{rev,m.has_mp~=false,math.floor(frac*400+0.5)},function()
      R:bar(id .. "_" .. k, bx + 13 * u, y + 21 * u, 102 * BAR_K * u, 8 * u, 9, frac, VITAL_BAR)
    end)
    R:group(id..':value:'..k,{rev,m.has_mp~=false,m[k],k=='hp' and (m.hpp or 100)<50},function()
    R:text(id .. "_" .. k .. "val", m[k] or "?", bx + (10 + 110 * BAR_K) * u, y + 17 * u, 12, sc, 17,
      k == "hp" and (m.hpp or 100) < 50 and { 250, 214, 96, 255 } or nil, "right")
    end)
    end
  end
  local out=m.in_zone==false or (m.distance and m.distance>(settings.get('range') or 21)) or false
  local number=out and settings.get('range_numbers') and m.distance or false
  R:group(id..':range',{rev,out,number},function()
  if m.in_zone == false or (m.distance and m.distance > (settings.get("range") or 21)) then
    if settings.get("range_numbers") and m.distance then
      R:text(id .. "_range_text", ("%.0f"):format(m.distance), x + 14 * u, y + 34 * u, 8, sc, 26, nil, "right")
    else R:text(id .. "_range", "··", x - 6 * u, y + 32 * u, 10, sc, 26, { 180, 180, 180, 255 }) end
  end
  end)
end
local function compact_row(slot, m, x, y, u, sc, selected, rev)
  local id = "xpl" .. slot
  R:group(id..':identity',{rev,m.id,m.name,m.main_job,m.in_zone,m.leader,m.alliance_leader},function()
  badge(id, m, x + 2 * u, y + u, 20 * u, sc, false)
  R:text(id .. "_name", X.short(m.name, 18), x + 27 * u, y + u, 11, sc, 17,
    m.in_zone == false and { 145, 145, 145, 255 } or nil)
  if m.leader or m.alliance_leader then
    leader_mark(id, x + 3 * u, y + 22 * u, sc, 25, m.alliance_leader)
  end
  grip(slot, x, y, 220 * u, 34 * u)
  end)
  R:group(id..':selection',{rev,selected},function()
    if selected then
      row_select(id, x, y, 220, 34, u, 6)
      job_highlight(id, x + 2 * u, y + u, 20 * u, 10)
    end
  end)
  R:group(id..':vitals',{rev,m.hpp,m.mpp},function()
  R:text(id .. "_hpp", tostring(m.hpp or "?") .. "%", x + 212 * u, y + u, 10, sc, 17, nil, "right")
  local hp_frac = X.clamp(m.hpp, 0, 100) / 100
  R:bar(id .. "_hp", x + 28 * u, y + 20 * u, 182 * u, 6 * u, 9, hp_frac, VITAL_BAR)
  if m.mpp and m.mpp > 0 then
    R:bar(id .. "_mp", x + 28 * u, y + 26 * u, 182 * u, 2 * u, 9,
      X.clamp(m.mpp, 0, 100) / 100, VITAL_BAR)
  end
  end)
end
local function draw_cast(id,m,row,u,sc,rev)
  local c,deferred,known=member_cast(m.id)
  if deferred and not known then prep_pending=true end
  local x,y=row.x,row.y
  R:group(id..':cast',{rev,c and c.name,c and c.progress,not row.full and c and c.source},function()
    if not c then return end
    if row.full then
      R:bar(id .. "_cast", x + 95 * u, y + 34 * u, 180 * u, 4 * u, 20, c.progress, "cast")
      R:text(id .. "_castname", X.short(c.name, 24), x + 95 * u, y + 32 * u, 9, sc, 23)
    else
      R:bar(id .. "_cast", x + 26 * u, y + 15 * u, 186 * u, 3 * u, 20, c.progress, "cast",
        { title = c.name, text = c.source })
    end
  end)
  return not deferred
end
-- Status slot geometry. Status art is 3:4 (24x32), so slots are narrower than tall; the
-- pitch is the icon width plus a fixed 4u gap. The first icon of each strip keeps the
-- centre it always had, and the rest pack at the tighter pitch. Returns the icon's left x,
-- top y and height in px; the icon width is X.status_w(height).
local function status_geom(row,i,x,y,u)
  local size,cx,sy
  if not row.full then size,sy=15,y+u
  elseif i<=6 then size,sy=18,y+2*u
  else size,sy=18,y+(46+math.floor((i-7)/20)*22)*u end
  local hpx=size*u
  local iw=X.status_w(hpx)
  local pitch=iw+4*u
  if not row.full then cx=x+(172+size/2)*u-(i-1)*pitch
  elseif i<=6 then cx=x+(288+size/2)*u+(i-1)*pitch
  else cx=x+(30+size/2)*u+((i-7)%20)*pitch end
  return cx-iw/2,sy,hpx
end
local function draw_statuses(id,m,row,L,rev)
  local x,y,u,sc=row.x,row.y,L.u,L.sc
  local count=row.full and L.count or 2
  probe.mark('row_status_prep')
  local list,key,status_rev=statuses(m,false,count)
  -- The player's own row ("me") shows nothing — the buff bar covers all statuses for self.
  -- Other members are unchanged. (X.statuses sorts debuffs first, so they are never pushed out.)
  local me=frame_player
  if list and me and not row.pet and m.id==me.id then
    list={}
    key=tostring(key)..'|none'
  end
  local previous=R.groups[id..':statuses']
  -- Old icons may stay during an art/metadata refresh for the SAME membership,
  -- never after we have seen a removal, roster/layout change or lost snapshot.
  local function deferred()
    if key and previous and not previous.retry and previous.revision[1]==rev and previous.revision[2]==key then
      R:group(id..':statuses',previous.revision,function()end)
      local pt=R.groups[id..':status_times']
      if pt then R:group(id..':status_times',pt.revision,function()end) end
      return false,true
    end
    return false
  end
  if not list then
    return deferred()
  end
  probe.mark('row_status_specs')
  if (not previous or previous.retry or previous.revision[1]~=rev or previous.revision[3]~=status_rev)
      and #list>0 and not admit_row_leaf() then return deferred() end
  R:group(id..':statuses',{rev,key,status_rev},function()
    for i=1,math.min(count,#list) do
      local sx,sy,hpx=status_geom(row,i,x,y,u)
      R:status(id..'_buff'..i,list[i],sx,sy,hpx,sc,25,false,false)
    end
    if row.full and #list>count then R:text(id..'_more','+'..(#list-count),x+406*u,y+32*u,8,sc,26,nil,'right') end
  end)
  -- Duration numbers: 59, then 1m.. (floored) from 60 s, then 1h.. from 60 min (X.fmt_time). A separate group keyed by the
  -- label text, so icons are not rebuilt; it only rewrites when a displayed number actually changes.
  local labels={}
  for i=1,math.min(count,#list) do
    local e=list[i]
    labels[i]=(not e.untimed and X.fmt_time(e.left)) or ''
  end
  R:group(id..':status_times',{rev,key,status_rev,table.concat(labels,'\31')},function()
    for i=1,#labels do
      if labels[i]~='' then
        local sx,sy,hpx=status_geom(row,i,x,y,u)
        R:text(id..'_buff'..i..'_time',labels[i],sx+X.status_w(hpx)/2,sy+hpx-5*sc,8,sc,26,nil,'center',true)
      end
    end
  end)
  return true
end
-- Geometry/roster descriptors contain slots, never captured native member
-- snapshots. Every frame resolves their current values before row production.
local function stamp(...)
  local out={};for i=1,select('#',...) do out[i]=tostring(select(i,...)) end
  return table.concat(out,'\31')
end
local function prepare_layout(members,pets,s)
  local parts={tostring(preview)}
  for _,m in ipairs(members) do parts[#parts+1]=stamp(m.id,m.slot,m.party) end
  for i=1,math.min(6,#pets) do parts[#parts+1]='pet:'..tostring(pets[i].id) end
  local key=table.concat(parts,'\30')
  if key~=roster_key then
    roster_key=key;generation=generation+1;roster={{},{},{}};layout_key=nil
    local alive={}
    for _,m in ipairs(members) do
      alive[m.id]=true
      if m.party and m.party>=0 and m.party<=2 then
        roster[m.party+1][#roster[m.party+1]+1]=m.slot+1
      end
    end
    for _,rows in ipairs(roster) do table.sort(rows) end
    for _,e in ipairs(pets) do alive[e.id]=true end
    for id in pairs(smooth) do if not alive[id] then smooth[id]=nil end end
    for id in pairs(status_cache) do if not alive[id] then status_cache[id]=nil end end
  end
  local sc=(settings.get('scale') or 100)/100
  local u=s.ui_scale*sc
  local count=math.floor(X.clamp(settings.get('buff_count') or 6,1,32))
  key=stamp(s.width,s.height,s.ui_scale,sc,count,settings.get('offset_x'),settings.get('offset_y'),
    settings.get('single_alliance'),settings.get('align_bottom'))
  if key~=layout_key then
    layout_key=key;layout_revision=layout_revision+1
    local h=46+math.max(0,math.ceil((count-6)/20))*22
    local x=X.clamp((settings.get('offset_x') or 16)*u+24*u,11*u,math.max(11*u,s.width-410*u))
    local y=X.clamp((settings.get('offset_y') or 120)*u,15*u,math.max(15*u,s.height-h*u))
    layout={rows={},headings={},complete=not preview,u=u,sc=sc,h=h,count=count}
    for group,rows in ipairs(roster) do
      local full=group==1 or (group==2 and #roster[3]==0 and settings.get('single_alliance'))
      local panel_w=full and 410 or 220
      local px=group==1 and x or X.clamp(s.width-(panel_w+8)*u,0,math.max(0,s.width-panel_w*u))
      local py=group==3 and y+224*u or y
      if settings.get('align_bottom') then py=py+math.max(0,6-#rows)*(full and h or 34)*u end
      if #rows>0 then layout.headings[#layout.headings+1]={id='xpl_heading'..group,
        text=(preview and 'PREVIEW ' or '')..({'PARTY','ALLIANCE 1','ALLIANCE 2'})[group],x=px,y=py-15*u} end
      for i,slot in ipairs(rows) do
        local rh=full and h or 34;local ry=py+(i-1)*rh*u
        local fit=ry+rh*u<=s.height and px+panel_w*u<=s.width
        if not fit then layout.complete=false end
        layout.rows[#layout.rows+1]={slot=slot,x=px,y=ry,full=full,fit=fit}
      end
    end
    local py=y+(#roster[1]*h+22)*u
    if #pets>0 then layout.headings[#layout.headings+1]={id='xpl_pets_heading',text='PETS',x=x,y=py-15*u} end
    for i=1,math.min(6,#pets) do layout.rows[#layout.rows+1]={slot=18+i,pet=i,x=x,y=py+(i-1)*34*u,
      full=false,fit=py+i*34*u<=s.height} end
  end
  return layout
end
local casts_active
local next_stage,stage_at,entity_samples,base_prepared=1,{1,1,1,1,1},{},{}
local samples_generation
local function resolve_row(m,row,slot)
  if row.pet or preview then return true end
  if m.in_zone==false then
    entity_samples[slot]=nil
    return true
  end
  local old=entity_samples[slot]
  if old and old.id==m.id and old.index==m.index and clock()-old.at<.2 then return true end
  if not admit_row_leaf() then return false end
  probe.mark('row_entity')
  local e=game.entity(m.id)
  probe.mark('row_specs')
  entity_samples[slot]={id=m.id,index=m.index,entity=e,at=clock()}
  return true
end
local function apply_entity(m,row,slot)
  if row.pet or preview then return end
  local me=frame_player
  if me and m.id==me.id and me.mp_max~=nil then m.has_mp=me.mp_max>0 end
  if m.in_zone==false then m.distance=nil;return end
  local sample,e=entity_samples[slot]
  if sample and sample.id==m.id and sample.index==m.index then
    if clock()-sample.at<.5 then e=sample.entity end
  else prep_pending=true end
  if e and e.trust and (not m.main_job or m.main_job==0) then
    local candidate,ambiguous
    for _,entry in ipairs(trust_index[(m.name or ''):lower()] or {}) do
      if entry.model and entry.model==e.model then candidate,ambiguous=entry.job,false;break end
      if not candidate then candidate=entry.job elseif candidate~=entry.job then ambiguous=true end
    end
    if candidate and not ambiguous and job_ids[candidate] then m.main_job,m.inferred_job=job_ids[candidate],true end
  end
  -- Keep the previously validated snapshot for roster coverage, not as an
  -- indefinitely fresh range display. A slow round-robin degrades to unknown
  -- distance without holding the native party fallback forever.
  m.distance=e and e.distance or nil
end
local function prepare_self_buffs(rows)
  local me=frame_player
  local m
  if me then
    for _,row in ipairs(rows) do
      local member=current[row.slot]
      if row.fit and not row.pet and member and member.id==me.id then m=member;break end
    end
  end
  -- Do not sample a player who has no visible main row (or is in preview).
  if not m or m.id~=me.id or m.in_zone==false then self_buffs=nil;return true end
  if admit_row_leaf() then
    probe.mark('row_buffs')
    local list=sample_buffs()
    self_buffs={id=me.id,list=list,at=clock()}
    probe.mark('row_specs')
    return true
  end
  if self_buffs and (self_buffs.id~=me.id or clock()-self_buffs.at>=.5) then self_buffs=nil end
  return false
end
local last_draw
local function draw()
  -- Retained HUD: an idle, fully built renderer needs refreshing at ~35 Hz, not
  -- every frame. Building, recovery and pending work are never throttled.
  local now=clock()
  local frame_started=R:enter_frame('party')
  probe.begin(frame_started)
  prep_at,prep_steps,cold_steps,prep_pending,prep_exhausted=frame_started,0,0,false,false
  frame_player=nil
  frame_casts=nil;frame_cast_ids={}
  current={}
  local members,hide_native=visible_members()
  local live=members~=nil and not preview
  if live and #members>0 then probe.visible() end
  if casts_active~=live then invalidate_casts();casts_active=live end
  demand_pets(live and #members>0 and settings.get('pets')~=false)
  if not members then
    self_buffs=nil
    if presenting then generation=generation+1;roster_key=nil;presenting=false end
    R:begin(generation);R.work_at=frame_started
    probe.mark('drain');R:finish();probe.mark('native');X.native('party',hide_native)
    R:leave_frame();probe.done(false,false,false);return
  end
  presenting=true
  if preview then members = {
    { id = -1, slot = 0, party = 0, name = "PREVIEW — RDM", main_job = 5, sub_job = 4, hp = 3122, mp = 454, tp = 1250, hpp = 92, mpp = 63, buffs = { 3, 40 } },
    { id = -2, slot = 6, party = 1, name = "PREVIEW — WAR", main_job = 1, hpp = 73, buffs = {} },
    { id = -3, slot = 12, party = 2, name = "PREVIEW — WHM", main_job = 3, hpp = 100, buffs = {} },
  } end
  probe.mark('snapshots')
  frame_player=not preview and game.player() or nil
  local target=not preview and game.target() or nil
  -- Resolve discovered pets once, using the same snapshots/visibility rules as
  -- their rows. Reuse this list below instead of sending stale discovery IDs or
  -- taking another entity snapshot just to filter the cast request.
  probe.mark('pet_resolve')
  local pets={}
  if settings.get("pets")~=false and not preview then
    local owners={};for _,m in ipairs(members) do if m.index and m.index>0 then owners[m.index]=m.name end end
    for _,id in ipairs(pet_ids) do
      local e=game.entity(id)
      if e and not e.hidden and not e.trust and e.distance and e.distance<=50 and
          (e.pet or (e.master and owners[e.master])) then pets[#pets+1]=e end
    end
    table.sort(pets,function(a,b)return a.id<b.id end)
  end
  local cast_seen={}
  local function request_cast(id)
    if cast_id(id) and not cast_seen[id] and #frame_cast_ids<24 then
      cast_seen[id]=true;frame_cast_ids[#frame_cast_ids+1]=id
    end
  end
  for _,m in ipairs(members) do request_cast(m.id) end
  for i=1,math.min(6,#pets) do request_cast(pets[i].id) end
  probe.mark('cast_batch');prepare_casts()
  probe.mark('layout')
  local L=prepare_layout(members,pets,hud.screen());local u,sc=L.u,L.sc
  if samples_generation~=generation then
    samples_generation=generation;next_stage,stage_at,entity_samples,base_prepared=1,{1,1,1,1,1},{},{}
  end
  R.move_units=u;R:begin(generation)
  -- Preparation belongs to this draw's allowance too. Keep one-operation
  -- progress when already over budget; never restart the clock before draining.
  R.work_at=frame_started
  R:group('headings',layout_revision,function()
    for _,heading in ipairs(L.headings) do R:text(heading.id,heading.text,heading.x,heading.y,10,sc,10) end
  end)
  for _,m in ipairs(members) do if m.slot~=nil then current[m.slot+1]=m end end
  for i=1,math.min(6,#pets) do
    local e=pets[i];current[18+i]={id=e.id,name=e.name,hpp=e.hpp,mpp=e.mpp,tp=e.tp,in_zone=true,buffs={}}
  end
  -- Keep the original per-visible-draw recurrence even if a cold base group
  -- has to wait. Advance only once, independently of the stage rotation.
  for _,row in ipairs(L.rows) do
    local m=current[row.slot]
    if row.fit and row.full and m then
      local fraction=X.clamp(m.hpp,0,100)/100
      local old=smooth[m.id] or fraction
      smooth[m.id]=fraction==0 and 0 or old+(fraction-old)*.35
    end
  end
  -- Slow leaves, cold base rows and cold status specs share one elapsed gate.
  -- An independent self-buff stage means a slow read cannot prevent its own
  -- metadata/producer from ever being served on a fresh slice.
  local n=#L.rows
  local first_deferred
  for step=0,4 do
    local stage=(next_stage+step-1)%5+1
    local first
    if stage==2 then
      if not prepare_self_buffs(L.rows) then first=1 end
    else
      for offset=0,n-1 do
        local i=(stage_at[stage]+offset-1)%n+1
        local row=L.rows[i]
        local m=current[row.slot]
        if m and row.fit then
          probe.mark('row_specs')
          local done
          local id,rev='xpl'..row.slot,layout_revision
          if stage==1 then done=resolve_row(m,row,row.slot)
          elseif stage==3 then
            done=draw_cast(id,m,row,u,sc,rev)
          elseif stage==4 then
            local retained
            done,retained=draw_statuses(id,m,row,L,rev)
            if not done and not retained then prep_pending=true end
          else
            local cold=base_prepared[row.slot]~=rev
            -- Reconciliation of already prepared live fields is small and
            -- immediate. Bound each new row's retained production separately.
            done=not cold or admit_row_leaf()
            if done then
              probe.mark('row_base')
              apply_entity(m,row,row.slot)
              if row.full then main_row(row.slot,m,row.x,row.y,u,sc,L.h,target and target.id==m.id,rev)
              else compact_row(row.slot,m,row.x,row.y,u,sc,target and target.id==m.id,rev) end
              base_prepared[row.slot]=rev
            else prep_pending=true end
          end
          if not done then first=first or i end
        end
      end
    end
    stage_at[stage]=first or 1
    if first and not first_deferred then first_deferred=stage end
  end
  if first_deferred then next_stage=next_stage%5+1 else next_stage=1 end
  -- Only hide aggregate native party once ALL party/alliance rows fit and exist.
  probe.mark('drain')
  R:finish()
  if prep_pending then R.incomplete=true end
  probe.mark('native');X.native("party", L.complete and #members>0)
  frame_player=nil
  R:leave_frame();probe.done(first_deferred~=nil,live and #members>0,L.complete and R:ready())
end
events.on("frame", draw)
timer.every(0.25, detail_refresh)
commands.register("xpl", function(arg)
  arg = tostring(arg or ""):lower()
  if arg == 'probe' then probe.manual();return
  elseif arg == "details" then open_details()
  elseif arg == "preview" then preview = not preview
  elseif arg == "on" then enabled = true
  elseif arg == "off" then enabled = false
  elseif arg == "status" then
    chat.add(("XIV Party List: %s (panel %s)"):format(
      enabled and "on" or "off", settings.get("on") and "on" or "off"))
  elseif arg == "" then enabled = not enabled
  else chat.add("Usage: //xpl [on|off|details|preview|status|probe]") end
  if arg=="preview" or not enabled then invalidate_casts();demand_pets(false);current={};self_buffs=nil end
  if not enabled then preview = false; R:hide(); ui.close(WIN); X.native("party", false) end
end)
