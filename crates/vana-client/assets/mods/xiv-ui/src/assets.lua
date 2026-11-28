-- Resolve img/ paths: suite `img/` or `modules/<panel>/img/`.
local M = {}

local function copy_spec(spec)
  local c = {}
  for k, v in pairs(spec) do c[k] = v end
  return c
end

function M.resolve_image_path(path, opts)
  opts = opts or {}
  if type(path) ~= "string" then return path end
  if path:find("^modules/", 1) then return path end
  if path:sub(1, 4) ~= "img/" then
    if opts.root then return opts.root .. path end
    return path
  end
  if fs and fs.exists and fs.exists(path) then return path end
  if opts.root then return opts.root .. path end
  return path
end

function M.prefix_path(root, path)
  return M.resolve_image_path(path, { root = root })
end

local function renderer_proto(X)
  if type(X) ~= "table" or type(X.renderer) ~= "function" then return nil end
  local sample = X.renderer()
  local mt = sample and getmetatable(sample)
  if type(mt) ~= "table" then return nil end
  if type(mt.put) == "function" then return mt end
  if type(mt.__index) == "table" and type(mt.__index.put) == "function" then return mt.__index end
  return nil
end

function M.patch_xiv_module(X, root)
  if type(X) ~= "table" then return end
  X.__xiv_ui_asset_root = root
  if type(X.exists) == "function" then
    local old_exists = X.exists
    function X.exists(path)
      local resolved = M.resolve_image_path(path, { root = root })
      if fs and fs.exists and fs.exists(resolved) then return true end
      return old_exists(resolved)
    end
  end
  local R = renderer_proto(X)
  if not R or R.__xiv_ui_asset_put then return end
  R.__xiv_ui_asset_put = true
  local orig_put = R.put
  function R.put(self, kind, id, spec)
    if kind == "image" and type(spec) == "table" and type(spec.path) == "string" then
      spec = copy_spec(spec)
      spec.path = M.resolve_image_path(spec.path, { root = root })
    end
    return orig_put(self, kind, id, spec)
  end
end

local roots_key = "__xiv_ui_asset_roots"
local id_roots = {
  tb_ = "modules/target/",
  xtb_ = "modules/target/",
  xft_ = "modules/focus/",
  xcb_ = "modules/cast/",
  pb_ = "modules/player/",
  xxp_ = "modules/exp/",
  xpl_ = "modules/party/",
}

function M.register_root(root)
  local roots = rawget(_G, roots_key) or {}
  rawset(_G, roots_key, roots)
  for _, r in ipairs(roots) do
    if r == root then return end
  end
  roots[#roots + 1] = root
end

function M.resolve_path(path, hud_id)
  if type(path) ~= "string" or path:find("^modules/", 1) then return path end
  if type(hud_id) == "string" then
    if hud_id:find("^bb", 1) then
      local resolved = M.resolve_image_path(path, { root = "modules/buff/" })
      if fs and fs.exists and fs.exists(resolved) then return resolved end
      return M.prefix_path("modules/buff/", path)
    end
    for prefix, root in pairs(id_roots) do
      if hud_id:find("^" .. prefix, 1) then
        local resolved = M.resolve_image_path(path, { root = root })
        if fs and fs.exists and fs.exists(resolved) then return resolved end
        return M.prefix_path(root, path)
      end
    end
  end
  local roots = rawget(_G, roots_key) or {}
  if fs and fs.exists and path:sub(1, 4) == "img/" then
    local resolved = M.resolve_image_path(path, { root = #roots == 1 and roots[1] or nil })
    if fs.exists(resolved) then return resolved end
    for i = #roots, 1, -1 do
      resolved = M.resolve_image_path(path, { root = roots[i] })
      if fs.exists(resolved) then return resolved end
    end
  end
  if fs and fs.exists then
    for i = #roots, 1, -1 do
      local full = roots[i] .. path
      if fs.exists(full) then return full end
    end
  end
  if #roots == 1 then return roots[1] .. path end
  if #roots > 0 then return roots[#roots] .. path end
  return path
end

return M