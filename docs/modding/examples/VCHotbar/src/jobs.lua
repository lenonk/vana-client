-- Job abbreviations ("RDM") for the player's main and support job.

local M = {}

local names = nil

-- Abbreviation for a job id, or nil for none (0 / nil).
function M.abbr(id)
  if not id or id == 0 then
    return nil
  end
  return res.job(id)
end

-- Main and sub abbreviations of the current character, and the Player table.
function M.current()
  local p = game.player()
  return M.abbr(p.main_job), M.abbr(p.sub_job), p
end

-- Normalises typed text ("rdm " -> "RDM") and returns it if it is a real job, else nil.
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
