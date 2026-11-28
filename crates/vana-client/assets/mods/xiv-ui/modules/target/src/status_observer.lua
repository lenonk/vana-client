-- Distinguish status-valued results from damage/roll/ability-valued results.
-- Landing message ids align with vana-client mod target-debuffs (assets/mods/target-debuffs/init.lua).
local data=require('src.status_map')
local M={}
local direct={}
for _,id in ipairs({127,141,186,230,236,237,242,243,266,267,268,271,277,278,319,320}) do direct[id]=true end
local ws_messages={[185]=true,[188]=true,[189]=true,[264]=true,[282]=true,[283]=true}
local function lands(row,r)
  if not row.landed[r.message] then return false end
  local value=r.value or 0
  if row.param=='E' then return value==row.effect end
  if row.param=='D' and row.formula=='HELIX' then return value>0 end
  if row.param=='R' then return value>=1 and value<=11 end
  if row.param=='S' or row.param=='G' then return value>0 end
  if row.param=='V' then return value==531 end
  return true
end
function M.effects(a,t,r)
  if a.category~=1 and a.category~=2 and a.category~=3 and a.category~=4 and
      a.category~=6 and a.category~=11 and a.category~=13 then return {} end
  local kind=a.category==4 and 'spell' or a.category==6 and 'ability' or
    (a.category==11 or a.category==13) and 'pet' or
    a.category==3 and (ws_messages[r.message] and 'ws' or 'ability')
  local rows=kind and data.actions[kind] and data.actions[kind][a.param]
  local out,seen={},{}
  local function add(id)
    if type(id)=='number' and id>0 and id<65535 and id%1==0 and not seen[id] then
      seen[id]=true;out[#out+1]=id
    end
  end
  if rows then
    for _,row in ipairs(rows) do
      if (row.target~='self' or t.id==a.actor) and lands(row,r) then add(row.effect) end
    end
  elseif direct[r.message] then add(r.value) end
  -- Additional effects carry their own value, not the main damage result.
  if r.proc and (r.proc.message==160 or r.proc.message==164) then add(r.proc.value) end
  return out
end
function M.ends(id) return data.ends[id] or {} end
return M
