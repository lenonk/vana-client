-- The macro palette follows the job (the macro changer add-ons): the book and set saved for main / sub, else for the
-- main job, else the book titled with the main job's abbreviation (set 1).

local function books()
  local s = storage.character
  s.books = s.books or {}
  return s.books
end

local function jobs()
  local p = game.player()
  if not p or not p.main_job then return nil end
  local main, sub = res.job(p.main_job), p.sub_job and p.sub_job > 0 and res.job(p.sub_job) or nil
  return main, sub
end

local function apply()
  if not settings.get("on") then return end
  local main, sub = jobs()
  if not main then return end
  local b = books()
  local pick = (sub and b[main .. "/" .. sub]) or b[main]
  if not pick then
    local m = game.macros()
    for i, title in ipairs(m and m.books or {}) do
      if title:upper() == main then
        pick = { book = i, set = 1 }
        break
      end
    end
  end
  if pick then actions.macro_set(pick.book, pick.set) end
end

events.on("job_change", apply)

-- The forms of //macros as mod.toml declares them: what the chat line's hint and //mods show.
local function usage()
  chat.add("Macro Book Switch:")
  for _, line in ipairs(commands.usage("macros")) do chat.add("  " .. line) end
end

commands.register("macros", function(cmd, which)
  cmd = (cmd or ""):lower()
  local main, sub = jobs()
  local key = main and ((which or ""):lower() == "sub" and sub and (main .. "/" .. sub) or main)
  local b = books()
  if cmd == "save" and key then
    local m = game.macros()
    if not m then
      chat.add("Macro Book Switch: the macros are not loaded yet.")
      return
    end
    b[key] = { book = m.book, set = m.set }
    storage.save()
    chat.add(("Macro Book Switch: %s uses book %d (%s), set %d."):format(key, m.book, m.book_title or "", m.set))
  elseif cmd == "forget" and key then
    b[key] = nil
    storage.save()
    chat.add("Macro Book Switch: " .. key .. " forgotten.")
  elseif cmd == "list" then
    local out = {}
    for k, v in pairs(b) do out[#out + 1] = ("%s: book %d set %d"):format(k, v.book, v.set) end
    table.sort(out)
    chat.add("Macro Book Switch: " .. (#out > 0 and table.concat(out, "; ") or "nothing saved"))
  else
    usage()
  end
end)

-- Once at the start, on the first frame that knows the job and has the macros (the palette's book loads with them).
local first
first = events.on("frame", function()
  if jobs() and game.macros() then
    events.off(first)
    apply()
  end
end)
