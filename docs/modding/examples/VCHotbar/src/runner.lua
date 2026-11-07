-- Runs a multi-line macro.
--
-- Each line is run as if typed. Waits work like the game's macros:
--   /wait 2                  a line that only waits
--   /ma "Cure" <t> <wait 3>  runs the line, then waits
-- A macro that is still running ignores a second press.

local M = {}

local running = {}
local MAX_WAIT = 60

local function clamp_wait(text)
  local n = tonumber(text) or 0
  if n < 0 then
    n = 0
  end
  if n > MAX_WAIT then
    n = MAX_WAIT
  end
  return n
end

-- -> "skip" | "wait", seconds | "cmd", line, seconds_to_wait_after?
function M.parse_line(line)
  line = tostring(line or ""):gsub("^%s+", ""):gsub("%s+$", "")
  if line == "" then
    return "skip"
  end
  local only = line:match("^/wait%s+([%d%.]+)$")
  if only then
    return "wait", clamp_wait(only)
  end
  local body, after = line:match("^(.-)%s*<wait%s+([%d%.]+)>%s*$")
  if body then
    if body == "" then
      return "wait", clamp_wait(after)
    end
    return "cmd", body, clamp_wait(after)
  end
  return "cmd", line
end

function M.is_running(macro)
  return running[macro] == true
end

-- `macro` is { name, lines }. Call from a player-input handler (a bind or a click)
-- so the `actions` permission applies, also across the waits.
function M.run(macro)
  if type(macro) ~= "table" or type(macro.lines) ~= "table" or running[macro] then
    return
  end
  running[macro] = true
  local ok, err = pcall(function()
    for _, line in ipairs(macro.lines) do
      local kind, a, b = M.parse_line(line)
      if kind == "wait" then
        sleep(a)
      elseif kind == "cmd" then
        commands.run(a)
        if b and b > 0 then
          sleep(b)
        end
      end
    end
  end)
  running[macro] = nil
  if not ok then
    error(err, 0)
  end
end

local last_warn = nil

-- Like run, but a refused permission becomes one clear chat line (at most one every
-- 5 seconds) instead of an error on every key press: the client suspends a mod that
-- raises 20 errors in a minute. Any other error is raised as before.
function M.run_checked(macro)
  local ok, err = pcall(M.run, macro)
  if ok then
    return true
  end
  err = tostring(err)
  if err:find("permission", 1, true) then
    local now = os.time()
    if not last_warn or now - last_warn >= 5 then
      last_warn = now
      chat.add("VCHotbar needs the 'actions' permission to run macros: type /lua grant VCHotbar actions "
        .. "(or switch on Allow game actions on its Extras page), then try again.")
    end
    return false
  end
  error(err, 0)
end

return M
