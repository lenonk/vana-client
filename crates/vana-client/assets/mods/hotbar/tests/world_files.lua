-- Hotbars: each world keeps its macros in macros-<World>.json, which starts from macros.json the first time.

local files = { ["macros.json"] = json.encode({ RUN = { library = { ["1"] = { name = "Old", lines = { "/p old" } } } } }) }
local data = require("src.data")
data.init({ read = function(f) return files[f] end, write = function(f, t) files[f] = t end, decode = json.decode,
  encode = json.encode })
assert(data.file() == "macros.json" and data.get_entry("RUN", "1").name == "Old", "macros.json until the world is known")

local switched, loaded = data.set_world("Asura")
assert(switched and loaded and data.file() == "macros-Asura.json")
assert(data.get_entry("RUN", "1").name == "Old", "a new world starts from macros.json")
data.save_entry("RUN", nil, { name = "Asura's", lines = { "/p a" } })
assert(files["macros-Asura.json"] and not files["macros.json"]:find("Asura's"), "saves go to the world's file")
assert(data.set_world("Asura") == false, "the same world: nothing to read")

data.set_world("Test Server!")
assert(data.file() == "macros-Test_Server_.json")
assert(#data.library("RUN") == 1, "another world has its own")

data.set_world("Asura")
assert(#data.library("RUN") == 2, "back on the first world, its macros")
