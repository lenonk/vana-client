-- Chat windows (the chat window add-ons): one channel's lines in a separate window with a line to answer in, and
-- a search over every line of the session.

local CHANNELS = {
  tell = { kind = 3, title = "Tells" }, party = { kind = 4, title = "Party", send = "/p" },
  ls = { kind = 5, title = "Linkshell", send = "/l" }, ls2 = { kind = 27, title = "Linkshell 2", send = "/l2" },
  say = { kind = 0, title = "Say", send = "/s" }, shout = { kind = 1, title = "Shout", send = "/sh" },
  yell = { kind = 26, title = "Yell", send = "/y" }, unity = { kind = 33, title = "Unity", send = "/u" },
}
local KEEP = 200
local lines = {}      -- by kind: "hh:mm Sender: text"
local senders = {}    -- tell: the last one who wrote
local session = {}    -- every log line, for //chatfind
local MARK = "\u{00B7} " -- our search results, kept out of the search

local function stamp() return os.date("%H:%M") end

local function show(name)
  local c = CHANNELS[name]
  local rows = lines[c.kind] or {}
  ui.window("chat_" .. name, {
    title = c.title, width = 440,
    elements = {
      { type = "list", id = "lines", rows = rows, height = 8, at = math.max(1, #rows) },
      { type = "input", id = "say", label = name == "tell" and ("To " .. (senders.last or "?")) or "Say", text = "", max = 120 },
    },
  })
end

events.on("chat_message", function(msg, sender, kind)
  if not settings.get("on") then return end
  local l = lines[kind] or {}
  lines[kind] = l
  l[#l + 1] = ("%s %s: %s"):format(stamp(), sender ~= "" and sender or "?", text.plain(msg))
  if #l > KEEP then table.remove(l, 1) end
  if kind == 3 and sender ~= "" then senders.last = sender end
  for name, c in pairs(CHANNELS) do
    if c.kind == kind and ui.is_open("chat_" .. name) then show(name) end
  end
end)

filters.chat(function(t)
  if t:find("^Chat Windows:") or t:find("^" .. MARK) then return end
  session[#session + 1] = t
  if #session > 4000 then table.remove(session, 1) end
end)

local function answer(name, said)
  if said == "" then return end
  local c = CHANNELS[name]
  if name == "tell" then
    if not senders.last then chat.add("Chat Windows: no tell to answer yet.") return end
    commands.run("/t " .. senders.last .. " " .. said)
  else
    commands.run(c.send .. " " .. said)
  end
  -- ours are not chat_message lines: the window keeps them itself
  local l = lines[c.kind] or {}
  lines[c.kind] = l
  local me = (game.player() or {}).name or "?"
  l[#l + 1] = ("%s %s: %s"):format(stamp(), name == "tell" and (me .. " >> " .. senders.last) or me, said)
  show(name)
end

for name in pairs(CHANNELS) do
  ui.on("chat_" .. name, function(_, e)
    if e.kind == "change" and e.id == "say" and e.text ~= "" then
      answer(name, e.text)
      ui.update("chat_" .. name, "say", { text = "" })
    end
  end)
end

commands.register("chatwin", function(name)
  if not settings.get("on") then return end
  name = (name or ""):lower()
  if not CHANNELS[name] then
    chat.add("Chat Windows: //chatwin <tell | party | ls | ls2 | say | shout | yell | unity>")
    return
  end
  local id = "chat_" .. name
  if ui.is_open(id) then
    ui.close(id)
  else
    show(name)
    ui.open(id)
  end
end)

commands.register("chatfind", function(...)
  if not settings.get("on") then return end
  local want = table.concat({ ... }, " "):lower()
  if want == "" then chat.add("Chat Windows: //chatfind <words>") return end
  local hits = {}
  for _, t in ipairs(session) do
    local plain = text.strip(t)
    if plain:lower():find(want, 1, true) then hits[#hits + 1] = plain end
  end
  if #hits == 0 then chat.add("Chat Windows: no line with " .. want .. ".") return end
  chat.add(("Chat Windows: %d line%s with %s:"):format(#hits, #hits > 1 and "s" or "", want))
  for i = math.max(1, #hits - 9), #hits do chat.add(MARK .. hits[i]) end
end)
