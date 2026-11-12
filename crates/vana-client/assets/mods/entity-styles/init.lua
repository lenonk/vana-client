-- How characters draw on our screen only (the renamer and name plate add-ons): renames by name, our name plate
-- hidden, our size. Applied twice a second to what is in range.

local styled = {}  -- server id -> true while we style it

local function renames()
  local s = storage.account
  s.renames = s.renames or {}
  return s.renames
end

local function apply()
  local seen = {}
  if settings.get("on") then
    local r = renames()
    for _, e in ipairs(game.entities()) do
      local look
      if e.me then
        local scale = settings.get("my_scale")
        if settings.get("hide_name") or scale ~= 1 then
          look = { scale = scale ~= 1 and scale or nil, name = settings.get("hide_name") and "" or nil }
        end
      elseif e.name and r[e.name:lower()] then
        look = { name = r[e.name:lower()] }
      end
      if look then
        render.entity(e.id, look)
        seen[e.id] = true
      end
    end
  end
  for id in pairs(styled) do
    if not seen[id] then render.entity(id, nil) end
  end
  styled = seen
end

timer.every(0.5, apply)
events.on("settings_changed", apply)

commands.register("rename", function(...)
  local line = table.concat({ ... }, " ")
  local from, to = line:match("^%s*(.-)%s+as%s+(.-)%s*$")
  local r = renames()
  if from and to ~= "" then
    r[from:lower()] = to
  elseif line:match("%S") then
    r[line:match("^%s*(.-)%s*$"):lower()] = nil
  else
    chat.add("Entity Styles: //rename <name> as <new name>, //rename <name> (undo), //renames")
    return
  end
  storage.save()
  apply()
end)

commands.register("renames", function()
  local out = {}
  for k, v in pairs(renames()) do out[#out + 1] = k .. " -> " .. v end
  table.sort(out)
  chat.add("Entity Styles: " .. (#out > 0 and table.concat(out, "; ") or "none"))
end)
