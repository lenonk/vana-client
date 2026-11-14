-- Runs a macro's lines as typed commands. "/wait 2" waits; a trailing "<wait 3>" waits after its line. A macro still
-- running ignores another press.

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

-- Call from player input (a bind or click) so the actions permission applies.
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

-- run, with a refused permission reported in chat (at most every 5 s) instead of raised: repeated errors would
-- suspend the mod.
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
      chat.add("Hotbars could not run the macro: " .. err)
    end
    return false
  end
  error(err, 0)
end

return M
