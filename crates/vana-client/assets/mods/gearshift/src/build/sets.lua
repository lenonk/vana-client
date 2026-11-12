-- The set builder's catalogue: every set (what it is for, the stats it values and their caps), each job's sets, the
-- weapon types a job fights with, which set an action wears, and the element gear the descriptions do not score
-- (staves, obis, gorgets and belts work by element, not by a number on the item).
-- Weights are points per stat point (per 1% for percentages); a negative weight values a negative stat (DT-5% scores).
-- Caps: past it a stat is worth nothing (a negative cap is a floor).

local M = {}

local DEF = { dt = -50, pdt = -50, mdt = -50 }
local function merge(a, b)
  local out = {}
  for k, v in pairs(a) do out[k] = v end
  for k, v in pairs(b or {}) do out[k] = v end
  return out
end

-- Shared weight groups.
local MELEE = { stp = 5, da = 7, ta = 10, qa = 12, haste = 8, acc = 1.0, att = 0.5, str = 0.3, dex = 0.6, crit = 2,
  dw = 3, zanshin = 2, allcombat = 0.9 }
local DT = { dt = -6, pdt = -5, mdt = -3, mdt2 = -3, pdt2 = -5, dt2 = -6, bdt = -1 }
local CASTER_IDLE = { refresh = 30, regen = 10, mp = 0.05, hp = 0.02, meva = 0.1, mdb = 1 }

M.SETS = {
  -- Standing, moving, resting
  Idle = { label = "Idle", when = "standing, not fighting", caps = DEF, weapons = true,
    weights = merge(DT, { refresh = 30, regen = 12, hp = 0.03, mp = 0.03, meva = 0.1, mdb = 1, def = 0.03, eva = 0.03 }) },
  Resting = { label = "Resting", when = "/heal", caps = DEF, weapons = true,
    weights = { hmp = 10, hhp = 4, refresh = 15, regen = 4, mp = 0.02 } },
  Movement = { label = "Movement", when = "running", weights = { move = 10 } },
  PDT = { label = "Physical defence", when = "the PDT toggle", caps = DEF,
    weights = { dt = -6, pdt = -6, pdt2 = -6, dt2 = -6, hp = 0.05, def = 0.05, eva = 0.05, block = 2 } },
  MDT = { label = "Magic defence", when = "the MDT toggle", caps = DEF,
    weights = { dt = -6, mdt = -6, mdt2 = -6, dt2 = -6, hp = 0.05, meva = 0.2, mdb = 2, resall = 0.5 } },
  -- Melee
  TP = { label = "TP", when = "engaged", caps = { haste = 25 }, weights = MELEE },
  TP_Hybrid = { label = "TP - hybrid", when = "engaged, the hybrid toggle", caps = merge(DEF, { haste = 25 }),
    weights = merge(MELEE, { dt = -5, pdt = -4, mdt = -2 }) },
  TP_Acc = { label = "TP - accuracy", when = "engaged, the accuracy toggle", caps = { haste = 25 },
    weights = merge(MELEE, { acc = 2.5, dex = 1, agi = 0.2 }) },
  TP_MNK = { label = "TP - martial arts", name = "TP", when = "engaged", caps = { haste = 25 },
    weights = merge(MELEE, { ma = 3, kick = 3, counter = 1.5, sk_h2h = 0.9 }) },
  TP_Pet = { label = "TP - with pet", when = "engaged with a pet out", caps = { haste = 25 },
    weights = merge(MELEE, { ["pet.acc"] = 0.6, ["pet.att"] = 0.4, ["pet.haste"] = 4 }) },
  WS = { label = "Weapon skill", when = "any weapon skill without its set",
    weights = { wsd = 8, att = 0.6, acc = 0.6, str = 0.8, dex = 0.6, tpb = 0.02, crit = 1, da = 2, ta = 3 } },
  Waltz = { label = "Waltz", when = "Curing Waltz", weights = { waltz = 10, chr = 1, vit = 0.5 } },
  Steps = { label = "Steps", when = "a step", weights = { stepacc = 3, acc = 2, haste = 2 } },
  Enmity = { label = "Enmity", when = "Provoke, Flash and other enmity actions", caps = DEF,
    weights = merge(DT, { enmity = 10, hp = 0.05 }) },
  -- Ranged
  Preshot = { label = "Ranged - preshot", when = "before a ranged attack", weights = { snapshot = 10, rapid = 6 } },
  Midshot = { label = "Ranged - midshot", when = "during a ranged attack",
    weights = { racc = 1.2, ratt = 1, agi = 0.8, stp = 4, crit = 2, doubleshot = 5, tripleshot = 7, trueshot = 2,
      sk_archery = 1, sk_marks = 1, sk_throw = 1 } },
  RangedWS = { label = "Ranged weapon skill", when = "a ranged weapon skill without its set",
    weights = { wsd = 8, ratt = 1, racc = 1, agi = 1, str = 0.5, tpb = 0.02 } },
  -- Casting
  Precast = { label = "Fast Cast", when = "before every spell", caps = { fc = 80 }, weights = { fc = 10, quickcast = 3 } },
  Precast_Cure = { label = "Fast Cast - Cure", when = "before Cure", caps = { fc = 80 },
    weights = { fc = 10, curect = 10, quickcast = 3 } },
  Precast_Song = { label = "Fast Cast - songs", when = "before a song", caps = { fc = 80 },
    weights = { fc = 10, songct = 8, quickcast = 3 } },
  Cure = { label = "Cure", when = "Cure, Curaga", caps = { cure = 50 },
    weights = { cure = 10, cure2 = 10, mnd = 1, vit = 0.3, healing = 0.5, cmp = 1, enmity = -0.5, sird = 1 } },
  Healing = { label = "Healing magic", when = "Raise, Erase and other healing magic", weights = { healing = 2, fc = 3, haste = 3 } },
  NaSpells = { label = "Status removal", when = "-na spells, Erase", weights = { healing = 2, fc = 3, haste = 3, cursna = 5 } },
  Divine = { label = "Divine magic", when = "Banish, Holy, Repose",
    weights = { divine = 1.5, mnd = 1, macc = 1, mab = 2, mdmg = 0.3 } },
  Enhancing = { label = "Enhancing magic", when = "enhancing magic without its set",
    weights = { enhancing = 2, enhdur = 6, fc = 2, haste = 2, mnd = 0.2 } },
  Stoneskin = { label = "Stoneskin", when = "Stoneskin", weights = { mnd = 2, enhancing = 1, ["ja:stoneskin"] = 10 } },
  Phalanx = { label = "Phalanx", when = "Phalanx", weights = { enhancing = 2, phalanx = 10, enhdur = 2 } },
  RefreshSpell = { label = "Refresh", when = "Refresh", weights = { enhdur = 6, ["ja:refresh"] = 10, enhancing = 0.5 } },
  RegenSpell = { label = "Regen", when = "Regen", weights = { regendur = 3, regenpot = 6, enhdur = 4 } },
  Enfeebling_MND = { label = "Enfeebling (MND)", when = "Slow, Paralyze, Silence, Dia",
    weights = { macc = 1.5, enfeebling = 1.5, mnd = 1, enfeffect = 3, enfdur = 3, maccskill = 0.3 } },
  Enfeebling_INT = { label = "Enfeebling (INT)", when = "Sleep, Blind, Bind, Gravity, Dispel",
    weights = { macc = 1.5, enfeebling = 1.5, int = 1, enfeffect = 3, enfdur = 3, maccskill = 0.3 } },
  Nuke = { label = "Nuke", when = "elemental magic",
    weights = { mab = 3, macc = 0.6, int = 0.8, mdmg = 0.5, elemental = 0.6, mcrit = 1, mcritdmg = 1, maccskill = 0.2 } },
  Nuke_MB = { label = "Nuke - magic burst", when = "elemental magic, the burst toggle",
    weights = { mbb = 8, mbb2 = 8, mab = 3, macc = 0.6, int = 0.8, mdmg = 0.5, elemental = 0.6 } },
  MagicAcc = { label = "Magic accuracy", when = "the magic accuracy toggle",
    weights = { macc = 2, int = 1, mnd = 0.5, elemental = 1, enfeebling = 1, dark = 1, maccskill = 0.5 } },
  Dark = { label = "Dark magic", when = "Bio, Absorb", weights = { dark = 2, macc = 1, int = 1, maccskill = 0.3 } },
  DrainAspir = { label = "Drain / Aspir", when = "Drain, Aspir", weights = { drain = 8, dark = 2, macc = 1, int = 0.5 } },
  Stun = { label = "Stun", when = "Stun", caps = { fc = 80 }, weights = { macc = 2, dark = 1, fc = 4, haste = 3, int = 0.5 } },
  -- Songs
  Songs_Buff = { label = "Songs - buffs", when = "Minuet, March, Ballad and other buff songs",
    weights = { allsongs = 10, songdur = 4, chr = 0.3, singing = 0.5, string = 0.5, wind = 0.5,
      song_minuet = 6, song_march = 6, song_madrigal = 6, song_paeon = 4, song_ballad = 6, song_minne = 4,
      song_mambo = 4, song_etude = 4, song_carol = 4, song_prelude = 4, song_mazurka = 4, song_scherzo = 4,
      song_hymnus = 4 } },
  Songs_Debuff = { label = "Songs - debuffs", when = "Lullaby, Elegy, Requiem, Threnody, Finale",
    weights = { macc = 2, chr = 1, singing = 1, string = 1, wind = 1, allsongs = 4, song_lullaby = 4, song_elegy = 4,
      song_requiem = 4, song_threnody = 4, song_nocturne = 4, song_virelai = 4, maccskill = 0.3 } },
  -- Summoning
  Idle_Avatar = { label = "Idle - avatar out", when = "an avatar is out", caps = DEF, weapons = true,
    weights = merge(CASTER_IDLE, { perp = 20, summoning = 0.5, ["avatar.refresh"] = 5 }) },
  BP_Delay = { label = "Blood pact - delay", when = "before a blood pact", weights = { bpdelay = 10, bpdelay2 = 10, summoning = 0.5 } },
  BloodPact = { label = "Blood pact", when = "during a blood pact",
    weights = { bpdmg = 8, summoning = 0.5, ["avatar.att"] = 1, ["avatar.acc"] = 1, ["avatar.mab"] = 2,
      ["avatar.macc"] = 1, ["avatar.bpdmg"] = 8, ["avatar.da"] = 4 } },
  -- Ninjutsu
  Utsusemi = { label = "Utsusemi", when = "Utsusemi", caps = { fc = 80 }, weights = { fc = 8, haste = 4, ["ja:utsusemi"] = 10 } },
  Ninjutsu_Nuke = { label = "Ninjutsu - nukes", when = "Katon, Suiton and the other elemental ninjutsu",
    weights = { nindmg = 5, mab = 3, macc = 1, int = 0.8, ninjutsu = 1, mdmg = 0.5 } },
  Ninjutsu_Debuff = { label = "Ninjutsu - debuffs", when = "Kurayami, Hojo, Jubaku, Dokumori",
    weights = { macc = 2, ninjutsu = 1.5, int = 0.5 } },
  -- Blue magic
  BluPhys = { label = "Blue - physical", when = "physical blue magic",
    weights = { str = 1, dex = 0.8, vit = 0.5, agi = 0.5, acc = 1, att = 0.7, blue = 0.5, crit = 1, wsd = 2 } },
  BluMag = { label = "Blue - magical", when = "magical blue magic and breaths",
    weights = { mab = 3, macc = 1, int = 0.8, mnd = 0.3, blue = 0.6, mdmg = 0.5 } },
  BluDebuff = { label = "Blue - debuffs", when = "blue magic that enfeebles", weights = { macc = 2, blue = 1.5, int = 0.5, mnd = 0.5 } },
  BluHeal = { label = "Blue - healing", when = "Magic Fruit, Pollen and other healing blue magic",
    weights = { cure = 10, cure2 = 10, mnd = 1, vit = 0.5, blue = 0.5 } },
  BluBuff = { label = "Blue - buffs", when = "Cocoon, Refueling and other self buffs", weights = { blue = 1.5, fc = 2, haste = 2 } },
  -- Geomancy
  Geo = { label = "Geomancy", when = "Indi- and Geo- spells",
    weights = { geomancy = 2, handbell = 2, geomancy_pot = 8, indidur = 5, cmp = 1 } },
  Idle_Luopan = { label = "Idle - luopan out", when = "a luopan is out", caps = DEF, weapons = true,
    weights = merge(CASTER_IDLE, { ["luopan.regen"] = 4, ["luopan.dt"] = -4, geomancy = 0.5 }) },
  -- Pets
  Idle_Pet = { label = "Idle - pet out", when = "a pet is out", caps = DEF, weapons = true,
    weights = merge(DT, { ["pet.dt"] = -4, ["pet.regen"] = 4, ["pet.haste"] = 3, refresh = 10, regen = 6 }) },
  Ready = { label = "Ready / Sic", when = "a pet's physical move",
    weights = { ["pet.acc"] = 1, ["pet.att"] = 1, ["pet.da"] = 4, ["pet.crit"] = 3, ["pet.str"] = 0.5 } },
  Ready_Magic = { label = "Ready - magical", when = "a pet's magical move", weights = { ["pet.mab"] = 3, ["pet.macc"] = 1.5 } },
  Reward = { label = "Reward", when = "Reward", weights = { mnd = 1.5, reward = 8, ["ja:reward"] = 8 } },
  Charm = { label = "Charm", when = "Charm", weights = { chr = 1.5, charm = 8, ["ja:charm"] = 8 } },
  Automaton = { label = "Automaton", when = "engaged with the automaton out",
    weights = { ["automaton.acc"] = 1, ["automaton.att"] = 1, ["automaton.haste"] = 4, ["automaton.da"] = 4,
      ["automaton.macc"] = 0.5, ["automaton.mab"] = 0.5 } },
  Breath = { label = "Wyvern breath", when = "Restoring and elemental breaths",
    weights = { breath = 8, hp = 0.05, ["wyvern.breath"] = 8 } },
  Jump = { label = "Jump", when = "Jump and the other jumps",
    weights = { ["ja:jump"] = 6, ["ja:high jump"] = 6, ["ja:spirit jump"] = 6, ["ja:soul jump"] = 6, acc = 1, att = 0.5,
      stp = 3, vit = 0.5, da = 3 } },
  -- Corsair
  Roll = { label = "Phantom Roll", when = "Phantom Roll", weights = { roll = 10, rolldur = 3 } },
  QuickDraw = { label = "Quick Draw", when = "Quick Draw",
    weights = { qd = 8, mab = 3, macc = 1.5, agi = 0.8, racc = 0.5, ["ja:quick draw"] = 8 } },
  -- Thief
  TH = { label = "Treasure Hunter", when = "the first hit on a monster", weights = { th = 20 } },
  Steal = { label = "Steal", when = "Steal, Despoil", weights = { steal = 8, despoil = 8, ["ja:steal"] = 8 } },
}

-- Ability sets: an ability's gear ("Boost"+5 on the item scores as ja:boost). Each one is a set named by the
-- ability; jobs list the ones their gear enhances.
M.ABILITY_SETS = {
  WAR = { "Berserk", "Warcry", "Aggressor", "Tomahawk", "Mighty Strikes", "Blood Rage", "Restraint" },
  MNK = { "Boost", "Chakra", "Focus", "Dodge", "Counterstance", "Footwork", "Hundred Fists", "Impetus", "Mantra" },
  WHM = { "Benediction", "Divine Caress", "Afflatus Solace", "Devotion", "Martyr" },
  BLM = { "Mana Wall", "Elemental Seal", "Manafont" },
  RDM = { "Chainspell", "Saboteur", "Composure" },
  THF = { "Flee", "Hide", "Sneak Attack", "Trick Attack", "Feint", "Collaborator", "Accomplice", "Perfect Dodge" },
  PLD = { "Sentinel", "Shield Bash", "Rampart", "Cover", "Invincible", "Holy Circle", "Chivalry", "Fealty", "Divine Emblem" },
  DRK = { "Souleater", "Weapon Bash", "Last Resort", "Arcane Circle", "Blood Weapon", "Dark Seal", "Nether Void" },
  BST = { "Call Beast", "Tame", "Familiar", "Killer Instinct" },
  BRD = { "Soul Voice", "Nightingale", "Troubadour" },
  RNG = { "Barrage", "Shadowbind", "Camouflage", "Eagle Eye Shot", "Scavenge", "Bounty Shot", "Velocity Shot" },
  SAM = { "Meditate", "Third Eye", "Warding Circle", "Meikyo Shisui", "Sengikori", "Hasso", "Seigan" },
  NIN = { "Yonin", "Innin", "Mijin Gakure", "Futae", "Sange" },
  DRG = { "Ancient Circle", "Angon", "Call Wyvern", "Spirit Link", "Dragon Breaker" },
  SMN = { "Astral Flow", "Elemental Siphon", "Mana Cede" },
  BLU = { "Azure Lore", "Burst Affinity", "Chain Affinity", "Efflux", "Diffusion" },
  COR = { "Wild Card", "Random Deal", "Snake Eye", "Fold" },
  PUP = { "Overdrive", "Activate", "Repair", "Maneuver", "Role Reversal", "Tactical Switch" },
  DNC = { "Trance", "No Foot Rise", "Saber Dance", "Fan Dance", "Climactic Flourish", "Striking Flourish", "Reverse Flourish" },
  SCH = { "Tabula Rasa", "Rapture", "Ebullience", "Immanence", "Perpetuance", "Klimaform", "Enlightenment" },
  GEO = { "Bolster", "Life Cycle", "Full Circle", "Radial Arcana", "Mending Halation", "Entrust", "Dematerialize" },
  RUN = { "Elemental Sforzo", "Vallation", "Valiance", "Pflug", "Gambit", "Rayke", "Battuta", "Liement", "One for All",
    "Embolden", "Vivacious Pulse", "Swordplay" },
}
for _, list in pairs(M.ABILITY_SETS) do
  for _, a in ipairs(list) do
    if not M.SETS[a] then
      M.SETS[a] = { label = a, when = a, ability = true, weights = { ["ja:" .. a:lower()] = 10 } }
    end
  end
end

-- Each job's sets in the window's order. "WS:<id>" sets come from the job's weapon skills (sets.ws_sets).
local MAGE = { "Idle", "Resting", "Precast", "Movement", "PDT", "MDT" }
local function list(...)
  local out = {}
  for _, t in ipairs({ ... }) do
    for _, s in ipairs(t) do out[#out + 1] = s end
  end
  return out
end
M.JOBS = {
  WAR = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Idle", "PDT", "MDT", "Enmity", "Movement", "Waltz" }, M.ABILITY_SETS.WAR),
  MNK = list({ "TP_MNK", "TP_Hybrid", "TP_Acc", "WS", "Idle", "PDT", "MDT", "Movement", "Waltz" }, M.ABILITY_SETS.MNK),
  WHM = list(MAGE, { "Precast_Cure", "Cure", "Healing", "NaSpells", "Enhancing", "Stoneskin", "RegenSpell", "Divine",
    "Enfeebling_MND", "TP", "WS" }, M.ABILITY_SETS.WHM),
  BLM = list(MAGE, { "Nuke", "Nuke_MB", "MagicAcc", "Dark", "DrainAspir", "Stun", "Enfeebling_INT", "Enhancing",
    "Stoneskin" }, M.ABILITY_SETS.BLM),
  RDM = list(MAGE, { "Precast_Cure", "Cure", "Enhancing", "RefreshSpell", "Phalanx", "Stoneskin", "Enfeebling_MND",
    "Enfeebling_INT", "Nuke", "Nuke_MB", "MagicAcc", "Dark", "DrainAspir", "TP", "TP_Hybrid", "WS" }, M.ABILITY_SETS.RDM),
  THF = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "TH", "Steal", "Idle", "PDT", "MDT", "Preshot", "Midshot", "Movement",
    "Waltz" }, M.ABILITY_SETS.THF),
  PLD = list({ "TP", "TP_Hybrid", "WS", "Enmity", "Idle", "PDT", "MDT", "Precast", "Precast_Cure", "Cure", "Phalanx",
    "Enhancing", "Divine", "Movement" }, M.ABILITY_SETS.PLD),
  DRK = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Idle", "PDT", "MDT", "Precast", "Dark", "DrainAspir", "Stun",
    "Enfeebling_INT", "Movement" }, M.ABILITY_SETS.DRK),
  BST = list({ "TP", "TP_Hybrid", "TP_Pet", "WS", "Idle", "Idle_Pet", "PDT", "MDT", "Ready", "Ready_Magic", "Reward",
    "Charm", "Movement" }, M.ABILITY_SETS.BST),
  BRD = list(MAGE, { "Precast_Song", "Precast_Cure", "Songs_Buff", "Songs_Debuff", "Cure", "Enhancing", "TP", "WS" },
    M.ABILITY_SETS.BRD),
  RNG = list({ "Preshot", "Midshot", "RangedWS", "WS", "TP", "Idle", "PDT", "MDT", "Movement" }, M.ABILITY_SETS.RNG),
  SAM = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "RangedWS", "Preshot", "Midshot", "Idle", "PDT", "MDT", "Movement" },
    M.ABILITY_SETS.SAM),
  NIN = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Idle", "PDT", "MDT", "Enmity", "Precast", "Utsusemi",
    "Ninjutsu_Nuke", "Ninjutsu_Debuff", "Preshot", "Midshot", "Movement" }, M.ABILITY_SETS.NIN),
  DRG = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Jump", "Breath", "Idle", "PDT", "MDT", "Precast", "Movement" },
    M.ABILITY_SETS.DRG),
  SMN = list(MAGE, { "Idle_Avatar", "BP_Delay", "BloodPact", "Cure", "Enhancing" }, M.ABILITY_SETS.SMN),
  BLU = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Idle", "PDT", "MDT", "Precast", "BluPhys", "BluMag", "BluDebuff",
    "BluHeal", "BluBuff", "Movement" }, M.ABILITY_SETS.BLU),
  COR = list({ "TP", "TP_Hybrid", "WS", "RangedWS", "Preshot", "Midshot", "Roll", "QuickDraw", "Idle", "PDT", "MDT",
    "Movement" }, M.ABILITY_SETS.COR),
  PUP = list({ "TP", "TP_Hybrid", "WS", "Automaton", "Idle", "Idle_Pet", "PDT", "MDT", "Movement" }, M.ABILITY_SETS.PUP),
  DNC = list({ "TP", "TP_Hybrid", "TP_Acc", "WS", "Waltz", "Steps", "Idle", "PDT", "MDT", "Movement" }, M.ABILITY_SETS.DNC),
  SCH = list(MAGE, { "Precast_Cure", "Cure", "Healing", "NaSpells", "Enhancing", "RegenSpell", "Stoneskin", "Nuke",
    "Nuke_MB", "MagicAcc", "Dark", "DrainAspir", "Enfeebling_MND", "Enfeebling_INT" }, M.ABILITY_SETS.SCH),
  GEO = list(MAGE, { "Geo", "Idle_Luopan", "Nuke", "Nuke_MB", "MagicAcc", "Dark", "DrainAspir", "Cure",
    "Enfeebling_MND", "Enfeebling_INT" }, M.ABILITY_SETS.GEO),
  RUN = list({ "TP", "TP_Hybrid", "WS", "Enmity", "Idle", "PDT", "MDT", "Precast", "Phalanx", "Enhancing", "Movement" },
    M.ABILITY_SETS.RUN),
}

-- The combat skill stats (ws.lua's skill names) a job's weapon types use: their skill counts in TP and WS sets.
M.SKILL_STAT = { hand_to_hand = "sk_h2h", dagger = "sk_dagger", sword = "sk_sword", great_sword = "sk_gsword",
  axe = "sk_axe", great_axe = "sk_gaxe", scythe = "sk_scythe", polearm = "sk_polearm", katana = "sk_katana",
  great_katana = "sk_gkatana", club = "sk_club", staff = "sk_staff", archery = "sk_archery",
  marksmanship = "sk_marks", throwing = "sk_throw" }

-- A weapon skill's set from its facts (ws.lua): physical, ranged or magical weights plus its modifiers.
function M.ws_set(id, ws, name)
  local w
  if ws.kind == "magic" then
    w = { wsd = 6, mab = 3, macc = 0.6, mdmg = 0.3, tpb = 0.02 }
  elseif ws.kind == "ranged" then
    w = { wsd = 8, ratt = 1, racc = 1, tpb = 0.02, crit = 1 }
  else
    w = { wsd = 8, att = 0.7, acc = 0.6, tpb = 0.02, da = 2, ta = 3, crit = 1 }
  end
  for stat, pct in pairs(ws.mods or {}) do w[stat] = (w[stat] or 0) + pct / 100 * 1.6 end
  local sk = M.SKILL_STAT[ws.skill]
  if sk then w[sk] = 0.6 end
  return { label = name or ("Weapon skill " .. id), when = name or "this weapon skill", ws = id, weights = w }
end

-- The weapon skills (ids) a job has its sets for: every one ws.lua lists for the job.
function M.ws_ids(job, wslist)
  local out = {}
  for id, ws in pairs(wslist) do
    if ws.jobs and ("," .. ws.jobs .. ","):find("," .. job .. ",", 1, true) then out[#out + 1] = id end
  end
  table.sort(out)
  return out
end

-- Jobs that swap weapons in every set but the melee ones (no TP to lose).
M.MAGE_JOBS = { WHM = true, BLM = true, SMN = true, SCH = true, GEO = true }
-- The melee sets (weapons kept, whatever the job).
M.MELEE_SETS = { TP = true, TP_Hybrid = true, TP_Acc = true, TP_MNK = true, TP_Pet = true, WS = true, RangedWS = true,
  Preshot = true, Midshot = true, Waltz = true, Steps = true, Jump = true, Automaton = true }

-- ---- Which set an action wears ----

-- Spell skills (res.spell().skill).
local SKILL = { divine = 32, healing = 33, enhancing = 34, enfeebling = 35, elemental = 36, dark = 37, summoning = 38,
  ninjutsu = 39, singing = 40, string = 41, wind = 42, blue = 43, geomancy = 44, handbell = 45 }

local function any(name, patterns)
  for _, p in ipairs(patterns) do
    if name:find(p) then return true end
  end
  return false
end

local DEBUFF_SONGS = { "Lullaby", "Requiem", "Elegy", "Threnody", "Finale", "Nocturne", "Virelai", "Dirge" }
local NIN_DEBUFF = { "^Kurayami", "^Hojo", "^Jubaku", "^Dokumori", "^Aisha", "^Yurin" }

-- The sets a spell wears, precast first: { precast, midcast } (either may be nil).
function M.spell_sets(spell, blu)
  local n, sk = spell.name or "", spell.skill
  local pre = "Precast"
  if n:find("^Cur") then pre = "Precast_Cure" end
  if sk == SKILL.singing or sk == SKILL.string or sk == SKILL.wind then pre = "Precast_Song" end
  local mid
  if n:find("^Cure") or n:find("^Cura") then mid = "Cure"
  elseif n:find("^Utsusemi") then mid = "Utsusemi"
  elseif n == "Stoneskin" then mid = "Stoneskin"
  elseif n:find("^Phalanx") then mid = "Phalanx"
  elseif n:find("^Refresh") then mid = "RefreshSpell"
  elseif n:find("^Regen") then mid = "RegenSpell"
  elseif n:find("^Drain") or n:find("^Aspir") then mid = "DrainAspir"
  elseif n == "Stun" then mid = "Stun"
  elseif n == "Flash" or n == "Reprisal" then mid = "Enmity"
  elseif n:find("na$") or n == "Erase" or n == "Cursna" then mid = "NaSpells"
  elseif sk == SKILL.healing then mid = "Healing"
  elseif sk == SKILL.divine then mid = "Divine"
  elseif sk == SKILL.enhancing then mid = "Enhancing"
  elseif sk == SKILL.enfeebling then mid = spell.kind == 1 and "Enfeebling_MND" or "Enfeebling_INT"
  elseif sk == SKILL.elemental then mid = "Nuke"
  elseif sk == SKILL.dark then mid = "Dark"
  elseif sk == SKILL.ninjutsu then mid = any(n, NIN_DEBUFF) and "Ninjutsu_Debuff" or (n:find("ton:") and "Ninjutsu_Nuke") or nil
  elseif sk == SKILL.singing or sk == SKILL.string or sk == SKILL.wind then
    mid = any(n, DEBUFF_SONGS) and "Songs_Debuff" or "Songs_Buff"
  elseif sk == SKILL.blue then
    local k = blu and blu[spell.id]
    mid = ({ phys = "BluPhys", magic = "BluMag", breath = "BluMag", debuff = "BluDebuff", heal = "BluHeal",
      drain = "BluMag", buff = "BluBuff" })[k or "buff"]
  elseif sk == SKILL.geomancy or sk == SKILL.handbell then mid = "Geo"
  end
  return { pre, mid }
end

-- The sets a job ability wears: its set if the job has one, else a family's.
function M.ability_sets(name, job_sets)
  if job_sets[name] then return { name } end
  if name:find("Waltz$") or name:find("^Curing Waltz") or name == "Divine Waltz" then return { "Waltz" } end
  if name:find("Step$") then return { "Steps" } end
  if name:find("Jump$") then return { "Jump" } end
  if name:find("Roll$") then return { "Roll" } end
  if name:find("Shot$") and job_sets.QuickDraw then return { "QuickDraw" } end
  if name == "Provoke" or name == "Sentinel" or name == "Animated Flourish" then return { "Enmity" } end
  if name == "Steal" or name == "Despoil" then return { "Steal" } end
  if name == "Reward" or name == "Charm" then return { name } end
  return {}
end

-- ---- Element gear (by name: their effect is not a number on the item) ----

M.ELEMENTS = { "fire", "ice", "wind", "earth", "thunder", "water", "light", "dark" }
-- res element ids (0 Fire .. 7 Dark) -> our names.
M.ELEMENT_ID = { [0] = "fire", "ice", "wind", "earth", "thunder", "water", "light", "dark" }

-- Elemental staves: NQ and HQ (the HQ's affinity is the stronger).
M.STAVES = {
  fire = { "Fire Staff", "Vulcan's Staff" }, ice = { "Ice Staff", "Aquilo's Staff" },
  wind = { "Wind Staff", "Auster's Staff" }, earth = { "Earth Staff", "Terra's Staff" },
  thunder = { "Thunder Staff", "Jupiter's Staff" }, water = { "Water Staff", "Neptune's Staff" },
  light = { "Light Staff", "Apollo's Staff" }, dark = { "Dark Staff", "Pluto's Staff" },
}
-- Obis: worn when the spell's element is the day's or the weather's.
M.OBIS = { fire = "Karin Obi", ice = "Hyorin Obi", wind = "Furin Obi", earth = "Dorin Obi", thunder = "Rairin Obi",
  water = "Suirin Obi", light = "Korin Obi", dark = "Anrin Obi" }
M.OBI_ALL = "Hachirin-no-Obi"
-- Gorgets and belts: worn for a weapon skill whose skillchain properties include their element (Fotia: all).
M.GORGETS = { fire = "Flame Gorget", ice = "Snow Gorget", wind = "Breeze Gorget", earth = "Soil Gorget",
  thunder = "Thunder Gorget", water = "Aqua Gorget", light = "Light Gorget", dark = "Shadow Gorget" }
M.BELTS = { fire = "Flame Belt", ice = "Snow Belt", wind = "Breeze Belt", earth = "Soil Belt", thunder = "Thunder Belt",
  water = "Aqua Belt", light = "Light Belt", dark = "Shadow Belt" }
M.FOTIA = { neck = "Fotia Gorget", waist = "Fotia Belt" }
-- Skillchain properties -> the elements they carry.
M.SC_ELEMENTS = {
  transfixion = { "light" }, compression = { "dark" }, liquefaction = { "fire" }, scission = { "earth" },
  reverberation = { "water" }, detonation = { "wind" }, induration = { "ice" }, impaction = { "thunder" },
  fusion = { "fire", "light" }, fragmentation = { "wind", "thunder" }, gravitation = { "earth", "dark" },
  distortion = { "water", "ice" }, light = { "fire", "wind", "thunder", "light" },
  darkness = { "earth", "water", "ice", "dark" },
}

return M
