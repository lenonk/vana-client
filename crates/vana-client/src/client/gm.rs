//! Ours (not retail): the GM window's data. With a server that speaks our packets (akreao/lsb #4), the client asks
//! for its GM level and command list at each zone-in (c2s 0x1F0), and the server answers with the real level
//! (`m_GMlevel`, not the name badge) and the commands that level may use, in pages (s2c 0x1F0). A command runs
//! through c2s 0x1F1, not the chat line; s2c 0x1F1 says whether it ran and carries what it printed.
//! gm_fields.txt adds what the server doesn't send: each command's group, a warning for the risky ones, and a
//! label and picker for each field. The window itself is client/modern/gmwin.rs.

use std::collections::HashMap;

/// The groups down the window's left, in order: (key, label).
pub const GROUPS: [(&str, &str); 9] = [
    ("travel", "Travel"),
    ("player", "Player & stats"),
    ("items", "Items & gil"),
    ("unlocks", "Unlocks & points"),
    ("quests", "Quests & missions"),
    ("mobs", "Mobs & NPCs"),
    ("moderation", "Moderation"),
    ("world", "World & events"),
    ("debug", "Debug & scripts"),
];

/// What a field's input offers besides typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    Text,
    Number,
    /// Empty means yourself (the server's default); suggests party and alliance names.
    Player,
    /// A zone by name or id; sends the id.
    Zone,
    /// An item id or name (the server looks names up); shows the id's name.
    Item,
    /// A mob or NPC id: "Use target" fills it.
    Entity,
    /// A mob or NPC by name (gotoname): the zone's names, spaces as _.
    EntityName,
    /// A quest or mission log (globals/log_ids.lua's keys).
    Log,
    Job,
    Weather,
    /// Part of a position: "Use my position" fills x, y and z.
    Coord,
    /// An augment id or value, folded away under "Augments".
    Augment,
    /// true / false.
    Toggle,
    Spell,
    KeyItem,
    Title,
    /// A combat or craft skill (xi.skill's names).
    Skill,
    Nation,
}

impl Pick {
    fn parse(s: &str) -> Pick {
        match s {
            "number" => Pick::Number,
            "player" => Pick::Player,
            "zone" => Pick::Zone,
            "item" => Pick::Item,
            "entity" => Pick::Entity,
            "entityname" => Pick::EntityName,
            "log" => Pick::Log,
            "job" => Pick::Job,
            "weather" => Pick::Weather,
            "coord" => Pick::Coord,
            "augment" => Pick::Augment,
            "toggle" => Pick::Toggle,
            "spell" => Pick::Spell,
            "keyitem" => Pick::KeyItem,
            "title" => Pick::Title,
            "skill" => Pick::Skill,
            "nation" => Pick::Nation,
            _ => Pick::Text,
        }
    }

    /// The line under the field.
    pub fn hint(self) -> &'static str {
        match self {
            Pick::Player => "Empty: yourself.",
            Pick::Zone => "Zone name or id.",
            Pick::Item => "Item id, or its name with _ for spaces.",
            Pick::Entity => "Empty: your target.",
            Pick::Log => "The log's name.",
            Pick::Spell => "Spell id or name.",
            Pick::KeyItem => "Key item id or name.",
            Pick::Title => "Title id or name.",
            Pick::Job => "Job, such as WAR or 1.",
            Pick::Skill => "Skill name or id.",
            _ => "",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    pub pick: Pick,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub name: String,
    /// The level the command needs (cmdprops permission).
    pub level: u8,
    /// cmdprops parameters: s a word, i a number, d a decimal, b the whole line to the script.
    pub types: String,
    pub group: &'static str,
    pub danger: bool,
    pub desc: String,
    pub fields: Vec<Field>,
}

impl Command {
    /// The script reads the rest of the line itself (b, or a lone s): our fields come from its usage line.
    fn whole_line(&self) -> bool {
        self.types == "b" || self.types == "s"
    }

    /// The line to send (without '!'), or what is wrong with the values, by field.
    pub fn line(&self, values: &[String]) -> Result<String, (usize, String)> {
        let vals: Vec<&str> = (0..self.fields.len()).map(|i| values.get(i).map_or("", |v| v.trim())).collect();
        let last = vals.iter().rposition(|v| !v.is_empty());
        let mut parts = vec![self.name.as_str()];
        for (i, f) in self.fields.iter().enumerate() {
            let v = vals[i];
            if last.is_none_or(|l| i > l) {
                if !f.optional {
                    return Err((i, format!("{} is needed.", f.label)));
                }
                continue;
            }
            if v.is_empty() {
                return Err((i, format!("Fill {} too: the server reads the fields in order.", f.label)));
            }
            let lone = self.fields.len() == 1 && self.whole_line();
            if !lone && v.contains(char::is_whitespace) {
                return Err((i, format!("{} can't contain spaces.", f.label)));
            }
            let int = !self.whole_line() && self.types.as_bytes().get(i) == Some(&b'i');
            if int && v.parse::<i64>().is_err() {
                return Err((i, format!("{} must be a whole number.", f.label)));
            }
            parts.push(v);
        }
        let line = parts.join(" ");
        let bytes = crate::dat::font::encode_sjis(&line).len();
        if bytes > MAX_LINE {
            return Err((last.unwrap_or(0), format!("Too long: {bytes} bytes, the packet holds {MAX_LINE}.")));
        }
        Ok(line)
    }
}

/// The most bytes of a line c2s 0x1F1 carries (Shift-JIS).
pub const MAX_LINE: usize = 248;

/// gm_fields.txt by command.
struct Extra {
    group: &'static str,
    danger: bool,
    desc: &'static str,
    fields: Vec<Field>,
}

fn extras() -> HashMap<&'static str, Extra> {
    let mut out = HashMap::new();
    for line in include_str!("gm_fields.txt").lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let col: Vec<&'static str> = line.split('\t').collect();
        let [name, group, danger, desc, fields] = col[..] else { continue };
        let group = GROUPS.iter().find(|g| g.0 == group).map_or("debug", |g| g.0);
        // A label can hold '|' (alternatives such as "mobId|npcId"): a piece without '~' joins the next one.
        let mut list: Vec<Field> = Vec::new();
        let mut held = String::new();
        for f in fields.split('|').filter(|f| !f.is_empty()) {
            if !f.contains('~') {
                held.push_str(f);
                held.push('/');
                continue;
            }
            let mut p = f.split('~');
            let label = std::mem::take(&mut held) + p.next().unwrap_or_default();
            let pick = Pick::parse(p.next().unwrap_or_default());
            list.push(Field { label, pick, optional: p.next() == Some("1") });
        }
        // Fields are positional: one followed by a needed field is needed too (giveitem's player, for one).
        if let Some(l) = list.iter().rposition(|f| !f.optional) {
            list[..l].iter_mut().for_each(|f| f.optional = false);
        }
        let fields = list;
        out.insert(name, Extra { group, danger: danger == "1", desc, fields });
    }
    out
}

/// A command the server lists: our extras, or plain fields from its types.
fn command(extra: &HashMap<&'static str, Extra>, name: String, level: u8, types: String) -> Command {
    match extra.get(name.as_str()) {
        Some(e) => {
            // Our usage-line fields hold for b and lone-s commands; else as many as the server's types.
            let mut fields = e.fields.clone();
            if !(types == "b" || types == "s") {
                fields.truncate(types.len());
                for (i, t) in types.bytes().enumerate().skip(fields.len()) {
                    fields.push(Field { label: format!("field {}", i + 1), pick: if t == b'i' { Pick::Number } else { Pick::Text }, optional: true });
                }
            }
            Command { name, level, types, group: e.group, danger: e.danger, desc: e.desc.to_string(), fields }
        }
        None => {
            let fields = match types.as_str() {
                "b" | "s" => vec![Field { label: "arguments".into(), pick: Pick::Text, optional: true }],
                _ => types.bytes().enumerate().map(|(i, t)| Field { label: format!("field {}", i + 1), pick: if t == b'i' { Pick::Number } else { Pick::Text }, optional: true }).collect(),
            };
            Command { name, level, types, group: "debug", danger: false, desc: String::new(), fields }
        }
    }
}

/// What s2c 0x1F1 said about a command.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub line: String,
    /// None while waiting; Some(0) ran, 1 failed, 2 refused.
    pub result: Option<u8>,
    pub output: Vec<String>,
    pub cut: bool,
}

impl Reply {
    pub fn status(&self) -> &'static str {
        match self.result {
            None => "Sent, waiting for the server.",
            Some(0) => "Ran.",
            Some(1) => "Failed: unknown command, level too low, or a script error.",
            _ => "Refused: not a GM, or in jail.",
        }
    }
}

/// The GM window's state, kept across zones (the list arrives again at each zone-in).
#[derive(Default)]
pub struct Gm {
    /// The real GM level from s2c 0x1F0; 0 until a server that speaks it says otherwise.
    pub level: u8,
    pub commands: Vec<Command>,
    /// The pages arriving: the list so far and the count expected.
    incoming: Vec<(String, u8, String)>,
    pub open: bool,
    /// The rail's group (None: all) and the list's page.
    pub group: Option<usize>,
    pub page: usize,
    /// The search's text while typing in it.
    pub find: String,
    pub selected: Option<String>,
    /// The values typed, by (command, field).
    pub values: HashMap<(String, usize), String>,
    /// The field taking the typed text: a field of the selected command, or the search (None).
    pub typing: Option<usize>,
    pub find_typing: bool,
    /// A risky command's first press.
    pub armed: bool,
    /// The augment fields shown (folded away by default).
    pub augments: bool,
    /// Each picker's choices once built (the item list takes a moment, so only when first needed).
    pub lists: HashMap<Pick, Choices>,
    loading: Vec<(Pick, std::sync::mpsc::Receiver<Choices>)>,
    next_id: u16,
    /// Replies by request id, newest last; a few kept.
    pub replies: Vec<(u16, Reply)>,
}

/// lsb's xi.job names, by id from 1.
const JOBS: [&str; 22] = ["WAR", "MNK", "WHM", "BLM", "RDM", "THF", "PLD", "DRK", "BST", "BRD", "RNG", "SAM", "NIN", "DRG", "SMN", "BLU", "COR", "PUP", "DNC", "SCH", "GEO", "RUN"];
/// lsb's xi.weather names.
const WEATHER: [&str; 20] = ["NONE", "SUNSHINE", "CLOUDS", "FOG", "HOT_SPELL", "HEAT_WAVE", "RAIN", "SQUALL", "DUST_STORM", "SAND_STORM", "WIND", "GALES", "SNOW", "BLIZZARDS", "THUNDER", "THUNDERSTORMS", "AURORAS", "STELLAR_GLARE", "GLOOM", "DARKNESS"];
/// lsb's xi.skill names (the commands take them as well as ids).
const SKILLS: [&str; 48] = ["HAND_TO_HAND", "DAGGER", "SWORD", "GREAT_SWORD", "AXE", "GREAT_AXE", "SCYTHE", "POLEARM", "KATANA", "GREAT_KATANA", "CLUB", "STAFF", "AUTOMATON_MELEE", "AUTOMATON_RANGED", "AUTOMATON_MAGIC", "ARCHERY", "MARKSMANSHIP", "THROWING", "GUARD", "EVASION", "SHIELD", "PARRY", "DIVINE_MAGIC", "HEALING_MAGIC", "ENHANCING_MAGIC", "ENFEEBLING_MAGIC", "ELEMENTAL_MAGIC", "DARK_MAGIC", "SUMMONING_MAGIC", "NINJUTSU", "SINGING", "STRING_INSTRUMENT", "WIND_INSTRUMENT", "BLUE_MAGIC", "GEOMANCY", "HANDBELL", "FISHING", "WOODWORKING", "SMITHING", "GOLDSMITHING", "CLOTHCRAFT", "LEATHERCRAFT", "BONECRAFT", "ALCHEMY", "COOKING", "SYNERGY", "RID", "DIG"];
/// globals/log_ids.lua's keys (quest and mission logs) and names.
const LOGS: &[(&str, &str)] = &[("SANDORIA", "San d'Oria"), ("BASTOK", "Bastok"), ("WINDURST", "Windurst"), ("JEUNO", "Jeuno"), ("SELBINA", "Selbina"), ("MHAURA", "Mhaura"), ("RABAO", "Rabao"), ("KAZHAM", "Kazham"), ("NORG", "Norg"), ("OTHER_AREAS_LOG", "Other Areas"), ("TAVNAZIA", "Tavnazian Safehold"), ("OUTLANDS", "Outlands"), ("ZILART", "Rise of the Zilart"), ("COP", "Chains of Promathia"), ("TOAU", "Treasures of Aht Urhgan"), ("AHT_URHGAN", "Aht Urhgan"), ("WOTG", "Wings of the Goddess"), ("CRYSTAL_WAR", "Crystal War"), ("ABYSSEA", "Abyssea"), ("ABYSSEA_KONSCHTAT", "Abyssea - Konschtat"), ("ABYSSEA_TAHRONGI", "Abyssea - Tahrongi"), ("ABYSSEA_LATHEINE", "Abyssea - La'Theine"), ("ABYSSEA_MISAREAUX", "Abyssea - Misareaux"), ("ABYSSEA_VUNKERL", "Abyssea - Vunkerl"), ("ABYSSEA_ATTOHWA", "Abyssea - Attohwa"), ("ABYSSEA_ALTEPA", "Abyssea - Altepa"), ("ABYSSEA_GRAUBERG", "Abyssea - Grauberg"), ("ABYSSEA_ULEGUERAND", "Abyssea - Uleguerand"), ("SOA", "Seekers of Adoulin"), ("ADOULIN", "Adoulin"), ("COALITION", "Coalition"), ("ASSAULT", "Assault"), ("CAMPAIGN", "Campaign"), ("ACP", "A Crystalline Prophecy"), ("AMK", "A Moogle Kupo d'Etat"), ("ASA", "A Shantotto Ascension"), ("ROV", "Rhapsodies of Vana'diel"), ("TVR", "The Voracious Resurgence")];

/// A picker's choices: (what the field sends, what the list shows).
pub type Choices = Vec<(String, String)>;

/// The choices a picker needs nothing loaded for.
fn fixed(pick: Pick) -> Option<Choices> {
    let named = |l: &[&str]| l.iter().map(|s| (s.to_string(), s.replace('_', " "))).collect();
    Some(match pick {
        Pick::Job => JOBS.iter().map(|s| (s.to_string(), s.to_string())).collect(),
        Pick::Weather => named(&WEATHER),
        Pick::Skill => named(&SKILLS),
        Pick::Log => LOGS.iter().map(|(k, n)| (k.to_string(), n.to_string())).collect(),
        Pick::Toggle => vec![("true".into(), "true".into()), ("false".into(), "false".into())],
        Pick::Nation => vec![("0".into(), "San d'Oria".into()), ("1".into(), "Bastok".into()), ("2".into(), "Windurst".into())],
        _ => return None,
    })
}

/// The choices read from the game's name tables, once: zones, items, spells, key items and titles by id.
pub fn from_names(pick: Pick, n: &crate::dat::names::Names) -> Option<Choices> {
    use crate::dat::names::NameSource;
    let keep = |id: u32, s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty() && s != ".").map(|s| (id.to_string(), s));
    Some(match pick {
        Pick::Zone => (0..0x300).filter_map(|i| keep(i, n.place(i))).collect(),
        Pick::Item => (1..0x10000).filter_map(|i| keep(i, n.items.item(i).map(|it| it.name))).collect(),
        Pick::Spell => (1..0x400).filter_map(|i| keep(i, n.spell(i))).collect(),
        Pick::KeyItem => n.key_item_list().into_iter().filter(|(id, _)| *id != 0).filter_map(|(id, s)| keep(id, Some(s))).collect(),
        Pick::Title => (1..0x800).filter_map(|i| keep(i, n.title(i))).collect(),
        _ => return None,
    })
}

/// Up to `max` choices for typed text: an id starting with it, or a name holding it (names starting with it first).
pub fn suggest<'a>(choices: &'a [(String, String)], typed: &str, max: usize) -> Vec<&'a (String, String)> {
    let t = typed.trim().to_lowercase();
    if t.is_empty() {
        return choices.iter().take(max).collect();
    }
    let mut hits: Vec<(u8, &(String, String))> = choices
        .iter()
        .filter_map(|c| {
            let n = c.1.to_lowercase();
            match () {
                _ if c.0.to_lowercase() == t || n == t => Some((0, c)),
                _ if n.starts_with(&t) || c.0.to_lowercase().starts_with(&t) => Some((1, c)),
                _ if n.contains(&t) => Some((2, c)),
                _ => None,
            }
        })
        .collect();
    hits.sort_by_key(|h| h.0);
    hits.into_iter().take(max).map(|h| h.1).collect()
}

/// Rows per page in the list.
pub const PER_PAGE: usize = 9;

impl Gm {
    /// s2c 0x1F0 (whole sub-packet): u8 Version @4, GmLevel @5, u16 Total @6, First @8, u8 Count @0xA, Flags @0xB
    /// (bit 0 last page), then entries u8 Permission, NameLen, ParamsLen, name, params.
    pub fn on_catalog(&mut self, p: &[u8]) {
        if p.len() < 12 || p[4] != 1 {
            return;
        }
        let (level, first, count, last) = (p[5], u16::from_le_bytes([p[8], p[9]]) as usize, p[10] as usize, p[11] & 1 != 0);
        if first == 0 {
            self.incoming.clear();
        }
        let mut o = 12;
        for _ in 0..count {
            let Some(&[perm, nl, tl]) = p.get(o..o + 3).map(|s| <&[u8; 3]>::try_from(s).unwrap()) else { break };
            let (nl, tl) = (nl as usize, tl as usize);
            let Some(name) = p.get(o + 3..o + 3 + nl) else { break };
            let Some(types) = p.get(o + 3 + nl..o + 3 + nl + tl) else { break };
            self.incoming.push((String::from_utf8_lossy(name).into_owned(), perm, String::from_utf8_lossy(types).into_owned()));
            o += 3 + nl + tl;
        }
        if last {
            let extra = extras();
            self.level = level;
            self.commands = std::mem::take(&mut self.incoming).into_iter().filter(|c| c.1 >= 1).map(|(n, l, t)| command(&extra, n, l, t)).collect();
            if self.level == 0 {
                self.open = false;
            }
        }
    }

    /// s2c 0x1F1: u16 RequestId @4, u8 Result @6, Flags @7 (bit 0 last, bit 1 cut short), Count @8, then lines of
    /// u8 length and text.
    pub fn on_result(&mut self, p: &[u8]) {
        if p.len() < 10 {
            return;
        }
        let id = u16::from_le_bytes([p[4], p[5]]);
        let Some((_, r)) = self.replies.iter_mut().find(|(k, _)| *k == id) else { return };
        r.result = Some(p[6]);
        r.cut |= p[7] & 2 != 0;
        let mut o = 10;
        for _ in 0..p[8] {
            let Some(&n) = p.get(o) else { break };
            let Some(t) = p.get(o + 1..o + 1 + n as usize) else { break };
            r.output.push(crate::dat::font::decode_sjis(t));
            o += 1 + n as usize;
        }
    }

    /// The commands the rail's group and the search show.
    pub fn shown(&self) -> Vec<&Command> {
        let q = self.find.trim().to_lowercase();
        self.commands
            .iter()
            .filter(|c| self.group.is_none_or(|g| GROUPS.get(g).is_some_and(|k| k.0 == c.group)))
            .filter(|c| q.is_empty() || c.name.contains(&q) || c.desc.to_lowercase().contains(&q))
            .collect()
    }

    /// Builds the choices the chosen command's pickers need: fixed lists, the name tables, the party and the
    /// people and things in the zone (those two every time, as they change).
    pub fn fill_lists(&mut self, names: Option<&std::sync::Arc<crate::dat::names::Names>>, people: Choices, around: Choices) {
        // Lists read from the DATs on a worker (items are 65535 reads), taken here once ready.
        self.loading.retain(|(pick, rx)| match rx.try_recv() {
            Ok(c) => {
                self.lists.insert(*pick, c);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        let picks: Vec<Pick> = self.selected().map(|c| c.fields.iter().map(|f| f.pick).collect()).unwrap_or_default();
        for pick in picks {
            match pick {
                Pick::Player => {
                    self.lists.insert(pick, people.clone());
                }
                Pick::Entity => {
                    self.lists.insert(pick, around.clone());
                }
                Pick::EntityName => {
                    let mut c: Choices = around.iter().map(|(_, n)| (n.replace(' ', "_"), n.clone())).collect();
                    c.sort();
                    c.dedup();
                    self.lists.insert(pick, c);
                }
                _ if self.lists.contains_key(&pick) || self.loading.iter().any(|l| l.0 == pick) => {}
                _ => {
                    if let Some(c) = fixed(pick) {
                        self.lists.insert(pick, c);
                    } else if let Some(n) = names.filter(|_| matches!(pick, Pick::Zone | Pick::Item | Pick::Spell | Pick::KeyItem | Pick::Title)) {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let n = n.clone();
                        std::thread::spawn(move || {
                            if let Some(c) = from_names(pick, &n) {
                                let _ = tx.send(c);
                            }
                        });
                        self.loading.push((pick, rx));
                    }
                }
            }
        }
    }

    /// A new zone (maybe another map server): forget the level and list until this one answers 0x1F0.
    /// The typed values and the replies stay.
    pub fn zoned(&mut self) {
        (self.level, self.open, self.typing, self.find_typing, self.armed) = (0, false, None, false, false);
        self.commands.clear();
        self.incoming.clear();
    }

    pub fn selected(&self) -> Option<&Command> {
        self.selected.as_ref().and_then(|n| self.commands.iter().find(|c| &c.name == n))
    }

    pub fn values_of(&self, c: &Command) -> Vec<String> {
        (0..c.fields.len()).map(|i| self.values.get(&(c.name.clone(), i)).cloned().unwrap_or_default()).collect()
    }

    /// c2s 0x1F1's body for a line: u16 RequestId, u8 Length, padding, the line in Shift-JIS (248 bytes at most).
    pub fn run(&mut self, line: String) -> Vec<u8> {
        self.next_id = self.next_id.wrapping_add(1);
        let id = self.next_id;
        let mut text = crate::dat::font::encode_sjis(&line);
        text.truncate(MAX_LINE);
        let mut body = id.to_le_bytes().to_vec();
        body.push(text.len() as u8);
        body.push(0);
        body.extend_from_slice(&text);
        self.replies.push((id, Reply { line, result: None, output: Vec::new(), cut: false }));
        if self.replies.len() > 8 {
            self.replies.remove(0);
        }
        body
    }
}

/// The modern-shot sample: every command of gm_fields.txt at level 5, !additem chosen with values, one reply.
pub fn sample() -> Gm {
    let extra = extras();
    let mut names: Vec<&&str> = extra.keys().collect();
    names.sort();
    let mut g = Gm { level: 5, open: true, ..Default::default() };
    g.commands = names.into_iter().map(|n| command(&extra, n.to_string(), if n.starts_with("reload") { 5 } else { 1 }, "s".repeat(extra[*n].fields.len()))).collect();
    if let Some(c) = g.commands.iter_mut().find(|c| c.name == "additem") {
        c.types = "siiiiiiiiii".into();
    }
    (g.group, g.selected, g.typing) = (Some(2), Some("additem".into()), Some(0));
    g.values.insert(("additem".into(), 0), "crystal".into());
    g.values.insert(("additem".into(), 1), "12".into());
    g.lists.insert(Pick::Item, [(4096, "Fire Crystal"), (4097, "Ice Crystal"), (4098, "Wind Crystal"), (4238, "Inferno Crystal"), (4100, "Lightning Crystal"), (4101, "Water Crystal"), (12, "Fire Cluster")].map(|(i, n)| (i.to_string(), n.to_string())).to_vec());
    g.replies.push((1, Reply { line: "additem 4096 12".into(), result: Some(0), output: vec!["Gave player item 4096 (Fire Crystal) x12.".into()], cut: false }));
    g
}

/// c2s 0x1F0's body: u8 Version (1), padding.
pub const CATALOG_REQUEST: [u8; 4] = [1, 0, 0, 0];

#[cfg(test)]
mod tests {
    use super::*;

    fn page(level: u8, first: u16, last: bool, entries: &[(u8, &str, &str)]) -> Vec<u8> {
        let mut p = vec![0xF0, 0x01, 0, 0, 1, level];
        p.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        p.extend_from_slice(&first.to_le_bytes());
        p.push(entries.len() as u8);
        p.push(last as u8);
        for (perm, n, t) in entries {
            p.extend_from_slice(&[*perm, n.len() as u8, t.len() as u8]);
            p.extend_from_slice(n.as_bytes());
            p.extend_from_slice(t.as_bytes());
        }
        p
    }

    #[test]
    fn fields_table_parses() {
        let e = extras();
        assert!(e.len() > 200);
        let a = &e["additem"];
        assert_eq!((a.group, a.fields[0].pick, a.fields[0].optional), ("items", Pick::Item, false));
        assert!(e["exec"].danger);
    }

    #[test]
    fn piped_labels_and_positional_optionals() {
        let e = extras();
        let goto = &e["gotoid"].fields;
        assert_eq!((goto.len(), goto[0].label.as_str()), (1, "mobId/npcId"));
        assert!(!e["giveitem"].fields[0].optional);
        assert!(e["givegil"].fields[1].optional);
        let c = command(&e, "gotoid".into(), 1, "s".into());
        assert_eq!(c.line(&["12345".into()]).unwrap(), "gotoid 12345");
    }

    #[test]
    fn long_lines_are_refused_not_cut() {
        let c = command(&extras(), "exec".into(), 5, "b".into());
        assert!(c.line(&["x".repeat(300)]).is_err());
    }

    #[test]
    fn suggestions_match_ids_and_names() {
        let c: Choices = vec![("4096".into(), "Fire Crystal".into()), ("4097".into(), "Ice Crystal".into()), ("4112".into(), "Potion".into())];
        fn names<'a>(v: Vec<&'a (String, String)>) -> Vec<&'a str> {
            v.into_iter().map(|c| c.1.as_str()).collect()
        }
        assert_eq!(names(suggest(&c, "crystal", 5)), ["Fire Crystal", "Ice Crystal"]);
        assert_eq!(names(suggest(&c, "ice", 5)), ["Ice Crystal"]);
        assert_eq!(names(suggest(&c, "409", 5)), ["Fire Crystal", "Ice Crystal"]);
        assert_eq!(names(suggest(&c, "potion", 5)), ["Potion"]);
        assert_eq!(fixed(Pick::Job).unwrap()[0].0, "WAR");
    }

    #[test]
    fn catalogue_pages_build_the_list() {
        let mut g = Gm::default();
        g.on_catalog(&page(3, 0, false, &[(1, "additem", "siiiiiiiiii")]));
        assert!(g.commands.is_empty());
        g.on_catalog(&page(3, 1, true, &[(0, "build", ""), (3, "getmod", "s"), (2, "newcmd", "is")]));
        assert_eq!(g.level, 3);
        // Level 0 commands stay out of the GM window.
        assert_eq!(g.commands.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["additem", "getmod", "newcmd"]);
        let n = &g.commands[2];
        assert_eq!((n.group, n.fields.len(), n.fields[0].pick), ("debug", 2, Pick::Number));
    }

    #[test]
    fn lines_follow_the_server_parser() {
        let mut g = Gm::default();
        g.on_catalog(&page(5, 0, true, &[(1, "additem", "siiiiiiiiii"), (1, "pos", "s"), (1, "zone", "b")]));
        let add = g.commands[0].clone();
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(add.line(&v(&["4096", "12"])), Ok("additem 4096 12".into()));
        assert_eq!(add.line(&v(&[])).unwrap_err().0, 0);
        assert_eq!(add.line(&v(&["4096", "", "5"])).unwrap_err().0, 1);
        assert_eq!(add.line(&v(&["4096", "x"])).unwrap_err().0, 1);
        assert_eq!(add.line(&v(&["fire crystal"])).unwrap_err().0, 0);
        assert_eq!(g.commands[1].line(&v(&["1", "2", "3", "230"])), Ok("pos 1 2 3 230".into()));
        assert_eq!(g.commands[2].line(&v(&["230"])), Ok("zone 230".into()));
    }

    #[test]
    fn replies_carry_the_output() {
        let mut g = Gm::default();
        let body = g.run("getid".into());
        let id = u16::from_le_bytes([body[0], body[1]]);
        assert_eq!(&body[2..4], &[5, 0]);
        let mut p = vec![0xF1, 0x01, 0, 0];
        p.extend_from_slice(&id.to_le_bytes());
        p.extend_from_slice(&[0, 1, 2, 0, 2, b'o', b'k', 3, b'1', b'2', b'3']);
        g.on_result(&p);
        let r = &g.replies[0].1;
        assert_eq!((r.result, r.output.clone()), (Some(0), vec!["ok".to_string(), "123".to_string()]));
    }
}
