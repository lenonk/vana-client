-- Item stats from the item's description and augment lines, as the item window shows them: "DEF:45 STR+5
-- Accuracy+10 Haste+3%". A "Word:" prefix starts a context for what follows: a pet's (Pet:, Avatar:, Wyvern:,
-- Automaton:, Luopan:) goes to that pet, any other (Set:, Latent effect:, weather, days, Unity Ranking...) is set
-- aside as conditional. Each signed number takes the longest known phrase that ends just before it.

local M = {}

-- Phrase (lowercase, no quotes, single spaces) -> stat key. A phrase with a trailing "%" matches only a percentage.
-- Key "_" = known and not scored (crafting, experience bonuses...).
M.ALIAS = {
  -- Attributes and base
  ["hp"] = "hp", ["mp"] = "mp", ["hp%"] = "hpp", ["mp%"] = "mpp", ["str"] = "str", ["dex"] = "dex", ["vit"] = "vit",
  ["agi"] = "agi", ["int"] = "int", ["mnd"] = "mnd", ["chr"] = "chr", ["def"] = "def", ["dmg"] = "dmg", ["delay"] = "delay",
  ["all attributes"] = "allattr", ["all stats"] = "allattr", ["lv."] = "_", ["lv"] = "_",
  -- The element glyphs (U+E000..E007, Fire .. Dark): resistance to that element.
  ["\u{E000}"] = "res_fire", ["\u{E001}"] = "res_ice", ["\u{E002}"] = "res_wind", ["\u{E003}"] = "res_earth",
  ["\u{E004}"] = "res_thunder", ["\u{E005}"] = "res_water", ["\u{E006}"] = "res_light", ["\u{E007}"] = "res_dark",
  -- Physical offence
  ["accuracy"] = "acc", ["acc."] = "acc", ["acc"] = "acc", ["attack"] = "att", ["atk."] = "att", ["att."] = "att",
  ["attack%"] = "attp", ["ranged accuracy"] = "racc", ["ranged acc."] = "racc", ["rng. acc."] = "racc",
  ["rng.acc."] = "racc", ["ranged attack"] = "ratt", ["rng. atk."] = "ratt", ["rng.atk."] = "ratt",
  ["ranged atk."] = "ratt", ["store tp"] = "stp", ["double attack%"] = "da", ["double attack"] = "da",
  ["dbl.atk.%"] = "da", ["triple attack%"] = "ta", ["triple attack"] = "ta", ["quadruple attack%"] = "qa",
  ["quadruple attack"] = "qa", ["critical hit rate%"] = "crit", ["critical hit rate"] = "crit",
  ["crit. hit rate%"] = "crit", ["critical hit damage%"] = "critdmg", ["crit. hit damage%"] = "critdmg",
  ["weapon skill damage%"] = "wsd", ["weapon skill damage"] = "wsd", ["weapon skill accuracy"] = "wsacc",
  ["haste%"] = "haste", ["haste"] = "haste", ["dual wield"] = "dw", ["dual wield%"] = "dw", ["subtle blow"] = "sb",
  ["subtle blow ii"] = "sb2", ["tp bonus"] = "tpb", ["skillchain bonus"] = "scb", ["zanshin"] = "zanshin",
  ["martial arts"] = "ma", ["kick attacks"] = "kick", ["kick attacks attack"] = "kickatt", ["counter"] = "counter",
  ["counter%"] = "counter", ["regain"] = "regain", ["conserve tp"] = "ctp", ["fencer"] = "fencer",
  ["double attack damage"] = "dadmg", ["triple attack damage"] = "tadmg", ["physical damage limit%"] = "pdl",
  ["occult acumen"] = "occult", ["killer effects"] = "killer", ["counter damage"] = "counterdmg",
  ["counter attack"] = "counteratt", ["counter critical hit rate%"] = "_", ["save tp"] = "savetp",
  ["converts damage taken to tp"] = "_",
  -- Defence
  ["evasion"] = "eva", ["eva."] = "eva", ["magic evasion"] = "meva", ["mag. eva."] = "meva", ["magic def. bonus"] = "mdb",
  ["m. def. b."] = "mdb", ["damage taken%"] = "dt", ["physical damage taken%"] = "pdt", ["physical damage taken ii%"] = "pdt2",
  ["magic damage taken%"] = "mdt", ["magic damage taken ii%"] = "mdt2", ["breath damage taken%"] = "bdt",
  ["enmity"] = "enmity", ["regen"] = "regen", ["refresh"] = "refresh", ["movement speed%"] = "move",
  ["chance of successful block"] = "block", ["chance of successful block%"] = "block", ["shield def. bonus"] = "shielddef",
  ["spell interruption rate down%"] = "sird", ["spell interruption rate down"] = "sird", ["tactical parry"] = "tparry",
  ["resist all elements"] = "resall", ["resistance to all status ailments"] = "resstatus", ["phalanx received"] = "_",
  ["phalanx"] = "phalanx", ["regen potency%"] = "regenpot", ["refresh potency"] = "refreshpot",
  ["potency of cure effect received%"] = "curercv", ["potency of cure effects received%"] = "curercv",
  ["enemy critical hit rate%"] = "enemycrit", ["synergy damage taken%"] = "_", ["breath damage dealt%"] = "_",
  -- Magic
  ["magic accuracy"] = "macc", ["mag. acc."] = "macc", ["magic acc."] = "macc", ["magic atk. bonus"] = "mab",
  ["magic attack bonus"] = "mab", ["magic damage"] = "mdmg", ["magic burst damage"] = "mbb",
  ["magic burst damage ii"] = "mbb2", ["magic critical hit rate%"] = "mcrit", ["magic critical hit damage%"] = "mcritdmg",
  ["fast cast%"] = "fc", ["fast cast"] = "fc", ["cure potency%"] = "cure", ["cure potency ii%"] = "cure2",
  ["cure spellcasting time%"] = "curect", ["conserve mp"] = "cmp", ["enhancing magic duration%"] = "enhdur",
  ["enhancing magic effect duration%"] = "enhdur", ["enfeebling magic effect"] = "enfeffect",
  ["drain and aspir potency"] = "drain", ["elemental magic casting time%"] = "elemct",
  ["elemental magic recast delay%"] = "_", ["magic accuracy skill"] = "maccskill", ["mp recovered while healing"] = "hmp",
  ["hp recovered while healing"] = "hhp", ["magic damage%"] = "mdmg", ["healing magic casting time%"] = "_",
  ["enhancing magic casting time%"] = "_", ["enfeebling magic casting time%"] = "_", ["dark magic casting time%"] = "_",
  ["utsusemi spellcasting time%"] = "_", ["blue magic spellcasting time%"] = "_", ["blue magic recast delay%"] = "_",
  ["sword enhancement spell damage"] = "swordenh", ["ninjutsu damage"] = "nindmg", ["slow%"] = "_",
  ["potency of banish vs. undead"] = "_", ["elemental resistance spells"] = "_", ["divine veil%"] = "_",
  ["occ. quickens spellcasting%"] = "quickcast", ["sublimation"] = "sublimation",
  -- Magic skills
  ["healing magic skill"] = "healing", ["divine magic skill"] = "divine", ["enhancing magic skill"] = "enhancing",
  ["enfeebling magic skill"] = "enfeebling", ["elemental magic skill"] = "elemental", ["dark magic skill"] = "dark",
  ["summoning magic skill"] = "summoning", ["ninjutsu skill"] = "ninjutsu", ["singing skill"] = "singing",
  ["string instrument skill"] = "string", ["wind instrument skill"] = "wind", ["blue magic skill"] = "blue",
  ["geomancy skill"] = "geomancy", ["handbell skill"] = "handbell", ["magic skills"] = "allmagic",
  -- Combat skills
  ["hand-to-hand skill"] = "sk_h2h", ["dagger skill"] = "sk_dagger", ["sword skill"] = "sk_sword",
  ["great sword skill"] = "sk_gsword", ["axe skill"] = "sk_axe", ["great axe skill"] = "sk_gaxe",
  ["scythe skill"] = "sk_scythe", ["polearm skill"] = "sk_polearm", ["katana skill"] = "sk_katana",
  ["great katana skill"] = "sk_gkatana", ["club skill"] = "sk_club", ["staff skill"] = "sk_staff",
  ["archery skill"] = "sk_archery", ["marksmanship skill"] = "sk_marks", ["throwing skill"] = "sk_throw",
  ["evasion skill"] = "sk_evasion", ["parrying skill"] = "sk_parry", ["guarding skill"] = "sk_guard",
  ["shield skill"] = "sk_shield", ["combat skills"] = "allcombat",
  -- Job traits and abilities
  ["treasure hunter"] = "th", ["snapshot"] = "snapshot", ["rapid shot"] = "rapid", ["waltz potency%"] = "waltz",
  ["potency of waltz effects received%"] = "_", ["song spellcasting time%"] = "songct", ["song recast delay"] = "songrecast",
  ["song effect duration%"] = "songdur", ["all songs"] = "allsongs", ["blood pact ability delay"] = "bpdelay",
  ["blood pact ability delay ii"] = "bpdelay2", ["blood pact damage"] = "bpdmg", ["blood pact recast time ii"] = "bpdelay2",
  ["avatar perpetuation cost"] = "perp", ["blood boon"] = "bloodboon", ["phantom roll effect duration"] = "rolldur",
  ["quick draw"] = "qd", ["steal"] = "steal", ["despoil"] = "despoil", ["charm"] = "charm", ["reward"] = "reward",
  ["tame success rate"] = "tame", ["recycle"] = "recycle", ["barrage"] = "barrage", ["true shot"] = "trueshot",
  ["double shot%"] = "doubleshot", ["double shot damage"] = "_", ["triple shot%"] = "tripleshot",
  ["velocity shot%"] = "_", ["sharpshot"] = "_", ["jig duration%"] = "jigdur", ["samba duration"] = "sambadur",
  ["step accuracy"] = "stepacc", ["step tp consumed"] = "_", ["violent flourish accuracy"] = "_",
  ["weapon bash"] = "weaponbash", ["shield bash"] = "shieldbash", ["souleater"] = "souleater",
  ["berserk effect duration"] = "_", ["berserk duration"] = "_", ["aggressor duration"] = "_", ["meditate duration"] = "meditate",
  ["rampart duration"] = "_", ["liement duration"] = "_", ["regen effect duration"] = "regendur",
  ["indicolure spell duration"] = "indidur", ["chain affinity"] = "_", ["burst affinity"] = "_", ["mug"] = "_",
  ["counterstance"] = "_", ["sentinel"] = "_", ["last resort"] = "_", ["footwork%"] = "_", ["mana wall%"] = "_",
  ["migawari"] = "_", ["utsusemi"] = "_", ["daken"] = "_", ["inquartata"] = "_", ["retaliation"] = "_",
  ["hasso"] = "_", ["spur"] = "_", ["spirit link"] = "_", ["stout servant"] = "_", ["call beast"] = "_",
  ["life cycle"] = "_", ["elemental siphon"] = "_", ["aquaveil"] = "_", ["cursna"] = "cursna", ["afflatus solace"] = "_",
  ["avatar's favor effect"] = "_", ["random deal effect"] = "_", ["overload rate"] = "_", ["shield mastery"] = "_",
  ["swordplay"] = "_", ["sneak attack"] = "_", ["critical parry"] = "_", ["chakra iii"] = "_", ["ninja tool expertise"] = "_",
  ["divine veil"] = "_", ["fast cast enmity"] = "_", ["resist charm"] = "_", ["resist gravity"] = "_",
  ["resist amnesia"] = "_", ["resist silence"] = "_", ["resist petrify"] = "_", ["resist stun"] = "_",
  ["resist sleep"] = "_", ["resist poison"] = "_", ["resist paralyze"] = "_", ["resist blind"] = "_",
  ["resist virus"] = "_", ["resist curse"] = "_", ["resist slow"] = "_", ["resist bind"] = "_",
  ["reduces distance knocked back"] = "_", ["monster correlation effects"] = "_",
  ["breath attacks"] = "breath", ["song casting time"] = "songct", ["regen duration"] = "regendur",
  ["all status ailment resistance"] = "resstatus", ["phantom roll effects"] = "roll", ["phantom roll"] = "roll",
  ["geomancy"] = "geomancy_pot", ["waltz potency received"] = "_", ["enemy tp accumulation"] = "_",
  ["potency of cursna effects received"] = "_", ["maneuver effects"] = "_", ["magic skill gain rate"] = "_",
  ["resistance to current avatar's element"] = "_", ["duration of refresh effects received"] = "_",
  ["increases magic effect duration"] = "_", ["skill"] = "_", ["damage taken ii"] = "dt2", ["damage taken ii%"] = "dt2",
  ["magic defense"] = "mdb", ["res. all ele."] = "resall", ["all elemental resistances"] = "resall",
  ["all resistances"] = "resall", ["cure casting time"] = "curect", ["cure casting time%"] = "curect",
  ["enfeebling magic duration"] = "enfdur", ["enfeebling magic duration%"] = "enfdur", ["song duration"] = "songdur",
  ["song duration%"] = "songdur", ["chance of receiving critical hits"] = "enemycrit",
  ["minuet"] = "song_minuet", ["march"] = "song_march", ["madrigal"] = "song_madrigal", ["paeon"] = "song_paeon",
  ["ballad"] = "song_ballad", ["minne"] = "song_minne", ["mambo"] = "song_mambo", ["etude"] = "song_etude",
  ["carol"] = "song_carol", ["lullaby"] = "song_lullaby", ["requiem"] = "song_requiem", ["threnody"] = "song_threnody",
  ["scherzo"] = "song_scherzo", ["elegy"] = "song_elegy", ["prelude"] = "song_prelude", ["mazurka"] = "song_mazurka",
  ["hymnus"] = "song_hymnus", ["virelai"] = "song_virelai", ["nocturne"] = "song_nocturne", ["finale"] = "_",
  -- Rare ability and spell effects: not scored
  ["reward recast time"] = "_", ["reward recast time%"] = "_",
  ["camouflage duration"] = "_", ["camouflage duration%"] = "_",
  ["dread spikes potency"] = "_", ["dread spikes potency%"] = "_",
  ["absorb effect duration"] = "_", ["absorb effect duration%"] = "_",
  ["flee duration"] = "_", ["flee duration%"] = "_",
  ["despoil effect"] = "_", ["despoil effect%"] = "_",
  ["absorb-tp effect"] = "_", ["absorb-tp effect%"] = "_",
  ["warcry effect duration"] = "_", ["warcry effect duration%"] = "_",
  ["spikes spell damage"] = "_", ["spikes spell damage%"] = "_",
  ["hide duration"] = "_", ["hide duration%"] = "_",
  ["cover effect duration"] = "_", ["cover effect duration%"] = "_",
  ["valiance and vallation effect duration"] = "_", ["valiance and vallation effect duration%"] = "_",
  ["dodge effect"] = "_", ["dodge effect%"] = "_",
  ["gambit effect duration"] = "_", ["gambit effect duration%"] = "_",
  ["elemental magic status ailment effects"] = "_", ["elemental magic status ailment effects%"] = "_",
  ["damage dealt"] = "_", ["damage dealt%"] = "_",
  ["stoneskin casting time"] = "_", ["stoneskin casting time%"] = "_",
  ["chance to learn blue magic"] = "_", ["chance to learn blue magic%"] = "_",
  ["enfeebling magic spellcasting time"] = "_", ["enfeebling magic spellcasting time%"] = "_",
  ["triple shot damage"] = "_", ["triple shot damage%"] = "_",
  ["absorb potency"] = "_", ["absorb potency%"] = "_",
  ["sword enhancement spell duration"] = "_", ["sword enhancement spell duration%"] = "_",
  ["spinning slash damage"] = "_", ["spinning slash damage%"] = "_",
  ["indicolure effect duration"] = "_", ["indicolure effect duration%"] = "_",
  ["gain magic effect"] = "_", ["gain magic effect%"] = "_",
  ["repair potency"] = "_", ["repair potency%"] = "_",
  ["addle"] = "_", ["addle%"] = "_",
  ["defender effect duration"] = "_", ["defender effect duration%"] = "_",
  ["dark magic duration"] = "_", ["dark magic duration%"] = "_",
  ["cure mp consumed"] = "_", ["cure mp consumed%"] = "_",
  ["meditate effect duration"] = "_", ["meditate effect duration%"] = "_",
  ["elemental magic mp consumed"] = "_", ["elemental magic mp consumed%"] = "_",
  ["reduces enmity decrease when taking damage"] = "_", ["reduces enmity decrease when taking damage%"] = "_",
  ["boosts enmity retention when taking damage"] = "_", ["boosts enmity retention when taking damage%"] = "_",
  -- Crafting, gathering, experience: not scored
  ["fishing skill"] = "_", ["woodworking skill"] = "_", ["smithing skill"] = "_", ["goldsmithing skill"] = "_",
  ["clothcraft skill"] = "_", ["leathercraft skill"] = "_", ["bonecraft skill"] = "_", ["alchemy skill"] = "_",
  ["cooking skill"] = "_", ["synthesis skill"] = "_", ["experience point bonus:%"] = "_", ["capacity point bonus:%"] = "_",
  ["chocobo riding time"] = "_", ["gilfinder"] = "_", ["combat skill gain rate"] = "_", ["subskill"] = "_", ["synthesis speed"] = "_",
  ["dec. likelihood of synthesis material loss%"] = "_", ["dec. likelihood of synth. material loss%"] = "_",
  ["inc. rate of high-quality item synthesis success"] = "_", ["inc. rate of high-quality item synth. success"] = "_",
}

-- Context prefixes: pets (their stats kept apart), the rest conditional.
M.PETS = { ["pet"] = "pet", ["avatar"] = "avatar", ["wyvern"] = "wyvern", ["automaton"] = "automaton",
  ["luopan"] = "luopan", ["jug pet"] = "pet" }

local ELEMENT_WORDS = { fire = true, ice = true, wind = true, earth = true, lightning = true, thunder = true,
  water = true, light = true, dark = true }

-- Lowercase, quotes gone, single spaces, no space before % or after a line break.
function M.norm(s)
  s = s:lower():gsub('"', ""):gsub("%s+", " ")
  return (s:gsub("^ ", ""):gsub(" $", ""))
end

-- The longest phrase ending the text that ALIAS knows: key, phrase (nil when none).
local function lookup(text, pct)
  local words = {}
  for w in text:gmatch("%S+") do words[#words + 1] = w end
  for first = 1, #words do
    local name = table.concat(words, " ", first)
    local k = (pct and M.ALIAS[name .. "%"]) or M.ALIAS[name]
    if k then return k, name end
  end
  return nil
end

-- The context a prefix ending in ":" names: "pet:avatar", "cond" or nil (not a context: "DEF", "DMG", "Delay").
local BASE_COLON = { def = true, dmg = true, delay = true }
local function context_of(word)
  word = M.norm(word)
  if BASE_COLON[word] then return nil end
  if M.PETS[word] then return "pet:" .. M.PETS[word] end
  return "cond"
end

-- Adds one text's stats into r = {stats, pet = {kind = {key = n}}, cond = {lines}, unknown = {phrase = n}}.
local function scan(text, r)
  text = text:gsub("\n", " ")
  local ctx, ctx_once = nil, nil
  local pos = 1
  while pos <= #text do
    local s1, e1, sign, num, pct = text:find("([%+%-])%s*(%d+)(%%?)", pos)
    local s2, e2, num2 = text:find(":%s*(%d+)", pos)
    local s, e, value, is_pct
    if s2 and (not s1 or s2 < s1) then
      s, e, value, is_pct = s2, e2, tonumber(num2), false
    elseif s1 then
      s, e, value, is_pct = s1, e1, tonumber(num) * (sign == "-" and -1 or 1), pct == "%"
    else
      break
    end
    local before = text:sub(pos, s - 1)
    if s == s2 then
      -- "DEF:45": the word before the colon is the name; a context prefix before it still counts.
      local head, word = before:match("^(.-)([%a%.]+)%s*$")
      before = (head or "") .. " " .. (word or "")
    end
    -- A "Word:" in what came before starts a new context (the last one wins).
    for prefix in before:gmatch("([^:]+):") do
      local c = context_of(prefix:match("([%a%s%.'\"%-]+)$") or prefix)
      if c then ctx = c end
    end
    local tail = before:match("([^:]*)$") or before
    local name = M.norm(tail)
    local key = lookup(name, is_pct)
    -- "Water Elemental Magic Accuracy+30": one element's, set aside as conditional.
    local el = name:match("^(%a+) elemental ")
    if el and ELEMENT_WORDS[el] then ctx_once = "cond" end
    -- An unknown quoted name ("Boost"+5): that job ability's effect.
    if key == nil then
      local q = tail:match('"([^"]+)"%s*$')
      if q then key = "ja:" .. M.norm(q) end
    end
    if ctx == "cond" or ctx_once == "cond" then
      ctx_once = nil
      r.cond[#r.cond + 1] = name .. (value >= 0 and "+" or "") .. value .. (is_pct and "%" or "")
    elseif key == nil then
      if name ~= "" then r.unknown[name] = (r.unknown[name] or 0) + 1 end
    elseif key ~= "_" then
      local into = r.stats
      if ctx then
        local pet = ctx:sub(5)
        r.pet[pet] = r.pet[pet] or {}
        into = r.pet[pet]
      end
      if key == "allattr" then
        for _, a in ipairs({ "str", "dex", "vit", "agi", "int", "mnd", "chr" }) do into[a] = (into[a] or 0) + value end
      else
        into[key] = (into[key] or 0) + value
      end
    end
    pos = e + 1
  end
end

-- An item's stats from its description and augment lines (a list of strings, or nil).
function M.parse(description, augments)
  local r = { stats = {}, pet = {}, cond = {}, unknown = {} }
  scan(description or "", r)
  for _, line in ipairs(augments or {}) do scan(line, r) end
  return r
end

return M
