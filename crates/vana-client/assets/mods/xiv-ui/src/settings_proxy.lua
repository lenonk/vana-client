-- Suite settings proxy (one mod.toml, per-panel `prefix_*` keys).
--
-- Client rule: settings.get("key") only works for keys declared in this mod's mod.toml.
-- Undeclared keys error at runtime. Suite panels must not call settings.get for layout
-- or drag fields that live in hud_config storage — use cfg.get via hud_config instead.
--
-- Submodule visibility: mod_<panel> toggles gate loading; inside a panel, settings.get("on")
-- reads that toggle via this proxy (not a separate mod.toml "on" per panel).
--
-- Manifest keys use prefix_field (e.g. target_width). LAYOUT_KEYS below are intercepted
-- and return nil so legacy settings.get("offset_x") paths do not hit undeclared keys;
-- values come from hud_config schema defaults or storage.account.xiv_ui_<panel>_v1.
local suite_settings = settings

local LAYOUT_KEYS = {
  offset_x = true,
  offset_y = true,
  place_x = true,
  place_y = true,
  last_x = true,
  last_y = true,
}

local mod_toggle = {
  target = "mod_target",
  focus = "mod_focus",
  cast = "mod_cast",
  player = "mod_player",
  buff = "mod_buff",
  exp = "mod_exp",
  party = "mod_party",
  notify = "mod_notify",
}

local function proxy(prefix)
  local p = {}
  function p.get(key)
    if key == "on" then
      local toggle = mod_toggle[prefix]
      return toggle and suite_settings.get(toggle) == true
    end
    local toggle = mod_toggle[prefix]
    if toggle and suite_settings.get(toggle) ~= true then return nil end
    if LAYOUT_KEYS[key] then return nil end
    return suite_settings.get(prefix .. "_" .. key)
  end
  return p
end

return { proxy = proxy }