//! The auto-translate dictionary (language slot 0x18; English file 55665) and the phrase blocks in text
//! (`FD kind lang hi lo FD`, kept by font::decode_sjis as four private-use chars). Spec:
//! ~/ffxi/lab/research/autotranslate_spec.md (loader FUN_101bb7a0, resolver FUN_101bc100, expander FUN_101ac990).

use super::DatIndex;
use super::font::{PHRASE_BASE, decode_sjis};
use super::msg::Messages;
use super::names::NameSource;
use anyhow::{Context, Result};
use std::collections::HashMap;

/// One phrase group: its title (phrase id 0, "【Greetings】") and its phrases by id.
pub struct Group {
    pub id: u8,
    pub title: String,
    pub key: String,
    /// (phrase id, text, completion key) in file order; `@X<hex>` texts stay raw until resolved.
    pub phrases: Vec<(u8, String, String)>,
}

pub struct Dictionary {
    pub groups: Vec<Group>,
    by_id: HashMap<u8, usize>,
    /// Group 0x12 (job names, `@J`), group 0x14 (places, `@A`), group 0xb 0x91 / 0x93 (the item and key item groups'
    /// titles).
    jobs: Option<Messages>,
    places: Option<Messages>,
    titles: [String; 2],
}

/// Header kind 2 a phrase group; language 2 English (the client's language + 1).
const GROUP_KIND: u8 = 2;
const LANG_EN: u8 = 2;

impl Dictionary {
    pub fn load(index: &DatIndex, ffximain: &[u8]) -> Result<Self> {
        let file = super::msg::slot_file(ffximain, 1, 0x18).context("auto-translate slot")?;
        let data = index.read(file)?;
        let msgs = |g: u8| super::msg::group_file(ffximain, 1, g).and_then(|f| Messages::load(index, f).ok());
        let b = msgs(0xb);
        let title = |id: usize| b.as_ref().and_then(|m| m.get(id)).unwrap_or_default();
        let mut d = Self::parse(&data)?;
        d.jobs = msgs(0x12);
        d.places = msgs(0x14);
        d.titles = [title(0x91), title(0x93)];
        Ok(d)
    }

    /// Groups (0x4c-byte header: kind, language, group id, 0, title[32], completion[32], u32 count, u32 bytes) then
    /// entries (4 id bytes, u8 length, text; languages 1 and 4 add a second string). Only English groups are kept.
    pub fn parse(data: &[u8]) -> Result<Self> {
        let mut groups = Vec::new();
        let mut at = 0;
        let cstr = |b: &[u8]| decode_sjis(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]);
        while at + 0x4c <= data.len() {
            let h = &data[at..at + 0x4c];
            let bytes = u32::from_le_bytes(h[0x48..0x4c].try_into().unwrap()) as usize;
            let count = u32::from_le_bytes(h[0x44..0x48].try_into().unwrap()) as usize;
            let body = data.get(at + 0x4c..at + 0x4c + bytes).context("group runs past the file")?;
            at += 0x4c + bytes;
            if h[0] != GROUP_KIND || h[1] != LANG_EN {
                continue;
            }
            let mut phrases = Vec::with_capacity(count);
            let mut p = 0;
            while p + 5 <= body.len() && phrases.len() < count {
                let (lang, id, len) = (body[p + 1], body[p + 3], body[p + 4] as usize);
                let text = cstr(body.get(p + 5..p + 5 + len).context("phrase runs past its group")?);
                p += 5 + len;
                if matches!(lang, 1 | 4) {
                    let l2 = body.get(p).copied().unwrap_or(0) as usize;
                    p += 1 + l2;
                }
                phrases.push((id, text.clone(), text));
            }
            groups.push(Group { id: h[2], title: cstr(&h[4..0x24]), key: cstr(&h[0x24..0x44]), phrases });
        }
        let by_id = groups.iter().enumerate().map(|(i, g)| (g.id, i)).collect();
        Ok(Dictionary { groups, by_id, jobs: None, places: None, titles: Default::default() })
    }

    /// `@A` place, `@C` spell, `@J` job, `@Y` ability by hex id (FUN_101ba5d0); other texts as they are.
    fn named(&self, text: &str, names: Option<&dyn NameSource>) -> Option<String> {
        let Some(rest) = text.strip_prefix('@') else { return Some(text.to_string()) };
        let (kind, hex) = rest.split_at(rest.len().min(1));
        let id = u32::from_str_radix(hex, 16).ok()?;
        match kind {
            "A" => self.places.as_ref().and_then(|m| m.get(id as usize)),
            "J" => self.jobs.as_ref().and_then(|m| m.get(id as usize)),
            "C" => names?.spell(id),
            "Y" => names?.ability(id),
            _ => None,
        }
    }

    /// FUN_101bc100: a block's text (the language byte is ignored: the client's own dictionary answers). Kinds 7..0x12
    /// are items, 0x13.. key items (00 bytes replaced by FF: 8 / 0xc / 0x10 / 0x14 id 0, 9.. lo only, 0xa.. hi only),
    /// id 0 the group's title; else a phrase (id 0 the group's title). None: unknown (the line ends there).
    pub fn resolve(&self, b: [u8; 4], names: Option<&dyn NameSource>) -> Option<String> {
        let [kind, _, hi, lo] = b;
        if kind >= 7 {
            let id = match (kind - 7) % 4 {
                0 => (hi as u32) << 8 | lo as u32,
                1 => 0,
                2 => lo as u32,
                _ => (hi as u32) << 8,
            };
            let key = kind >= 0x13;
            if id == 0 {
                return Some(self.titles[key as usize].clone()).filter(|t| !t.is_empty());
            }
            let n = names?;
            return if key { n.key_item(id).map(|x| x.bare) } else { n.item(id).map(|x| x.bare) };
        }
        if kind == 6 {
            return None;
        }
        let g = &self.groups[*self.by_id.get(&hi)?];
        if lo == 0 {
            return Some(g.title.clone());
        }
        let (_, text, _) = g.phrases.iter().find(|p| p.0 == lo)?;
        self.named(text, names)
    }

    /// FUN_101ac990 with braces (the log): each block becomes its text between the green and red auto-translate
    /// brackets (EF 27 / EF 28, U+E008 / U+E009); an unknown block cuts the line there.
    pub fn expand(&self, s: &str, names: Option<&dyn NameSource>) -> String {
        let mut out = String::with_capacity(s.len());
        let mut block = [0u8; 4];
        for ch in s.chars() {
            let cp = ch as u32;
            if !(PHRASE_BASE..PHRASE_BASE + 0x400).contains(&cp) {
                out.push(ch);
                continue;
            }
            let k = ((cp - PHRASE_BASE) / 0x100) as usize;
            block[k] = cp as u8;
            if k < 3 {
                continue;
            }
            match self.resolve(block, names) {
                Some(t) => {
                    out.push('\u{E008}');
                    out.push_str(&t);
                    out.push('\u{E009}');
                }
                None => break,
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File 55665: 42 English groups; Greetings 1 is "Nice to meet you."; @J / @C / @Y name references resolve; a
    /// block in a chat line expands between the brackets.
    #[test]
    fn english_dictionary() {
        let cfg = crate::config::Config::load();
        let Ok(index) = DatIndex::open(&cfg.ffxi_dir()) else { return };
        let Ok(dll) = std::fs::read(cfg.ffxi_dir().join("FFXiMain.dll")) else { return };
        let d = Dictionary::load(&index, &dll).unwrap();
        assert_eq!(d.groups.len(), 42);
        let names = crate::dat::names::Names::load(&index, crate::dat::items::Lang::En).unwrap();
        let n: Option<&dyn NameSource> = Some(&names);
        assert_eq!(d.resolve([2, 2, 1, 1], n).as_deref(), Some("Nice to meet you."));
        assert_eq!(d.resolve([2, 2, 0x10, 1], n).as_deref(), Some("Thief"));
        assert_eq!(d.resolve([2, 2, 0x1b, 1], n).as_deref(), Some("Protect"));
        assert_eq!(d.resolve([2, 2, 0x2a, 0x27], n).as_deref(), Some("Soul Enslavement"));
        let line = decode_sjis(b"Hi \xFD\x02\x02\x01\x01\xFD");
        assert_eq!(d.expand(&line, n), "Hi \u{E008}Nice to meet you.\u{E009}");
    }
}
