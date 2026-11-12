-- //ahbuy, //ahsell, //ahlist at the auction house (the auction helper add-ons). The game's questions follow
-- each bid and sale, as the auction windows ask them.

local function say(t) chat.add("Auction Desk: " .. t) end

-- "12k", "1.5m", "3000": gil, or nil.
local function gil(s)
  local n, unit = (s or ""):lower():gsub(",", ""):match("^(%d+%.?%d*)([km]?)$")
  if not n then return nil end
  local v = tonumber(n) * (unit == "k" and 1000 or unit == "m" and 1000000 or 1)
  return v >= 1 and math.floor(v) or nil
end

-- Item ids by lower-case name over the item ranges the auction house sells, built on first use in slices.
local RANGES = { { 1, 0x0FFF }, { 0x1000, 0x1FFF }, { 0x2800, 0x6FFF } }
local by_name
local function item_named(name)
  local id = tonumber(name)
  if id then return id end
  if not by_name then
    local index, n = {}, 0
    for _, r in ipairs(RANGES) do
      for i = r[1], r[2] do
        local nm = res.name("item", i)
        if nm and nm ~= "" then index[nm:lower()] = index[nm:lower()] or i end
        n = n + 1
        if n % 2000 == 0 then sleep(0.01) end
      end
    end
    by_name = index
  end
  return by_name[name:lower()]
end

-- "<item words> <price> [stack]"
local function parse(args)
  local stack = false
  if (args[#args] or ""):lower() == "stack" then
    stack = true
    table.remove(args)
  end
  local price = gil(table.remove(args))
  return table.concat(args, " "), price, stack
end

local function at_house()
  if game.auction() then return true end
  say("open the auction house first.")
  return false
end

commands.register("ahbuy", function(...)
  if not settings.get("on") then return end
  local name, price, stack = parse({ ... })
  if name == "" or not price then say("//ahbuy <item> <price> [stack]") return end
  if not at_house() then return end
  local id = item_named(name)
  if not id then say("no item named " .. name .. ".") return end
  actions.auction_bid(id, price, stack)
end)

commands.register("ahsell", function(...)
  if not settings.get("on") then return end
  local name, price, stack = parse({ ... })
  if name == "" or not price then say("//ahsell <item> <price> [stack]") return end
  if not at_house() then return end
  for _, it in ipairs(game.find_items(name, { 0 })) do
    local r = res.item(it.id)
    local full = r and it.count >= (r.stack or 1)
    if it.locked == 0 and (not stack or full) then
      actions.auction_sell(it.slot, price, stack)
      return
    end
  end
  say(stack and ("no full stack of " .. name .. " in the inventory.") or ("no " .. name .. " in the inventory."))
end)

commands.register("ahlist", function()
  if not settings.get("on") or not at_house() then return end
  local slots = game.auction().slots
  if #slots == 0 then say("nothing up for sale.") return end
  for _, s in ipairs(slots) do
    local r = res.item(s.item)
    say(("%d. %s%s for %s gil, %s"):format(s.slot, r and r.name or ("item " .. s.item), s.count > 1 and (" x" .. s.count) or "",
      tostring(s.price), s.state))
  end
end)
