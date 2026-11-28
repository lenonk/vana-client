-- Pure real-value normalization (vana-client player vitals; TP ready at 1000 / cap 3000 like player-bars).
local M={}
local function number(v)return type(v)=='number' and v==v and v>=0 and v<math.huge and v or nil end
function M.read(p)
  if type(p)~='table' then return nil end
  local hp,max=number(p.hp),number(p.hp_max)
  if hp==nil or max==nil or max<=0 then return nil end
  local mp,mpmax,tp=number(p.mp),number(p.mp_max),number(p.tp)
  local function ratio(cur,total)return cur and total and total>0 and math.min(1,cur/total) or 0 end
  return {hp={current=hp,total=max,fraction=ratio(hp,max)},
    mp={current=mp,total=mpmax,fraction=ratio(mp,mpmax)},
    tp={current=tp,total=3000,fraction=ratio(tp,1000),ready=tp~=nil and tp>=1000},
    mp_less=mpmax==0,job=p.main_job,level=p.main_level,sub_job=p.sub_job,sub_level=p.sub_level}
end
function M.smooth(old,new,dt)
  if new==0 then return 0 end
  if old==nil or math.abs(old-new)<0.002 then return new end
  dt=math.max(0,math.min(0.1,tonumber(dt) or 0.016))
  return old+(new-old)*(1-math.exp(-dt*12))
end
return M
