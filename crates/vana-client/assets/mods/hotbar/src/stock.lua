-- The game's macro palette: back up and wipe (or restore) the set you are on.
--
--   //hb wipestock     asks first, saves a backup, then clears the current set's 20 macros
--   //hb restorestock  puts the backup of the current set back
--
-- The backup is stock-macros-backup.json in the mod's data folder, keyed "<book>:<set>":
--   { "1:1": { book = 1, set = 1, book_title = "Book01", saved = <os.time>, ctrl = {...}, alt = {...} } }
-- Each macro is { title = "...", lines = { 6 strings } }, as game.macros() returns it.

local M = {}

local BACKUP = "stock-macros-backup.json"
local ROWS = { "ctrl", "alt" }

local function key_of(m)
  return tostring(m.book) .. ":" .. tostring(m.set)
end

local function macro_empty(mac)
  if type(mac) ~= "table" then
    return true
  end
  if (mac.title or "") ~= "" then
    return false
  end
  for _, line in ipairs(mac.lines or {}) do
    if line ~= "" then
      return false
    end
  end
  return true
end

-- How many of the current set's 20 macros hold anything.
function M.used(m)
  local n = 0
  for _, row in ipairs(ROWS) do
    for _, mac in ipairs(m[row] or {}) do
      if not macro_empty(mac) then
        n = n + 1
      end
    end
  end
  return n
end

local function snapshot(m)
  local snap = { book = m.book, set = m.set, book_title = m.book_title, saved = os.time() }
  for _, row in ipairs(ROWS) do
    snap[row] = {}
    for i, mac in ipairs(m[row] or {}) do
      local lines = {}
      for j = 1, 6 do
        lines[j] = (mac.lines and mac.lines[j]) or ""
      end
      snap[row][i] = { title = mac.title or "", lines = lines }
    end
  end
  return snap
end

local function read_backups()
  local text = fs.read(BACKUP)
  if not text or text == "" then
    return {}
  end
  local ok, db = pcall(json.decode, text)
  if ok and type(db) == "table" then
    return db
  end
  return nil, "the existing backup file could not be read"
end

-- Saves the set. A set that is already empty never replaces an earlier backup of it.
function M.backup(m)
  local db, why = read_backups()
  if not db then
    return false, why
  end
  if M.used(m) > 0 or not db[key_of(m)] then
    db[key_of(m)] = snapshot(m)
  end
  fs.write(BACKUP, json.encode(db, { pretty = true }))
  return true
end

-- Applies `texts[row][i] = { title, lines }` to the current set; returns edits made and failures.
local function apply(get)
  local done, failed = 0, 0
  for _, row in ipairs(ROWS) do
    for i = 1, 10 do
      local title, lines = get(row, i)
      if title ~= nil then -- nil: leave this macro alone
        local function edit(line, text)
          local ok = pcall(actions.macro_edit, row, i, line, text)
          if ok then
            done = done + 1
          else
            failed = failed + 1
          end
        end
        edit("title", title)
        for j = 1, 6 do
          edit(j, lines[j] or "")
        end
      end
    end
  end
  return done, failed
end

local EMPTY = {}

local function current()
  local m = game.macros()
  if not m then
    chat.add("Hotbars: the game's macros have not loaded yet.")
  end
  return m
end

local function wipe(m)
  local ok, why = M.backup(m)
  if not ok then
    chat.add("Hotbars: nothing was wiped, the backup failed (" .. tostring(why) .. ").")
    return
  end
  local _, failed = apply(function(row, i)
    if macro_empty(m[row] and m[row][i]) then
      return nil
    end
    return "", EMPTY
  end)
  sleep(0.5)
  local after = game.macros()
  local left = after and M.used(after) or 0
  chat.add(("Hotbars: wiped book %s set %s (backup saved to %s)%s."):format(
    tostring(m.book), tostring(m.set), BACKUP,
    left > 0 and (", but " .. left .. " macros still hold text, run it again") or (failed > 0 and (", " .. failed .. " edits were refused") or "")))
end

function M.start_wipe()
  local m = current()
  if not m then
    return
  end
  local n = M.used(m)
  if n == 0 then
    chat.add(("Hotbars: book %s set %s has no stock macros to wipe."):format(tostring(m.book), tostring(m.set)))
    return
  end
  ui.prompt(("Wipe the %d stock macros of book %s, set %s? A backup is saved first."):format(
    n, tostring(m.book), tostring(m.set)), { "Wipe", "Cancel" }, function(i)
    if i == 1 then
      local fresh = current()
      if fresh then
        wipe(fresh)
      end
    end
  end)
end

function M.restore()
  local m = current()
  if not m then
    return
  end
  local db, why = read_backups()
  if not db then
    chat.add("Hotbars: " .. why .. ".")
    return
  end
  local snap = db[key_of(m)]
  if not snap then
    chat.add(("Hotbars: no backup of book %s set %s."):format(tostring(m.book), tostring(m.set)))
    return
  end
  local _, failed = apply(function(row, i)
    local mac = snap[row] and snap[row][i]
    return (mac and mac.title) or "", (mac and mac.lines) or EMPTY
  end)
  chat.add(("Hotbars: restored book %s set %s from the backup%s."):format(
    tostring(m.book), tostring(m.set), failed > 0 and (" (" .. failed .. " edits were refused)") or ""))
end

return M
