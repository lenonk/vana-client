-- Read-only counter model adapted from bundled mod `xp-tracker`
-- (`crates/vana-client/assets/mods/xp-tracker/init.lua`, same repo / MIT).
-- Same EXP/merit/JP rollover equations; monotonic time and per-job reset.
local M={};local Tracker={};Tracker.__index=Tracker
local function valid(n)return type(n)=='number' and n==n and n>=0 and n<math.huge end
function M.new()local self=setmetatable({},Tracker);self:reset();return self end
function Tracker:reset(reading)
  self.last=nil
  if reading then self.last={};for key,value in pairs(reading) do self.last[key]=value end end
  self.gains={exp={total=0},limit={total=0},capacity={total=0}}
end
function Tracker:add(kind,n,now)
  if not valid(n) or n<=0 then return end
  local gain=self.gains[kind];gain.since=gain.since or now;gain.total=gain.total+n
end
function Tracker:sample(b,now)
  if type(b)~='table' or not valid(b.job) then return end
  local a=self.last
  if a and a.job~=b.job then self:reset();a=nil end
  if a then
    if valid(a.exp) and valid(b.exp) and valid(a.level) and valid(b.level) then
      if b.level==a.level then self:add('exp',b.exp-a.exp,now)
      elseif b.level==a.level+1 and valid(a.exp_next) then self:add('exp',a.exp_next-a.exp+b.exp,now)
      elseif b.level>a.level+1 then self.gains.exp.incomplete=true end
    end
    if valid(a.limit) and valid(b.limit) and valid(a.merits) and valid(b.merits) then
      self:add('limit',(b.merits-a.merits)*10000+b.limit-a.limit,now)
      if b.merits<a.merits then self.gains.limit.incomplete=true end
    end
    if valid(a.capacity) and valid(b.capacity) and valid(a.jp) and valid(b.jp) and
        valid(a.jp_spent) and valid(b.jp_spent) then
      if b.jp_spent>=a.jp_spent then
        self:add('capacity',((b.jp+b.jp_spent)-(a.jp+a.jp_spent))*30000+b.capacity-a.capacity,now)
      else self.gains.capacity.incomplete=true end
    end
  end
  local saved={};for key,value in pairs(b) do saved[key]=value end;self.last=saved
end
function M.select(r,choice)
  if type(r)~='table' then return nil end
  if choice=='auto' or choice==nil then
    if (r.level or 0)>=99 and valid(r.capacity) and valid(r.jp) then choice='capacity'
    elseif (r.level or 0)>=75 and valid(r.limit) and valid(r.merits) then choice='limit'
    else choice='exp' end
  end
  if choice=='exp' and valid(r.exp) and valid(r.exp_next) and r.exp_next>0 then
    return {kind='exp',label='EXP',current=r.exp,total=r.exp_next}
  elseif choice=='limit' and (r.level or 0)>=75 and valid(r.limit) and valid(r.merits) then
    return {kind='limit',label='LIMIT',current=r.limit,total=10000,merits=r.merits,merit_max=r.merit_max}
  elseif choice=='capacity' and (r.level or 0)>=99 and valid(r.capacity) and valid(r.jp) then
    return {kind='capacity',label='CAPACITY',current=r.capacity,total=30000,points=r.jp,spent=r.jp_spent}
  end
  return nil
end
function Tracker:detail(row,now)
  if not row then return nil end
  local result={};for key,value in pairs(row) do result[key]=value end
  local gain=self.gains[row.kind];result.observed_gain=gain.total;result.incomplete=gain.incomplete or false
  local elapsed=gain.since and now-gain.since or 0
  if elapsed>=60 then
    result.rate=gain.total*3600/elapsed
    if result.rate>0 and not result.incomplete then result.eta=math.max(0,row.total-row.current)/result.rate*3600 end
  end
  return result
end
return M
