-- NPC errands (the menu NPC and job change add-ons): //npc walks an NPC's menus by their rows' words, as the player
-- would pick them; //errand keeps such walks by name; //job changes jobs as the moogle's window does.

local function say(t) chat.add("NPC Errands: " .. t) end

local function norm(s)
  return (s:lower():gsub("[^%w#%[%] ]", ""):gsub("%s+", " "):match("^%s*(.-)%s*$"))
end

local function sig(m) return m.question .. "\n" .. table.concat(m.options, "\n") end

-- The next menu that is not the one just answered; a page waiting for a key goes on (10 s at most).
local function next_menu(last)
  local started = false
  for _ = 1, 100 do
    sleep(0.1)
    local e = game.event()
    if e then
      started = true
      if e.menu and sig(e.menu) ~= last then return e.menu end
      if not e.menu and e.text and e.text.waiting then actions.advance() end
    elseif started then
      return nil, "closed"
    end
  end
  return nil, "no answer"
end

local function nearest(name)
  local want, best = name:lower(), nil
  for _, e in ipairs(game.entities({ kinds = { "npc" } })) do
    if e.name:lower() == want and not e.hidden and (not best or e.distance < best.distance) then best = e end
  end
  return best
end

-- "<npc>: a > b > c"
local function walk(line)
  local who, path = line:match("^%s*(.-)%s*:%s*(.-)%s*$")
  if not who or who == "" then say("//npc <name>: <option> > <option> > ...") return end
  local e = nearest(who)
  if not e then say("no " .. who .. " here.") return end
  if e.distance > 5.5 then
    if e.distance > 30 then say(who .. " is too far away.") return end
    actions.run_to(e.pos.x, e.pos.z, 4)
    for _ = 1, 100 do
      sleep(0.1)
      local n = game.entity(e.id)
      if n and n.distance <= 5.5 then break end
    end
  end
  actions.talk(e.id)
  local last = ""
  for step in path:gmatch("[^>]+") do
    local want = norm(step)
    local m, why = next_menu(last)
    if not m then
      say(why == "closed" and ("the talk ended before " .. step:match("^%s*(.-)%s*$") .. ".") or "the menu did not answer.")
      return
    end
    local pick
    for i, o in ipairs(m.options) do
      if norm(o):find(want, 1, true) then pick = i break end
    end
    if not pick then
      say(("no %s in \"%s\"; the menu is yours."):format(step:match("^%s*(.-)%s*$"), m.question))
      return
    end
    last = sig(m)
    actions.choose(pick)
  end
end

commands.register("npc", function(...)
  if not settings.get("on") then return end
  if game.event() then say("finish the conversation first.") return end
  walk(table.concat({ ... }, " "))
end)

local function errands()
  local s = storage.account
  s.errands = s.errands or {}
  return s.errands
end

commands.register("errand", function(cmd, ...)
  if not settings.get("on") then return end
  local c = (cmd or ""):lower()
  local rest = table.concat({ ... }, " ")
  local list = errands()
  if c == "" then
    say("//errand <name> | save <name> <npc>: <option> > ... | list | delete <name>")
  elseif c == "save" then
    local name, line = rest:match("^(%S+)%s+(.+)$")
    if not name or not line:find(":") then say("//errand save <name> <npc>: <option> > ...") return end
    list[name:lower()] = line
    say(name .. " saved.")
  elseif c == "list" then
    local names = {}
    for k, v in pairs(list) do names[#names + 1] = k .. " = " .. v end
    table.sort(names)
    say(#names > 0 and table.concat(names, "; ") or "none saved.")
  elseif c == "delete" then
    list[rest:lower()] = nil
    say(rest .. " deleted.")
  else
    local line = list[c]
    if not line then say("no errand " .. c .. ".") return end
    if game.event() then say("finish the conversation first.") return end
    walk(line)
  end
end)

-- Job ids by abbreviation, from the game's names.
local function job_id(s)
  s = (s or ""):upper()
  if s == "NONE" or s == "-" then return 0 end
  for id = 1, 22 do
    if res.job(id) == s then return id end
  end
end

commands.register("job", function(spec)
  if not settings.get("on") then return end
  local main, sub = (spec or ""):match("^(%a+)/?(%a*)$")
  local m = job_id(main)
  if not m or m == 0 then say("//job <main>[/<sub>], as //job war/nin or //job blm/none.") return end
  if sub ~= "" then
    local s = job_id(sub)
    if not s then say("no job " .. sub .. ".") return end
    actions.change_job(m, s)
  else
    actions.change_job(m)
  end
end)
