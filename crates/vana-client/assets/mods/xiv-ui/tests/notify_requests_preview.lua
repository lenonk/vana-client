-- Notify invite pill: bound panel settings use notify_request_offset_* keys (not bare request_offset_x).

hud = {
  screen = function() return { width = 800, height = 600, ui_scale = 1 } end,
  text = function() end, rect = function() end, image = function() end,
  set = function() end, bar = function() end, icon = function() end, line = function() end,
}
ui = { close = function() end, window = function() end, open = function() end, on = function() end,
  is_open = function() return false end, update = function() end }
chat = { add = function() end }
events = { on = function() end }
mods = { call = function() return nil end, provide = function() end }
game = { info = function() return {} end }
actions = {}
text = { strip = function(s) return s end }
clock = function() return 1000 end

local xiv_src = assert(TEST.source("src/xiv_renderer.lua"))
local X = load(xiv_src, "@xiv_renderer")()
local style = load(assert(TEST.source("modules/notify/src/style.lua")), "@style")()
local orig_require = require
require = function(name)
  if name == "src.xiv" then return X end
  if name == "src.style" then return style end
  return orig_require(name)
end

local suite = settings
local panel = { get = function(key) return suite.get("notify_" .. key) end }
local requests = load(assert(TEST.source("modules/notify/src/requests.lua")), "@requests")()
requests.bind_settings(panel)
requests.preview(true)
requests.draw(true, 1)
requests.preview(false)