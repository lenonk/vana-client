-- Hotbars: a sticky macro is in every job's library (sticky.json, for every world), any job's slots name it, and
-- unstuck it goes back to the job's library, with a copy for each other job using it.

local files = {}
local data = require("src.data")
data.init({ read = function(f) return files[f] end, write = function(f, t) files[f] = t end, decode = json.decode,
  encode = json.encode })

local warp = data.save_entry("RDM", nil, { name = "Warp", lines = { '/item "Warp Ring" <me>' } })
data.assign("RDM", nil, 1, 9, warp)
local s = data.set_sticky("RDM", warp, true)
assert(s == "s1" and data.is_sticky(s) and not data.is_sticky(warp))
assert(data.get_raw("RDM", nil, 1, 9).ref == s and data.resolve("RDM", nil, 1, 9).name == "Warp")
local lib = data.library("RDM")
assert(#lib == 1 and lib[1].id == s and lib[1].sticky)
assert(data.library("WAR")[1].id == s and data.macro_of("WAR", s).name == "Warp")
data.assign("WAR", "NIN", 2, 1, s)
assert(data.uses("RDM", s) == 2 and data.uses("WAR", s) == 2)
assert(files["sticky.json"]:find("Warp"))

-- The job's own entries come first; a new sticky entry, kept sticky through an edit.
data.save_entry("RDM", nil, { name = "Cure", lines = { "/ma Cure <t>" } })
local choco = data.save_entry("RDM", nil, { name = "Choco", lines = { "/mount Chocobo" } }, true)
assert(choco == "s2")
data.save_entry("RDM", choco, { name = "Chocobo", lines = { "/mount Chocobo" } })
lib = data.library("RDM")
assert(#lib == 3 and lib[1].entry.name == "Cure" and lib[2].id == s and lib[3].entry.name == "Chocobo")

-- Read back, and in another world's file.
data.load()
assert(data.library("BLM")[1].id == s and data.library("BLM")[2].id == choco)
assert(data.set_world("Other"))
assert(data.library("PLD")[2].entry.name == "Chocobo")
data.set_world(nil)

-- Deleted: gone from every job's slots, and its id is not given out again.
data.assign("WHM", nil, 1, 1, choco)
data.delete_entry("RDM", choco)
assert(data.get_raw("WHM", nil, 1, 1) == nil and #data.library("WHM") == 1)
assert(data.save_entry("RDM", nil, { name = "Next", lines = { "/echo" } }, true) == "s3")
data.delete_entry("RDM", "s3")

-- Unstuck from RDM: RDM and WAR each get a copy their slots name; it leaves the other jobs' libraries.
local back = data.set_sticky("RDM", s, false)
assert(not data.is_sticky(back) and data.get_entry("RDM", back).name == "Warp")
assert(data.get_raw("RDM", nil, 1, 9).ref == back)
local wref = data.get_raw("WAR", "NIN", 2, 1).ref
assert(not data.is_sticky(wref) and data.get_entry("WAR", wref).name == "Warp")
assert(#data.library("BLM") == 0 and data.get_entry("RDM", s) == nil)

-- Saving with the switch moves it too.
local again = data.save_entry("RDM", back, { name = "Warp", lines = { '/item "Warp Ring" <me>' } }, true)
assert(data.is_sticky(again) and data.get_raw("RDM", nil, 1, 9).ref == again)

-- Another client on the same install made s9 after this one loaded: a change here starts from the file, so the next
-- id is new and s9 stays.
local other = json.decode(files["sticky.json"])
other.library.s9 = { name = "Theirs", lines = { "/echo theirs" } }
other.next_id = 10
files["sticky.json"] = json.encode(other)
assert(data.save_entry("RDM", nil, { name = "Mine", lines = { "/echo mine" } }, true) == "s10")
assert(json.decode(files["sticky.json"]).library.s9.name == "Theirs" and data.get_entry("WAR", "s9").name == "Theirs")

-- Every sticky change holds the lock around its read and write.
local locked, inside = 0, false
data.init({ read = function(f) return files[f] end, decode = json.decode, encode = json.encode,
  write = function(f, t) assert(f ~= "sticky.json" or inside); files[f] = t end,
  lock = function(path, f)
    assert(path == "sticky.json.lock" and not inside)
    locked, inside = locked + 1, true
    local r = f()
    inside = false
    return r
  end })
local id = data.save_entry("RDM", nil, { name = "Locked", lines = { "/echo" } }, true)
data.save_entry("RDM", id, { name = "Locked 2", lines = { "/echo" } })
data.set_sticky("RDM", id, false)
assert(locked == 3)
