-- Music by choice (the music add-ons): //music <track> holds a track; in battle the zone's day or night music, or
-- silence; while mounted, silence.

local ENGAGED, MOUNTED = 1, 5
local picked     -- a track held by //music
local holding    -- what we hold now: a track id, "mute", or nil

-- The zone's field track for the hour, as the game picks it (night from 18:00 to 6:00).
local function zone_track()
  local slots = audio.music_slots()
  local h = game.time().hour
  local t = (h < 6 or h > 17) and slots[2] or slots[1]
  return t and t > 0 and t or nil
end

local function want()
  if not settings.get("on") then return nil end
  if picked then return picked end
  local me = game.player()
  local status = me and me.status
  if status == MOUNTED and settings.get("mount") == "Silence" then return "mute" end
  if status == ENGAGED then
    local b = settings.get("battle")
    if b == "Silence" then return "mute" end
    if b == "The Zone's" then return zone_track() end
  end
  return nil
end

local function apply()
  local w = want()
  if w == holding then return end
  if holding == "mute" then audio.mute_music(false) end
  if w == "mute" then
    audio.music(nil)
    audio.mute_music(true)
  elseif w then
    audio.music(w)
  else
    audio.music(nil)
  end
  holding = w
end

timer.every(0.5, apply)
events.on("settings_changed", apply)

commands.register("music", function(arg)
  arg = (arg or ""):lower()
  if arg == "off" or arg == "" then
    picked = nil
  elseif tonumber(arg) then
    picked = tonumber(arg)
  else
    chat.add("Music Picker: //music <track number>, //music off")
    return
  end
  apply()
end)
