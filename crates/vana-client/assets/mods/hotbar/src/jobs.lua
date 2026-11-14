-- Job abbreviations ("RDM").

local M = {}

local names = nil

function M.abbr(id)
  if not id or id == 0 then
    return nil
  end
  return res.job(id)
end

-- Main, sub, and game.player().
function M.current()
  local p = game.player()
  return M.abbr(p.main_job), M.abbr(p.sub_job), p
end

-- "rdm " -> "RDM" when it is a job, else nil.
function M.parse(text)
  local up = tostring(text or ""):gsub("^%s+", ""):gsub("%s+$", ""):upper()
  if up == "" then
    return nil
  end
  if not names then
    names = {}
    for id = 1, 22 do
      local a = res.job(id)
      if a then
        names[a] = true
      end
    end
  end
  if names[up] then
    return up
  end
  return nil
end

return M
