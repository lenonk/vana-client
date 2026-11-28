-- Bootstrap-load every panel with all mod_* toggles on (catches undeclared settings.get during import).

local toggles = {
  on = true,
  mod_target = true, mod_focus = true, mod_cast = true, mod_player = true,
  mod_buff = true, mod_exp = true, mod_party = true, mod_notify = true,
}
local real_get = settings.get
settings.get = function(k)
  if toggles[k] ~= nil then return toggles[k] end
  return real_get(k)
end

game = setmetatable({
  player = function() return { id = 1, name = "Test", main_job = 1, sub_job = 0 } end,
  party = function() return {} end,
  info = function() return { menu_open = false } end,
  target = function() return nil end,
  buffs = function() return {} end,
  items = function() return {} end,
  bag_size = function() return 0 end,
}, { __index = function() return function() return nil end end })
res = setmetatable({}, { __index = function() return function() return nil end end })
chat = { add = function() end }

local FRAME
events.on = function(name, f) if name == "frame" then FRAME = f end end
load(assert(TEST.source("init.lua")), "@init.lua")()
assert(FRAME, "init.lua did not register a frame handler")

for _ = 1, 5000 do
  local ok, err = pcall(FRAME)
  assert(ok, err)
end