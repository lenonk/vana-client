-- Suite-wide HUD primitives (injected as src.xiv per panel). No cross-mod file reads.
local X = { version = 7 }
X.text_colour = { 240, 255, 255, 255 }
X.stroke = { 6, 45, 84, 210 }
local roles = { war = "tank", pld = "tank", run = "tank", pup = "tank", whm = "healer", sch = "healer",
  cor = "support", geo = "support" }
local colours = { tank = { 55, 100, 200, 255 }, healer = { 50, 150, 65, 255 },
  support = { 50, 150, 65, 255 }, dps = { 185, 55, 50, 255 } }
function X.role(code) return code and colours[roles[code] or "dps"] or { 128, 128, 128, 255 } end
function X.clamp(v, lo, hi) return math.max(lo, math.min(hi, tonumber(v) or lo)) end
-- Compact durations like the modern UI: 59, 1m, 1h rather than raw seconds past a minute.
function X.fmt_time(seconds)
  local n = tonumber(seconds)
  if not n or n < 0 then return nil end
  if n >= 3600 then return math.floor(n / 3600) .. "h"
  elseif n >= 60 then return math.floor(n / 60) .. "m"
  else return tostring(math.floor(n)) end
end
function X.id(v) return type(v) == "number" and v >= 0 and v < 65535 and v % 1 == 0 end
local function hud_config_active()
  local req = X._module_require
  if type(req) == "function" then
    local m = req("src.hud_config")
    if type(m) == "table" and type(m.active) == "function" then return m.active() end
  end
  local ok, m = pcall(require, "src.hud_config")
  return ok and type(m) == "table" and type(m.active) == "function" and m.active()
end
function X.target(id)
  if hud_config_active() then return false end
  if type(id)~='number' or id<=0 or id>4294967295 or id%1~=0 then return false end
  local entity=game.entity(id)
  if not entity or entity.id~=id or entity.hidden or entity.targetable==false then return false end
  local ok=pcall(actions.target,id)
  if not ok and not X.target_warning then chat.add('XIV HUD: targeting unavailable; review this mod actions permission.');X.target_warning=true end
  return ok
end
function X.short(s, n)
  s = tostring(s or "")
  local ok, at = pcall(utf8.offset, s, n + 1)
  return ok and at and s:sub(1, at - 1) .. "..." or s
end
function X.call(mod, method, ...)
  if not mods or not mods.call then return nil end
  local ok, value = pcall(mods.call, mod, method, ...)
  return ok and value or nil
end
local files = {}
local warnings = {}
local native_state={}
for _,event in ipairs({'zone_change','settings_changed'}) do events.on(event,function()native_state={} end) end
local stall_since = {}
function X.native(piece, hidden, force)
  -- Do not show the native piece just because a frame has not finished yet: that flashed the
  -- native window for a frame when a new target appeared. Only fall back after 500 ms of stall.
  if hidden and X.active_renderer and X.active_renderer.incomplete then
    local now = clock()
    local since = stall_since[piece]
    if not since then stall_since[piece] = now; since = now end
    if now - since >= 0.5 then hidden = false end
  else
    stall_since[piece] = nil
  end
  local old=native_state[piece]
  if not force and old and old.hidden==hidden and clock()-old.at<1 then return old.ok end
  local ok = pcall(function() ui.native.hide(piece, hidden) end)
  native_state[piece]={hidden=hidden,at=clock(),ok=ok}
  if ok then warnings[piece] = nil
  elseif not warnings[piece] then
    chat.add("XIV HUD: native " .. piece .. " hide unavailable; check ui.native permission/client support.")
    warnings[piece] = true
  end
  return ok
end
function X.exists(path)
  if files[path] == nil then files[path] = fs.exists(path) end
  return files[path]
end
local function same(a, b)
  if a == b then return true end
  if type(a) ~= type(b) then return false end
  if type(a) ~= "table" then return a == b end
  for k, v in pairs(a) do if not same(v, b[k]) then return false end end
  for k in pairs(b) do if a[k] == nil then return false end end
  return true
end
local function copy(a)
  if type(a) ~= "table" then return a end
  local b = {}; for k, v in pairs(a) do b[k] = copy(v) end; return b
end
local R = {}; R.__index = R
function X.renderer()
  return setmetatable({ state = {}, desired = {}, active = {}, seen = {}, groups = {}, handlers = {},
    kinds = {}, queue = {}, head = 1, tail = 0, waiting = {}, pending = 0, uncommitted = {},
    counts = { groups = 0, specs = 0, creates = 0, updates = 0, hides = 0, handlers = 0, failures = 0 } }, R)
end
-- Draw callbacks do not yield. An unfinished caller is recovered BEFORE the
-- next draw's preparation, on its fresh deadline, not inside an expired frame.
function R:enter_frame(piece)
  local unfinished=self.callback_open
  self.callback_open=true;X.active_renderer=self
  local at=clock()
  if unfinished then
    self.incomplete=true
    -- Name the owned piece even if settings/zone reset the cache. Force the
    -- reveal: a hide side effect may have escaped before its cache commit.
    X.native(piece,false,true)
  end
  return at
end
function R:leave_frame() self.callback_open=nil end
function R:enqueue(id)
  if self.waiting[id] then return end
  -- An abort can interrupt ANY part of the append. On a fresh begin, finish
  -- this one authoritative ID instead of leaving a hole or duplicate marker.
  self.incomplete=true
  self.enqueuing={id=id,tail=self.tail,pending=self.pending}
  self.tail=self.tail+1;self.queue[self.tail]=id;self.waiting[id]=true;self.pending=self.pending+1
  self.enqueuing=nil
end
-- Optional generation is a scalar token. Replace it on target/zone/roster changes.
-- Committed native state survives; queued operations contain IDs, NEVER old specs.
function R:begin(generation)
  -- A producer may have escaped its pcall on the previous callback. Its
  -- unpublished group is abandoned; a fresh frame must be allowed to retry.
  self.building=nil
  local appending=self.enqueuing
  if appending then
    self.queue[appending.tail+1]=appending.id;self.tail=appending.tail+1
    self.waiting[appending.id]=true;self.pending=appending.pending+1
    self.enqueuing=nil
  end
  -- A callback can escape between a native operation and its FIFO commit. Roll
  -- that turn back on a FRESH frame, retaining one authoritative pending ID.
  local interrupted=self.draining
  if interrupted then
    if interrupted.id then
      interrupted.queue[interrupted.head]=interrupted.id
      interrupted.queue[interrupted.tail+1]=nil
      self.waiting[interrupted.id]=true
    end
    self.queue=interrupted.queue;self.head=interrupted.head;self.tail=interrupted.tail;self.pending=interrupted.pending
    self.draining=nil
  end
  if generation ~= nil and self.generation ~= generation then
    self.generation=generation;self.groups={};self.desired={}
    -- Keep the FIFO of IDs, including inactive IDs whose committed hides have
    -- not succeeded. Clearing it loses cleanup on successive generations and
    -- resetting its order can starve cleanup during continuous target churn.
    -- drain_one resolves ONLY current desired state after membership pruning;
    -- an old queued ID therefore cannot recreate an obsolete specification.
    for id in pairs(self.active) do self:enqueue(id) end
  end
  self.seen={};self.work_at=clock();self.calls=0;self.heavy=0;self.incomplete=self.pending>0;self.in_frame=true;X.active_renderer=self
end
-- Use between begin/finish. A producer owns disjoint IDs, runs only when its
-- revision changes, and may use the ordinary image/text/status/grip methods:
-- R:group('identity', {layout_revision, name}, function(r) r:text(...) end).
-- Revisions are copied plain data; omission hides a group, reappearance restores
-- it without production. finish ALWAYS drains even when no producer ran.
function R:group(key, revision, producer)
  assert(not self.building, 'Renderer groups cannot nest')
  local group=self.groups[key]
  if not group or group.retry or not same(group.revision,revision) then
    self.incomplete=true
    -- Keep completed membership until success, but force fresh production if
    -- an interrupted replacement staged desired values and revision reverts.
    if group then group.retry=true end
    group={revision=copy(revision),ids={}};self.building=group
    self.counts.groups=self.counts.groups+1
    local ok,err=pcall(producer,self);self.building=nil
    if not ok then error(err,0) end
    self.groups[key]=group
  end
  for id in pairs(group.ids) do
    self.seen[id]=true
    if not self.active[id] then
      self.incomplete=true;self:enqueue(id);self.active[id]=true
    end
  end
end
function R:allow(id)
  if (self.calls or 0)>=16 or ((self.calls or 0)>0 and self.work_at and clock()-self.work_at>=0.006) then
    self.incomplete=true;return false
  end
  self.calls=(self.calls or 0)+1;return true
end
function R:create(id) return self:allow(id) end
function R:ready() return not self.incomplete and not self.callback_open end
function R:put(kind, id, spec)
  self.seen[id]=true;self.active[id]=true
  if self.building then self.building.ids[id]=true end
  spec.visible=spec.visible~=false
  self.desired[id]={kind=kind,spec=copy(spec)}
  self.counts.specs=self.counts.specs+1
  -- Legacy direct image/status helpers may omit begin/finish; keep those immediate;
  -- retained frames stage until finish.
  if self.in_frame then self:enqueue(id)
  elseif not self:drain_one(id) then self:enqueue(id) end
end
-- One native operation per turn, then requeue at the tail if a handler still
-- needs binding. A frequently changing first element cannot starve cold rows.
function R:drain_one(id)
  local want=self.active[id] and self.desired[id]
  local old=self.state[id]
  local uncertain=self.uncommitted[id]
  local kind,spec,operation
  if not want then
    if uncertain then
      -- The native side effect may have happened even without a Lua commit.
      -- remove is idempotent for absent IDs; set would fail forever if the
      -- interrupted create never ran. Never replay a retired specification.
      operation='hides';kind='remove'
    else
      if not old or not old.visible then return true end
      operation='hides';spec={visible=false};kind='set'
    end
  elseif uncertain or not old or self.kinds[id]~=want.kind then
    operation='creates';spec=want.spec;kind=want.kind
  else
    spec={}
    for k,v in pairs(want.spec) do if not same(v,old[k]) then spec[k]=v end end
    -- PNG replacement adopts native dimensions unless BOTH are reasserted.
    if want.kind=='image' and old.path~=want.spec.path and want.spec.w and want.spec.h then
      spec.w,spec.h=want.spec.w,want.spec.h
    end
    if next(spec) then operation='updates';kind='set'
    elseif want.handler and not self.handlers[id] then operation='handlers'
    else return true end
  end
  -- Creating an image element is the costly native call on slow hosts (a PNG
  -- load and GPU upload, ~1-2 ms each). Admit at most ONE slow creation per
  -- draw; cheap creations and updates still flow up to the normal allowance.
  if operation=='creates' and kind=='image' and (self.heavy or 0)>=1 then return false,'budget' end
  if not self:allow(id) then return false,'budget' end
  local t0=clock()
  -- Leave this marker until ALL native-state bookkeeping commits. Recovery
  -- recreates current desired content, or removes an uncertain retired ID.
  self.uncommitted[id]=true
  local ok
  if operation=='handlers' then
    ok=pcall(hud.on,id,function(element,e)
      local current=self.active[id] and self.desired[id]
      if self.callback_open or self.incomplete or not current or current.spec.visible==false or not current.handler then return end
      if self.drag and self.drag(e) then return end
      return current.handler(element,e)
    end)
    if ok then self.handlers[id]=true end
  else
    if kind=='remove' then ok=pcall(hud.remove,id)
    else ok=pcall(hud[kind],id,copy(spec)) end
    if ok then
      if kind=='remove' then
        self.state[id]=nil;self.kinds[id]=nil;self.handlers[id]=nil
      elseif operation=='creates' then
        self.state[id]=copy(spec);self.kinds[id]=want.kind;self.handlers[id]=nil
      else for k,v in pairs(spec) do old[k]=copy(v) end end
    end
  end
  if operation=='creates' and kind=='image' and clock()-t0>=.0006 then self.heavy=(self.heavy or 0)+1 end
  if not ok then self.uncommitted[id]=uncertain;self.counts.failures=self.counts.failures+1;return false end
  self.uncommitted[id]=nil
  self.counts[operation]=self.counts[operation]+1
  if want and want.handler and not self.handlers[id] then return false end
  return true
end
function R:finish()
  -- Only current membership is scanned, not all historically retained specs.
  for id in pairs(self.active) do
    if not self.seen[id] then self:enqueue(id);self.active[id]=nil end
  end
  while self.head<=self.tail do
    local id=self.queue[self.head]
    self.draining={queue=self.queue,head=self.head,tail=self.tail,pending=self.pending,id=id}
    local done,why=self:drain_one(id)
    if why=='budget' then
      -- An operation not attempted keeps its place. Moving it behind a failing
      -- predecessor every slice can otherwise starve it forever on slow HUDs.
      self.draining=nil
      break
    end
    -- Commit only after drain returns. Failed operations rotate with the same
    -- pending marker; an escaping commit can be rolled back without duplicates.
    self.queue[self.head]=nil;self.head=self.head+1
    if done then self.waiting[id]=nil;self.pending=self.pending-1
    else self.tail=self.tail+1;self.queue[self.tail]=id end
    self.draining=nil
  end
  if self.tail>0 and (self.head>self.tail or (self.head>128 and self.head>self.tail-self.head+1)) then
    self.draining={queue=self.queue,head=self.head,tail=self.tail,pending=self.pending}
    if self.head>self.tail then self.queue={};self.head=1;self.tail=0
    else
      local compact={};for i=self.head,self.tail do compact[#compact+1]=self.queue[i] end
      self.queue=compact;self.head=1;self.tail=#compact
    end
    self.draining=nil
  end
  self.incomplete=self.pending>0;self.in_frame=false
  if self.incomplete then
    for piece,entry in pairs(native_state) do if entry.hidden then X.native(piece,false) end end
  end
end
function R:stats()
  local out=copy(self.counts);out.pending=self.pending;out.calls=self.calls or 0;out.active=0
  for _ in pairs(self.active) do out.active=out.active+1 end
  return out
end
function R:hide() self:begin(); self:finish() end
function R:image(id, path, x, y, w, h, layer, colour, tip)
  self:put("image", id, { path = path, x = x, y = y, w = math.max(1, w), h = math.max(1, h),
    layer = layer, colour = colour or false, tip = tip or false })
end
function R:rect(id, x, y, w, h, layer, colour, opts)
  opts = opts or {}
  local spec = { x = x, y = y, w = math.max(1, w), h = math.max(1, h), layer = layer,
    colour = colour or { 0, 0, 0, 160 }, round = opts.round or 0,
    tip = opts.tip or false }
  if opts.line ~= nil then spec.line = opts.line end
  self:put("rect", id, spec)
end
function R:bar(id, x, y, w, h, layer, value, kind_or_colour, tip)
  local spec = { x = x, y = y, w = math.max(1, w), h = math.max(1, h), layer = layer,
    value = X.clamp(tonumber(value) or 0, 0, 1), tip = tip or false }
  if type(kind_or_colour) == "table" then spec.colour = kind_or_colour
  elseif kind_or_colour then spec.kind = kind_or_colour end
  self:put("bar", id, spec)
end
function R:text(id, text, x, y, px, scale, layer, colour, align, bold)
  local size = px / 14 * scale
  self:put("text", id, { text = tostring(text or ""), x = x, y = y, size = size,
    font = bold and "bold" or "regular", colour = colour or X.text_colour, align = align or "left",
    outline = X.stroke, outline_width = scale / math.max(1, size), layer = layer })
end
function R:grip(id, x, y, w, h, layer, handler, tip)
  self:put("rect", id, { x = x, y = y, w = w, h = h, colour = { 0, 0, 0, 1 },
    layer = layer, tip = tip or false })
  self.desired[id].handler=handler
  if not self.in_frame and not self:drain_one(id) then self:enqueue(id) end
end
local replacement = { poison = "poison", paralysis = "paralyze", paralyze = "paralyze", blindness = "blind",
  blind = "blind", silence = "silence", mute = "silence", slow = "slow", bind = "bind", weight = "heavy",
  sleep = "sleep", petrification = "petrify", stun = "stun", doom = "doom", curse = "doom", amnesia = "amnesia",
  addle = "addle", dia = "dia", bio = "miasma", plague = "miasma", disease = "disease", terror = "petrify",
  charm = "hysteria", burn = "burn", frost = "frostbite", choke = "windburn", rasp = "sludge",
  shock = "electrocution", drown = "dropsy", ["accuracy down"] = "accuracy_down",
  ["defense down"] = "damage_down", ["magic defense down"] = "mag_vuln", weakness = "weakness" }
local status_names,status_info={},{}
function X.status_name(id)
  local value=status_names[id]
  if not value then value=res.status(id);if value then status_names[id]=value end end
  return value
end
function X.status_info(id)
  local value=status_info[id]
  if not value and res.status_info then value=res.status_info(id);if value then status_info[id]=value end end
  return value
end
function X.debuff(id, name)
  return id <= 20 or replacement[tostring(name):lower():gsub("%s+%d+$", "")] ~= nil
    or tostring(name):lower():find("down$", 1) ~= nil or tostring(name):lower():find("daze", 1, true) ~= nil
end
function X.statuses(list, hidden, priority)
  if type(list) ~= "table" then list = {} end
  if type(hidden) ~= "table" then hidden = nil end
  if type(priority) ~= "table" then priority = {} end
  local out, seen, rank = {}, {}, {}
  for i, id in ipairs(priority or {}) do if X.id(tonumber(id)) then rank[tonumber(id)] = i end end
  for _, e in ipairs(list or {}) do
    if type(e) == "number" then e = { id = e } end
    if type(e) ~= "table" then e = {} end
    local id = e.id or e.status
    if X.id(id) and not seen[id] and not (hidden and hidden[tostring(id)]) then
      local left = e.left or e.remaining
      if left == nil or left > 0 then
        local status_name = X.status_name(id) or ("Status " .. id)
        local name = e.name or status_name
        local info = X.status_info(id)
        out[#out + 1] = { id = id, name = name, status_name = status_name, left = left, total = e.secs or e.total,
          untimed = info and info.untimed or false,
          debuff = X.debuff(id, status_name), source = e.source or (left and "Tracked duration (estimated)" or "Observed; duration unknown") }
        seen[id] = true
      end
    end
  end
  table.sort(out, function(a, b)
    local ar, br = rank[a.id] or 999, rank[b.id] or 999
    if ar ~= br then return ar < br end
    if a.debuff ~= b.debuff then return a.debuff end
    return a.id < b.id
  end)
  return out
end
function X.status_w(size) return size end
function R:status(id, row, x, y, size, scale, layer, enemy, show_time)
  local info = X.status_info(row.id)
  local tip = { title = row.name, text = (info and info.description or "") .. "\n" .. row.source }
  local sw = X.status_w(size)
  self:put("icon", id .. "_native", { kind = "status", id = row.id, x = x, y = y, size = sw, layer = layer, tip = tip })
  local label = show_time~=false and not row.untimed and X.fmt_time(row.left)
  if label then self:text(id .. "_time", label, x + sw / 2, y + size - 5 * scale,
    8, scale, layer + 1, nil, "center", true) end
end
return X
