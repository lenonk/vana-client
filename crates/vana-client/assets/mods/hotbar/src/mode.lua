-- Hotbars or gamepad mode: the Mode setting, overridden by //hb mode or Detect Gamepad until the setting changes.

local M = {}

M.HOTBARS = "hotbars"
M.GAMEPAD = "gamepad"

local override = nil

function M.get()
  if override then return override end
  return settings.get("mode") == "Gamepad" and M.GAMEPAD or M.HOTBARS
end

function M.is_pad()
  return M.get() == M.GAMEPAD
end

function M.set(to)
  override = to == M.GAMEPAD and M.GAMEPAD or M.HOTBARS
end

function M.use_setting()
  override = nil
end

local seen = nil -- controller connected at the last sample; nil before the first
function M.forget()
  seen = nil
end

-- Returns the mode to switch to when the connection changed (the first sample switches only to gamepad), else nil.
function M.detect(enabled, connected)
  local before = seen
  seen = connected
  if not enabled or connected == before or (before == nil and not connected) then return nil end
  local to = connected and M.GAMEPAD or M.HOTBARS
  if M.get() == to then return nil end
  M.set(to)
  return to
end

function M.toggle()
  M.set(M.is_pad() and M.HOTBARS or M.GAMEPAD)
  return M.get()
end

local ALIASES = {
  hotbars = M.HOTBARS, hotbar = M.HOTBARS, bars = M.HOTBARS, keyboard = M.HOTBARS, keys = M.HOTBARS,
  gamepad = M.GAMEPAD, pad = M.GAMEPAD, controller = M.GAMEPAD, crossbar = M.GAMEPAD, crossbars = M.GAMEPAD,
}

function M.parse(text)
  return ALIASES[tostring(text or ""):lower()]
end

return M
