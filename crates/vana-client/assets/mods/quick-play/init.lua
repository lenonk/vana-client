-- Plays the chosen character once the lobby shows the character list, once per start (the auto-login add-ons). The
-- password stays the login screen's (Remember password).

local done = false

local function try()
  if done or not settings.get("on") then return end
  local want = (settings.get("character") or ""):lower()
  if want == "" then return end
  local s = lobby.state()
  if not s or s.busy or (s.screen ~= "select" and s.screen ~= "title") then return end
  for _, c in ipairs(s.characters or {}) do
    if c.name and c.name:lower() == want and c.available ~= false then
      done = true
      lobby.play(c.name)
      return
    end
  end
end

events.on("lobby_screen", try)
timer.every(0.5, try)
