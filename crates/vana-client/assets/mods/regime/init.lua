-- The Fields / Grounds of Valor page in force and its kills, in the training regime's edge window (beside the
-- Timers). Learnt passively, nothing is sent for it: the Field Manual / Grounds Tome event's traffic (its start's
-- params[7], the page answer s2c 0x05C, our end option c2s 0x05B, the 0x02A confirmation, the Review answer), the
-- kill credits (battle message 558), completion (559) and repeats (643); the labels from the zone dialogue's regime
-- block. The server re-sends nothing at login, so the state is kept in storage.character; kills seen with no page
-- known name their rows after the monsters until a book is visited.

local DEFEATS = { [6] = true, [20] = true }
local CREDIT, DONE, ANEW = 558, 559, 643
-- How long after a book event's end its confirmation may come, and a defeat still names a credit's monster.
local CONFIRM_SECS, DEFEAT_SECS = 5, 2

-- Fields of Valor 1..146, Grounds of Valor 602..819 (147..559 are hunts).
local function is_regime(id)
  return (id >= 1 and id <= 146) or (id >= 602 and id <= 819)
end

local book     -- the book event under way: {hunt, start, option, page, pending, at}
local defeat   -- {name, at}: the last monster defeated
local block    -- {zone, r}: the zone's regime block base

local function trim(s)
  return (s:match("^%s*(.-)%s*$"))
end

local function cap(s)
  return s:sub(1, 1):upper() .. s:sub(2)
end

-- One page line's label: "[Siege Bat/Siege Bats]" its singular, "member[/s] of the worm family" "Worm family",
-- "undead-type creature[/s]" "Undead-type".
local function label(line)
  local out, i, n = {}, 1, #line
  while i <= n do
    local c = line:byte(i)
    if c == 0x0a then
      i = i + 2
    elseif c == 0x7f then
      i = i + (line:byte(i + 1) == 0x92 and 3 or 2)
    elseif c == 0x5b then
      local e = line:find("]", i, true) or n + 1
      out[#out + 1] = line:sub(i + 1, e - 1):match("^[^/]*")
      i = e + 1
    elseif c >= 0x80 then
      out[#out + 1] = line:sub(i, i + 1)
      i = i + 2
    elseif c < 0x20 then
      i = i + 1
    else
      out[#out + 1] = string.char(c)
      i = i + 1
    end
  end
  local s = trim(trim(text.from_shift_jis(table.concat(out))):gsub("%.+$", ""))
  local f = s:match("^member of the (.+) family$")
  if f then return cap(f) .. " family" end
  local k = s:match("^(.+) creature$")
  if k then return cap(k) end
  return cap(s)
end

-- A text's lines (split at 07, up to the 00 that ends it; code arguments stepped over: 0a n, 7f 92 n, 7f x).
local function lines(raw)
  local out, start, i, n = {}, 1, 1, #raw
  while i <= n do
    local c = raw:byte(i)
    if c == 0x00 then
      break
    elseif c == 0x07 then
      out[#out + 1] = raw:sub(start, i - 1)
      start = i + 1
      i = i + 1
    elseif c == 0x0a then
      i = i + 2
    elseif c == 0x7f then
      i = i + (raw:byte(i + 1) == 0x92 and 3 or 2)
    elseif c >= 0x80 then
      i = i + 2
    else
      i = i + 1
    end
  end
  out[#out + 1] = raw:sub(start, math.min(i, n + 1) - 1)
  return out
end

-- The page's labels (R - 2050 + id: a line each, every one with its count code) and the area (R + 3 + id).
local function page(zone, r, id)
  if r + id < 2050 then return nil end
  local raw = res.zone_text(zone, r + id - 2050)
  if not raw then return nil end
  local found = {}
  for _, l in ipairs(lines(raw)) do
    if l:find("[\33-\255]") then found[#found + 1] = l end
  end
  if #found == 0 or #found > 4 then return nil end
  for _, l in ipairs(found) do
    if not l:find("\10", 1, true) then return nil end
  end
  local area = ""
  local a = res.zone_text(zone, r + 3 + id)
  if a then
    a = a:sub(1, (a:find("[\0\127]") or #a + 1) - 1)
    area = (trim(trim(text.from_shift_jis(a)):gsub("^Training area:", "")):gsub("%.+$", ""))
  end
  local labels = {}
  for k, l in ipairs(found) do labels[k] = label(l) end
  return labels, area
end

-- This zone's regime block base: the entry that is "New training regime registered!" (one per book zone).
local function zone_block()
  local zone = game.player().zone
  if not zone then return nil end
  if not (block and block.zone == zone) then
    local r = res.zone_text_find(zone, "New training regime registered!")
    if not r then return nil end
    block = { zone = zone, r = r }
  end
  return zone, block.r
end

-- A label fits a monster's name: the same name, or a family / type word starting one of its words ("Bat family" fits
-- "Seeker Bats").
local function matches(lbl, name)
  if lbl:lower() == name:lower() then return true end
  local word = lbl:match("^(.*) family$") or lbl:match("^(.*)%-type$")
  if not word then return false end
  word = word:lower()
  for w in name:gmatch("%w+") do
    if w:lower():sub(1, #word) == word then return true end
  end
  return false
end

local function kind_slot(lbl)
  return lbl:match(" family$") ~= nil or lbl:match("%-type$") ~= nil
end

-- Which slot a credit (k of n) is: the ones it fits by the counts, then the defeated monster's name, then the lowest;
-- with none, the slot of that count nearest. Nil when nothing takes it.
local function attribute(slots, k, n, name)
  local c = {}
  for i, s in ipairs(slots) do
    if (s.needed == n or s.needed == 0) and (not s.known or s.killed + 1 == k) then c[#c + 1] = i end
  end
  if #c == 1 then return c[1] end
  if #c > 1 then
    if name then
      local m, kinds = {}, {}
      for _, i in ipairs(c) do
        if matches(slots[i].label, name) then m[#m + 1] = i end
        if kind_slot(slots[i].label) then kinds[#kinds + 1] = i end
      end
      if #m == 1 then return m[1] end
      if #m == 0 and #kinds == 1 then return kinds[1] end
      if m[1] then return m[1] end
    end
    -- Counts that fit exactly before guesses.
    for _, i in ipairs(c) do
      if slots[i].needed == n and slots[i].known then return i end
    end
    return c[1]
  end
  local best, bd
  for i, s in ipairs(slots) do
    if s.needed == n and s.killed < n then
      local d = math.abs(k - 1 - s.killed)
      if not bd or d < bd then best, bd = i, d end
    end
  end
  return best
end

-- ---- The window ----

local function shown(st)
  return st and not st.complete and #st.slots > 0
end

local function show()
  local st = storage.character.regime
  if not settings.get("on") or not shown(st) then
    ui.close("regime")
    return
  end
  local rows = {}
  for _, s in ipairs(st.slots) do
    local n = s.needed == 0 and "?" or tostring(s.needed)
    local k = s.known and tostring(s.killed) or "?"
    local known = s.known and s.needed > 0
    rows[#rows + 1] = {
      type = "gauge", style = "row", label = s.label, text = k .. "/" .. n,
      value = known and s.killed / s.needed or 0, dim = known and s.killed >= s.needed,
    }
  end
  ui.window("regime", {
    edge = true, panel = "regime", resize = true, title = "Training",
    heading = st.area ~= "" and st.area or "Training Regime", elements = rows,
  })
  ui.open("regime")
end

local function set(st, why)
  log.info("regime: " .. why)
  storage.character.regime = st
  storage.save()
  show()
end

-- ---- What the server tells us ----

-- s2c 0x032..0x034: a Field Manual or Grounds Tome tells the server's regime (params[7]); a Hunt Registry may cancel it.
local function on_event_start(p)
  local d = p.data
  local target = string.unpack("<I4", d, 5)
  local id = 0
  if p.id == 0x034 then
    id = string.unpack("<I4", d, 0x24 + 1)
  elseif p.id == 0x033 and string.unpack("<I2", d, 0xe + 1) & 8 ~= 0 then
    id = string.unpack("<I4", d, 0x6c + 1)
  end
  local e = game.entity(target)
  local name = e and e.name or ""
  local is_book, hunt = name == "Field Manual" or name == "Grounds Tome", name == "Hunt Registry"
  book = (is_book or hunt) and { hunt = hunt, start = id, option = 0 } or nil
  if not is_book then return end
  local st = storage.character.regime
  if not is_regime(id) then
    if st then set(nil, "the book says none") end
    return
  end
  if st and st.id == id then return end
  -- A page we did not see accepted: its labels now, the counts from kills or a Review; a kills-only state's counts go
  -- to the slots their monsters fit.
  local zone, r = zone_block()
  if not zone then return end
  local labels, area = page(zone, r, id)
  if not labels then return end
  local slots = {}
  for k, l in ipairs(labels) do slots[k] = { label = l, needed = 0, killed = 0, known = false } end
  if st and st.id == 0 then
    for _, o in ipairs(st.slots) do
      for _, s in ipairs(slots) do
        if not s.known and matches(s.label, o.label) then
          s.needed, s.killed, s.known = o.needed, o.killed, o.known
          break
        end
      end
    end
  end
  set({ id = id, area = area, slots = slots, ["repeat"] = false, complete = false }, "the book's regime")
end

-- Our c2s 0x05B at a book: an update's option, or the end (2 | page << 4, bit 31 repeat: an accept; 3 a cancel; at a
-- Hunt Registry 4 cancels).
local function on_send(p)
  if not book then return end
  local option, mode = string.unpack("<I4", p.data, 9), string.unpack("<I2", p.data, 0xe + 1)
  if mode == 1 then
    book.option = option
    return
  end
  local pending
  if book.hunt then
    pending = option == 4 and "cancel" or nil
  elseif option & 0xf == 2 and (option >> 4) & 0xff >= 1 then
    pending = option >> 31 ~= 0 and "accept repeat" or "accept"
  elseif option == 3 then
    pending = "cancel"
  end
  book.pending, book.at = pending, clock()
end

-- s2c 0x05C at a book: the page shown (num[0..3] needed, num[7] its id) or the Review (num[0..3] needed, num[4..7]
-- killed).
local function on_params(p)
  if not book then return end
  local num = {}
  for i = 0, 7 do num[i] = string.unpack("<I4", p.data, 4 + 4 * i + 1) end
  if book.option == 1 and num[0] ~= 0 and is_regime(book.start) then
    local id = book.start
    local zone, r = zone_block()
    if not zone then return end
    local labels, area = page(zone, r, id)
    if not labels then return end
    local slots = {}
    for k, l in ipairs(labels) do slots[k] = { label = l, needed = num[k - 1], killed = num[k + 3], known = true } end
    local st = storage.character.regime
    set({ id = id, area = area, slots = slots, ["repeat"] = st ~= nil and st.id == id and st["repeat"], complete = false }, "Review")
  elseif book.option & 0xf == 2 then
    book.page = num
  end
end

-- The zone dialogue line after our accept: the block's "New training regime registered!" (its id is R; the page must
-- fit the block, else it was a refusal); after a cancel, any line.
events.on("zone_text", function(id, actor, plain)
  if not (book and book.pending) then return end
  if clock() - book.at > CONFIRM_SECS then
    book.pending = nil
    return
  end
  if not plain or actor ~= game.player().id then return end
  local pending = book.pending
  book.pending = nil
  if pending == "cancel" then
    set(nil, "cancelled")
    return
  end
  local num = book.page
  if not num then return end
  local rid = num[7]
  local zone = game.player().zone
  local needed = {}
  for i = 0, 3 do
    if num[i] == 0 then break end
    needed[#needed + 1] = num[i]
  end
  local labels, area = page(zone, id, rid)
  if not (labels and is_regime(rid) and #labels == #needed) then
    log.info("regime: text " .. id .. " after an accept is not the block's (a refusal?)")
    return
  end
  block = { zone = zone, r = id }
  local slots = {}
  for k, l in ipairs(labels) do slots[k] = { label = l, needed = needed[k], killed = 0, known = true } end
  set({ id = rid, area = area, slots = slots, ["repeat"] = pending == "accept repeat", complete = false }, "accepted")
end)

events.on("battle_message", function(actor, target, message, p1, p2)
  local msg = message & 0x7fff
  if DEFEATS[msg] then
    local e = game.entity(target)
    if e then defeat = { name = e.name, at = clock() } end
    return
  end
  if target ~= game.player().id then return end
  local st = storage.character.regime
  if msg == CREDIT then
    local k, n = math.max(0, math.min(255, p1)), math.max(0, math.min(255, p2))
    local name = defeat and clock() - defeat.at <= DEFEAT_SECS and defeat.name or nil
    defeat = nil
    st = st or { id = 0, area = "", slots = {}, ["repeat"] = false, complete = false }
    st.complete = false
    local i = attribute(st.slots, k, n, name)
    if not i then
      -- Kills alone: a row per monster, up to the page's four.
      if st.id == 0 and #st.slots < 4 then
        st.slots[#st.slots + 1] = { label = name or ("Target " .. (#st.slots + 1)), needed = 0, killed = 0, known = false }
        i = #st.slots
      else
        log.info(("regime: credit %d/%d (%s) fits no slot"):format(k, n, tostring(name)))
        return
      end
    end
    local s = st.slots[i]
    s.needed, s.killed, s.known = n, k, true
    set(st, ("credit %d/%d (%s)"):format(k, n, tostring(name)))
  elseif msg == DONE then
    if st then
      st.complete = true
      set(st, "complete")
    end
  elseif msg == ANEW then
    if st then
      st.complete, st["repeat"] = false, true
      for _, s in ipairs(st.slots) do s.killed, s.known = 0, true end
      set(st, "begun anew")
    end
  end
end)

events.on("zone_change", function()
  book = nil
end)

for _, id in ipairs({ 0x032, 0x033, 0x034 }) do packets.on_in(id, on_event_start) end
packets.on_in(0x05C, on_params)
packets.on_out(0x05B, on_send)
events.on("settings_changed", show)
show()

-- For the client's tests.
return { label = label, page = page, attribute = attribute, matches = matches }
