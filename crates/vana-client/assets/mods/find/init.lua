-- /find <words>: one line per item whose name has them, with the bags holding it and the count in each; the account's
-- other characters' bags too, as they were when each last played.
local BAGS = { [0] = "Inventory", "Mog Safe", "Storage", "Temporary", "Mog Locker", "Mog Satchel", "Mog Sack", "Mog Case", "Mog Wardrobe", "Mog Safe 2" }

local function bag_name(b)
  if b >= 10 and b <= 16 then
    return "Mog Wardrobe " .. (b - 8)
  end
  return BAGS[b] or ("Bag " .. b)
end

local function item_name(id)
  local name = res.item(id)
  return type(name) == "table" and name.name or name
end

-- Our bags as {[item id] = {[bag] = count}}, kept for the account's other characters' /find (the find-all add-ons).
local function snapshot()
  local out = {}
  for bag = 0, 17 do
    for _, it in ipairs(game.items(bag)) do
      local k = tostring(it.id)
      out[k] = out[k] or {}
      out[k][tostring(bag)] = (out[k][tostring(bag)] or 0) + math.max(it.count, 1)
    end
  end
  return out
end

local function remember()
  local me = game.player()
  if not me or not me.name or me.name == "" then return end
  local s = storage.account
  s.characters = s.characters or {}
  s.characters[me.name] = { at = os.time(), items = snapshot() }
end

timer.every(60, remember)
events.on("zone_change", remember)
events.on("logout", function()
  remember()
  storage.save()
end)

commands.register("find", function(...)
  local what = table.concat({ ... }, " ")
  local want = what:lower()
  if want == "" then return end
  local found, names = {}, {}
  local me = game.player()
  local function add(name, place, n)
    if not found[name] then
      found[name] = {}
      names[#names + 1] = name
    end
    found[name][place] = (found[name][place] or 0) + n
  end
  for bag = 0, 17 do
    for _, it in ipairs(game.items(bag)) do
      local name = item_name(it.id)
      if name and name:lower():find(want, 1, true) then add(name, bag, math.max(it.count, 1)) end
    end
  end
  -- The account's other characters, as their bags were last seen.
  for who, c in pairs(storage.account.characters or {}) do
    if not me or who ~= me.name then
      for id, bags in pairs(c.items or {}) do
        local name = item_name(tonumber(id))
        if name and name:lower():find(want, 1, true) then
          for bag, n in pairs(bags) do add(name, who .. "'s " .. bag_name(tonumber(bag)), n) end
        end
      end
    end
  end
  -- The system message mode's colour, as the client's lines.
  if #names == 0 then
    chat.add("/find " .. what .. ": (none)", { colour = 0x8b })
    return
  end
  table.sort(names)
  for _, name in ipairs(names) do
    local at = {}
    for bag = 0, 17 do
      if found[name][bag] then
        at[#at + 1] = bag_name(bag) .. " " .. found[name][bag]
      end
    end
    local others = {}
    for place, n in pairs(found[name]) do
      if type(place) == "string" then others[#others + 1] = place .. " " .. n end
    end
    table.sort(others)
    for _, o in ipairs(others) do at[#at + 1] = o end
    chat.add(name .. ": " .. table.concat(at, ", "), { colour = 0x8b })
  end
end)
