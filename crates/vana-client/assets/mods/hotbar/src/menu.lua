-- A slot's right-click menu: the slot's macro, then its actions.

local data = require("src.data")
local actions = require("src.actions")

local M = {}

local WIN = "slotmenu"
local GOLD = { 232, 210, 156 }
local DIM = { 160, 162, 172 }

-- items: { { label, fn }, ... }; `where` names the slot ("Bar 1, Slot 2", "LT A").
function M.open(title, macro, where, items)
  local act = macro and actions.of(macro)
  local rows = {}
  for n, it in ipairs(items) do rows[n] = { it[1] } end
  ui.window(WIN, {
    title = title,
    width = 270,
    popup = true,
    elements = {
      { type = "list", id = "head", height = 1, rows = { {
        macro and data.name_of(macro) or "Empty slot", where, icon = act and { kind = act.kind, id = act.id } or nil,
        colour = macro and GOLD or DIM } }, columns = { {}, { width = 70, align = "right" } } },
      { type = "separator" },
      { type = "list", id = "acts", rows = rows, height = #rows },
    },
  })
  ui.on(WIN, function(_, e)
    if e.kind ~= "select" or e.id ~= "acts" then return end
    ui.close(WIN)
    items[e.index][2]()
  end)
  ui.open(WIN)
end

return M
