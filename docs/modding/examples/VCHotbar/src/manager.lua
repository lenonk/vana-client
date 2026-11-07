-- The macro manager window.
--
--   Slots  pick which set to edit (a main job, optionally with a sub job), see every
--          slot of a bar, and choose one to edit, clear or run.
--   Macro  name, button text and lines of the chosen slot.
--   Keys   the hotkey modifier of each bar and the key row (changes apply at once).
--
-- A set is "RDM" alone (loads whenever RDM is main) or "RDM" + "WHM" (loads when RDM/WHM
-- and overrides RDM's slots one by one).

local data = require("src.data")
local jobs = require("src.jobs")
local keys = require("src.keys")
local runner = require("src.runner")

local M = {}

-- Set by init.lua: changed() redraws the bars, rebind() rebinds the hotkeys.
M.hooks = {}

local WIN = "vch_mgr"
local TAB_SLOTS, TAB_MACRO, TAB_KEYS = 1, 2, 3

local S = { mj = nil, sj = "", bar = 1, slot = 1 }
local defined = false

local DIM = { 150, 150, 160 }
local SUB_COLOUR = { 130, 180, 235 }

local function say(msg)
  chat.add("VCHotbar: " .. msg)
end

local function slots_count()
  return math.min(settings.get("slots"), data.MAX_SLOTS)
end

local function bars_count()
  return math.min(settings.get("bars"), data.MAX_BARS)
end

local function sub_arg()
  if S.sj ~= "" then
    return S.sj
  end
  return nil
end

local function numbers(n)
  local t = {}
  for i = 1, n do
    t[#t + 1] = tostring(i)
  end
  return t
end

-- ---------------------------------------------------------------------------
-- Refreshing what the window shows
-- ---------------------------------------------------------------------------

local function set_text()
  if not S.mj then
    return "Jobs are not loaded yet."
  end
  if S.sj ~= "" then
    return ("Editing %s with sub %s: overrides the %s set slot by slot."):format(S.mj, S.sj, S.mj)
  end
  return ("Editing %s main set: loads whenever %s is your main job."):format(S.mj, S.mj)
end

local function refresh_slots()
  ui.update(WIN, "ctx", { text = set_text() })
  if not S.mj then
    ui.update(WIN, "slots", { rows = {} })
    return
  end
  local eff = data.effective(S.mj, sub_arg())
  local rows = {}
  for i = 1, slots_count() do
    local cell = eff[S.bar] and eff[S.bar][i]
    local name, from, colour = "", "", DIM
    if cell then
      name = cell.macro.name or ""
      from = cell.from
      -- While editing a sub set, slots that still come from main are shown dim.
      colour = (S.sj ~= "" and cell.from == "main") and DIM or (cell.from == "sub" and SUB_COLOUR or { 255, 255, 255 })
    end
    rows[#rows + 1] = { keys.hint(S.bar, i), name, from, colour = colour }
  end
  ui.update(WIN, "slots", { rows = rows, at = S.slot })
end

local function refresh_keys()
  for b = 1, data.MAX_BARS do
    ui.update(WIN, "mod" .. b, { at = keys.index_of(keys.MOD_LABELS, keys.get_mod(b)) })
  end
  ui.update(WIN, "row", { at = keys.index_of(keys.ROW_LABELS, keys.get_row()) })
  local c = keys.conflicts()
  ui.update(WIN, "clash", {
    text = #c == 0 and "No key clashes." or ("Clash: " .. table.concat(c, "; ")),
    colour = #c == 0 and { 160, 200, 160 } or { 240, 120, 100 },
  })
end

local function refresh_all()
  ui.update(WIN, "bar", { items = numbers(bars_count()), at = math.min(S.bar, bars_count()) })
  ui.update(WIN, "mj", { text = S.mj or "" })
  ui.update(WIN, "sj", { text = S.sj })
  refresh_slots()
  refresh_keys()
end

-- ---------------------------------------------------------------------------
-- Editing a slot
-- ---------------------------------------------------------------------------

local function edit_slot()
  if not S.mj then
    return
  end
  local macro = data.resolve(S.mj, sub_arg(), S.bar, S.slot)
  ui.update(WIN, "where", {
    text = ("Bar %d, slot %d (%s) of %s"):format(S.bar, S.slot, keys.hint(S.bar, S.slot),
      S.sj ~= "" and (S.mj .. " + sub " .. S.sj) or (S.mj .. " main")),
  })
  ui.update(WIN, "name", { text = macro and macro.name or "" })
  ui.update(WIN, "label", { text = macro and macro.label or "" })
  ui.update(WIN, "lines", { text = macro and data.join_lines(macro.lines) or "" })
  ui.update(WIN, "tabs", { at = TAB_MACRO })
end

local function save_macro()
  if not S.mj then
    return
  end
  local lines = data.split_lines(ui.get(WIN, "lines").text)
  if #lines == 0 then
    say("add at least one line to the macro.")
    return
  end
  local name = data.trim(ui.get(WIN, "name").text)
  if name == "" then
    name = lines[1]:sub(1, 24)
  end
  data.set_macro(S.mj, sub_arg(), S.bar, S.slot, {
    name = name,
    label = ui.get(WIN, "label").text,
    lines = lines,
  })
  refresh_slots()
  M.hooks.changed()
  ui.update(WIN, "tabs", { at = TAB_SLOTS })
end

local function clear_slot()
  if not S.mj then
    return
  end
  data.clear(S.mj, sub_arg(), S.bar, S.slot)
  refresh_slots()
  M.hooks.changed()
end

local function run_slot()
  if not S.mj then
    return
  end
  local macro = data.resolve(S.mj, sub_arg(), S.bar, S.slot)
  if macro then
    runner.run_checked(macro)
  end
end

-- ---------------------------------------------------------------------------
-- The window
-- ---------------------------------------------------------------------------

local function define()
  ui.window(WIN, {
    title = "VCHotbar",
    width = 480,
    help = "Left-click a slot on the bar to fire it, right-click to edit it.",
    elements = {
      { type = "tabs", id = "tabs", items = { "Slots", "Macro", "Keys" }, at = 1 },

      -- Slots
      { type = "text", id = "ctx", text = "", page = TAB_SLOTS },
      { type = "input", id = "mj", label = "Main job", text = "", max = 3, page = TAB_SLOTS,
        tip = "The job you play as main, e.g. RDM." },
      { type = "input", id = "sj", label = "Sub job (blank = the main job's own set)", text = "", max = 3,
        page = TAB_SLOTS, tip = "Sub job, e.g. WHM: its slots load when that is your sub and override the main set." },
      { type = "choice", id = "bar", label = "Bar", items = { "1" }, at = 1, page = TAB_SLOTS },
      { type = "list", id = "slots", rows = {}, height = 12, page = TAB_SLOTS,
        columns = { { title = "Key", width = 50 }, { title = "Macro" }, { title = "From", width = 56 } },
        tip = "Enter or click a slot to edit its macro." },
      { type = "buttons", id = "sbtn", items = { "Edit", "Clear", "Run" }, page = TAB_SLOTS },

      -- Macro
      { type = "text", id = "where", text = "", page = TAB_MACRO },
      { type = "input", id = "name", label = "Name", text = "", max = 32, page = TAB_MACRO },
      { type = "input", id = "label", label = "Button text (blank = from the name)", text = "", max = 14,
        page = TAB_MACRO },
      { type = "input", id = "lines", label = "Lines (separate with |)", text = "", max = 1024, page = TAB_MACRO,
        tip = 'Example: /ma "Cure" <t> | /wait 2 | /echo done' },
      { type = "text", id = "tip", text = "Waits: /wait 2 on its own line, or <wait 2> at the end of one.",
        dim = true, page = TAB_MACRO },
      { type = "buttons", id = "ebtn", items = { "Save", "Cancel" }, page = TAB_MACRO },

      -- Keys
      { type = "text", id = "ktext", text = "Hotkey modifier for each bar:", page = TAB_KEYS },
      { type = "choice", id = "mod1", label = "Bar 1", items = keys.MOD_LABELS, at = 1, page = TAB_KEYS },
      { type = "choice", id = "mod2", label = "Bar 2", items = keys.MOD_LABELS, at = 1, page = TAB_KEYS },
      { type = "choice", id = "mod3", label = "Bar 3", items = keys.MOD_LABELS, at = 1, page = TAB_KEYS },
      { type = "choice", id = "row", label = "Keys", items = keys.ROW_LABELS, at = 1, page = TAB_KEYS },
      { type = "text", id = "clash", text = "", page = TAB_KEYS },
      { type = "buttons", id = "kbtn", items = { "Reset to defaults" }, page = TAB_KEYS },
    },
  })

  ui.on(WIN, function(_, e)
    if e.kind == "set" then
      if e.id == "bar" then
        S.bar = e.index
        refresh_slots()
      elseif e.id:match("^mod%d$") then
        keys.set_mod(tonumber(e.id:sub(4)), keys.MOD_LABELS[e.index])
        M.hooks.rebind()
        M.hooks.changed()
        refresh_keys()
        refresh_slots()
      elseif e.id == "row" then
        keys.set_row(keys.ROW_LABELS[e.index])
        M.hooks.rebind()
        M.hooks.changed()
        refresh_keys()
        refresh_slots()
      end
    elseif e.kind == "change" and (e.id == "mj" or e.id == "sj") then
      local text = data.trim(e.text)
      if text == "" and e.id == "sj" then
        S.sj = ""
      else
        local job = jobs.parse(text)
        if job then
          S[e.id] = job
        else
          say(('"%s" is not a job (use the three-letter name, e.g. RDM).'):format(text))
        end
      end
      refresh_all()
    elseif e.kind == "select" and e.id == "slots" then
      S.slot = e.index
      edit_slot()
    elseif e.kind == "press" then
      if e.id == "sbtn" then
        S.slot = ui.get(WIN, "slots").at
        if e.index == 1 then
          edit_slot()
        elseif e.index == 2 then
          clear_slot()
        else
          run_slot()
        end
      elseif e.id == "ebtn" then
        if e.index == 1 then
          save_macro()
        else
          ui.update(WIN, "tabs", { at = TAB_SLOTS })
        end
      elseif e.id == "kbtn" then
        keys.reset()
        M.hooks.rebind()
        M.hooks.changed()
        refresh_keys()
        refresh_slots()
      end
    end
  end)
  defined = true
end

local function ensure()
  if not defined then
    define()
  end
end

-- Points the manager at the current jobs (main set).
local function reset_to_current()
  local main = jobs.current()
  S.mj = main or S.mj
  S.sj = ""
  S.bar = math.min(S.bar, bars_count())
  S.slot = math.min(S.slot, slots_count())
end

function M.open()
  ensure()
  if not S.mj then
    reset_to_current()
  end
  refresh_all()
  ui.update(WIN, "tabs", { at = TAB_SLOTS })
  ui.open(WIN)
end

function M.toggle()
  ensure()
  if ui.is_open(WIN) then
    ui.close(WIN)
  else
    M.open()
  end
end

-- Opens the editor on one slot of the current jobs; `from` is "sub" when the slot's
-- macro comes from the sub job's set (so that is the set to edit).
function M.edit_slot(bar, slot, from)
  ensure()
  local main, sub = jobs.current()
  S.mj = main or S.mj
  S.sj = (from == "sub" and sub) or ""
  S.bar = bar
  S.slot = slot
  refresh_all()
  edit_slot()
  ui.open(WIN)
end

-- Redraws the window's lists after the data changed from outside (//hb reload).
function M.refresh()
  if defined then
    refresh_all()
  end
end

return M
