-- Totals of the worn gear's stats (checkparam and kin), read from each item's description and augments as the item
-- window shows them. Conditional lines are left out; DMG and Delay are a weapon's, not summed.

local WIN = "gearstats"
local SLOTS = { "Main", "Sub", "Range", "Ammo", "Head", "Body", "Hands", "Legs", "Feet", "Neck", "Waist", "Ear", "Ear",
  "Ring", "Ring", "Back" }
local SKIP_LINE = { "^Pet:", "^Avatar:", "^Automaton:", "^Wyvern:", "^Luopan:", "^Set:", "^Latent effect", "^Unity Ranking",
  "^Weather:", "^Day:", "^Night:", "^Daytime:", "^Dynamis", "^Additional effect", "^Enchantment", "^Lv%.", "^Campaign",
  "^Assault", "^Besieged", "^Salvage", "^Reives", "^Domain Invasion", "^In areas", "^Jug pet:" }
local NOT_SUMMED = { DMG = true, Delay = true }

-- One line's stats into totals.
local function add_line(totals, line)
  for _, p in ipairs(SKIP_LINE) do
    if line:find(p) then return end
  end
  local def = line:match("DEF:(%d+)")
  if def then totals["DEF"] = (totals["DEF"] or 0) + tonumber(def) end
  for name, sign, n, pct in line:gmatch('([%a"][%a%s%."\']-)%s*([%+%-])(%d+)(%%?)') do
    name = name:gsub("^%s+", ""):gsub("%s+$", "")
    if not NOT_SUMMED[name] and name ~= "" then
      local key = name .. pct
      totals[key] = (totals[key] or 0) + (sign == "-" and -1 or 1) * tonumber(n)
    end
  end
end

local function totals()
  local out = {}
  for slot, worn in pairs(game.equipment()) do
    if worn.id and SLOTS[slot + 1] then
      local r = res.item(worn.id)
      for line in ((r and r.description) or ""):gmatch("[^\n]+") do add_line(out, line) end
      local it
      for _, x in ipairs(game.items(worn.bag)) do
        if x.slot == worn.slot then it = x break end
      end
      if it and it.extdata then
        for _, line in ipairs(res.augments(worn.id, it.extdata).lines or {}) do add_line(out, line) end
      end
    end
  end
  return out
end

local function rows()
  local t = totals()
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = k end
  table.sort(keys)
  local out = {}
  for _, k in ipairs(keys) do
    local v = t[k]
    if v ~= 0 then
      local pct = k:sub(-1) == "%"
      out[#out + 1] = { pct and k:sub(1, -2) or k, ("%s%d%s"):format(v > 0 and k ~= "DEF" and "+" or "", v, pct and "%" or "") }
    end
  end
  return out
end

local function show()
  ui.window(WIN, {
    title = "Gear Stats", width = 340,
    elements = { { type = "list", id = "stats", rows = rows(), height = 14,
      columns = { { title = "Stat", width = 190 }, { title = "Total", width = 80, align = "right" } } } },
  })
end

commands.register("gearstats", function()
  if not settings.get("on") then return end
  if ui.is_open(WIN) then ui.close(WIN) return end
  show()
  ui.open(WIN)
end)

-- Kept current while open: built again when what is worn changes.
local worn_key
timer.every(1, function()
  if not ui.is_open(WIN) then worn_key = nil return end
  local ids = {}
  for slot = 0, 15 do ids[#ids + 1] = tostring((game.equipment()[slot] or {}).id) end
  local k = table.concat(ids, ",")
  if k ~= worn_key then
    worn_key = k
    show()
  end
end)
