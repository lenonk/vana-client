local factory = require("src.hud_config_factory")
local ALIAS = "player"
local M = {}
function M.new(base, schema, hooks) return factory.new(base, schema, hooks, ALIAS) end
function M.active() return factory.active(ALIAS) end
function M.read(key, base) return factory.read(ALIAS, key, base) end
return M