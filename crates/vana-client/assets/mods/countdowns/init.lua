-- Countdowns in the Timers windows (the timer add-ons): //timer <time> <name>. The resting tick is the client's.

local mine = {}       -- name -> timer id of its "up" line

-- "90", "5m", "1h30m", "2:30", "1:02:03" -> seconds.
local function seconds(s)
  s = s:lower()
  if s:find(":", 1, true) then
    local n = 0
    for part in s:gmatch("[^:]+") do
      if not tonumber(part) then return nil end
      n = n * 60 + tonumber(part)
    end
    return n
  end
  if tonumber(s) then return tonumber(s) end
  local n, any = 0, false
  for v, u in s:gmatch("(%d+)([hms])") do
    n = n + tonumber(v) * ({ h = 3600, m = 60, s = 1 })[u]
    any = true
  end
  return any and n or nil
end

local function cancel(name)
  if mine[name] then timer.cancel(mine[name]) end
  mine[name] = nil
  timers.remove(name)
end

commands.register("timer", function(first, ...)
  if not settings.get("on") then return end
  local rest = table.concat({ ... }, " ")
  local cmd = (first or ""):lower()
  if cmd == "clear" then
    for name in pairs(mine) do cancel(name) end
  elseif cmd == "cancel" and rest ~= "" then
    cancel(rest)
  else
    local secs = first and seconds(first)
    if not secs or secs <= 0 or rest == "" then
      chat.add("Countdowns: //timer <time> <name> (90, 5m, 1h30m or 2:30), //timer cancel <name>, //timer clear")
      return
    end
    cancel(rest)
    timers.add(rest, secs, { total = secs })
    mine[rest] = timer.after(secs, function()
      mine[rest] = nil
      chat.add("Countdowns: " .. rest .. " is up.")
    end)
  end
end)
