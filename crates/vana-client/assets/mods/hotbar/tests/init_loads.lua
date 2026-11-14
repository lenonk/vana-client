-- Hotbars: init.lua loads and wires every module.

chat = { add = function() end }
ui.native = { hide = function() end, list = function() return nil end }
res = setmetatable({ job = function(id) return ({ [1] = "WAR" })[id] end },
  { __index = function() return function() end end })
game = setmetatable({
  player = function() return { main_job = 1, sub_job = 0 } end,
  recasts = function() return { spells = {}, abilities = {}, totals = { spells = {}, abilities = {} } } end,
  info = function() return { server = "Test" } end,
}, { __index = function() return function() return {} end end })
local ok, err = pcall(require, "init")
assert(ok, tostring(err))
