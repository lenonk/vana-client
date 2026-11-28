-- XIV-UI: load heavy M18 modules across frames (100 ms budget per slice).
local function load_src(path)
  local text = fs.read(path)
  local f, err = load(text, "@" .. path, "t", _ENV)
  if not f then error(err, 0) end
  return f()
end

local bridge = load_src("src/bridge.lua")
local bootstrap = load_src("src/bootstrap.lua")
local settings_proxy = load_src("src/settings_proxy.lua")
local assets = load_src("src/assets.lua")

if not rawget(_G, "__xiv_ui_hud_image_patched") then
  rawset(_G, "__xiv_ui_hud_image_patched", true)
  local old_image = hud.image
  function hud.image(id, spec)
    if type(spec) == "table" and type(spec.path) == "string" then
      local copy = {}
      for k, v in pairs(spec) do copy[k] = v end
      copy.path = assets.resolve_path(copy.path, id)
      spec = copy
    end
    return old_image(id, spec)
  end
  local old_set = hud.set
  function hud.set(id, spec)
    if type(spec) == "table" and type(spec.path) == "string" then
      local copy = {}
      for k, v in pairs(spec) do copy[k] = v end
      copy.path = assets.resolve_path(copy.path, id)
      spec = copy
    end
    return old_set(id, spec)
  end
end

local function load_manifest(dir)
  local f = load(fs.read(dir .. "/manifest.lua"), "@" .. dir .. "/manifest.lua", "t", _ENV)
  return f()
end

local PANEL_TOGGLES = {
  "mod_target", "mod_focus", "mod_cast", "mod_player", "mod_buff", "mod_exp", "mod_party", "mod_notify",
}

local function any_panel_enabled()
  for _, key in ipairs(PANEL_TOGGLES) do
    if settings.get(key) == true then return true end
  end
  return false
end

-- Master off still allows loading when at least one panel toggle is on; master on with all
-- panels off loads an idle suite (no submodule queue).
local function suite_on()
  if settings.get("on") == false and not any_panel_enabled() then return false end
  return true
end

mods.provide("loaded", function() return true end)
mods.provide("suite", function() return bridge.registry end)
mods.provide("xiv_config", function(op, panel, ...)
  return bridge.config_dispatch(op, panel, ...)
end)

-- Host provides for other mods/tools. Do not reuse bare names across panels
-- (e.g. target and player both had "layout" — now player is player_layout).
local HOST_COMPAT = {
  target = { layout = "layout", cast = "cast", casts_for = "casts_for", effects = "effects" },
  player = { player_layout = "layout" },
}

local function publish_host_compat()
  for alias, mapping in pairs(HOST_COMPAT) do
    local tab = bridge.registry[alias]
    if not tab then goto continue end
    for host_name, method in pairs(mapping) do
      local fn = tab[method]
      if type(fn) == "function" then mods.provide(host_name, fn) end
    end
    ::continue::
  end
end

local queue = {}
local function enqueue(alias, dir, toggle)
  if suite_on() and settings.get(toggle) == true then
    queue[#queue + 1] = { alias, dir, toggle }
  end
end
enqueue("target", "modules/target", "mod_target")
enqueue("focus", "modules/focus", "mod_focus")
enqueue("cast", "modules/cast", "mod_cast")
enqueue("player", "modules/player", "mod_player")
enqueue("buff", "modules/buff", "mod_buff")
enqueue("exp", "modules/exp", "mod_exp")
enqueue("party", "modules/party", "mod_party")
enqueue("notify", "modules/notify", "mod_notify")

local next_ix = 1
local active_loader = nil

local function pump()
  if active_loader then
    if bootstrap.step(active_loader) then active_loader = nil end
    return
  end
  if next_ix > #queue then return end
  local job = queue[next_ix]
  next_ix = next_ix + 1
  active_loader = bootstrap.begin(bridge, assets, {
    prefix = job[1],
    dir = job[2],
    manifest = load_manifest(job[2]),
    settings = settings_proxy.proxy(job[1]),
  })
end

local function load_complete()
  return next_ix > #queue and not active_loader
end

if #queue > 0 then
  local frame_id
  local pump_busy = false
  local function frame_hook()
    if pump_busy then return end
    pump_busy = true
    local ok, err = pcall(pump)
    pump_busy = false
    if not ok then
      if active_loader and active_loader.saved_req then
        _ENV.require = active_loader.saved_req
        _ENV.settings = active_loader.saved_settings
        if active_loader.unwrap then active_loader.unwrap() end
      end
      active_loader = nil
      if log and log.error then log.error("XIV-UI load: " .. tostring(err)) end
      events.off(frame_id)
      return
    end
    if load_complete() then
      events.off(frame_id)
      publish_host_compat()
    end
  end
  frame_id = events.on("frame", frame_hook)
elseif suite_on() then
  publish_host_compat()
end

local TOGGLE_ALIAS = {
  mod_target = "target", mod_focus = "focus", mod_cast = "cast", mod_player = "player",
  mod_buff = "buff", mod_exp = "exp", mod_party = "party", mod_notify = "notify",
}
local last_panels = {}
for _, toggle in ipairs(PANEL_TOGGLES) do
  last_panels[toggle] = settings.get(toggle) == true
end
events.on("settings_changed", function()
  for _, toggle in ipairs(PANEL_TOGGLES) do
    local now = settings.get(toggle) == true
    local was = last_panels[toggle]
    last_panels[toggle] = now
    if now and not was then
      local alias = TOGGLE_ALIAS[toggle]
      if alias and not bridge.registry[alias] then
        chat.add("XIV-UI: " .. toggle .. " is on — /lua reload xiv-ui to load that panel.")
      end
    end
  end
end)