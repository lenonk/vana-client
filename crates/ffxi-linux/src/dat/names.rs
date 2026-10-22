//! Name tables the dialogue expander reads, per language (FUN_10250e50 slots):
//! key items 0x3d (FUN_1014e010), spells 0x5a (FUN_10221eb0), abilities 0x5c (FUN_10219b90,
//! job abilities at 0x200 + id), places 0x10 (message group 0x14), titles 0x3c (FUN_10150790).
//! All are "d_msg" files; entries are field lists, either indexed by a table or fixed-size.

use super::DatIndex;
use super::font::decode_sjis;
use super::items::{Field, Items, Lang};
use anyhow::{Context, Result, bail};

/// File ids per language (ja, en, de, fr) from the FUN_10250de0 tables; tools/langtab.py prints them.
pub const KEY_ITEMS: [u32; 4] = [55585, 55705, 55825, 56245];
pub const SPELLS: [u32; 4] = [55582, 55702, 55822, 56242];
pub const ABILITIES: [u32; 4] = [55581, 55701, 55821, 56241];
pub const PLACES: [u32; 4] = [55535, 55465, 55775, 56195];
pub const TITLES: [u32; 4] = [55584, 55704, 55824, 56244];

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// A "d_msg" file. Header: +0xa u16 1 = XOR 0xff, +0x18 header size, +0x1c table size (0 = fixed
/// entries of +0x20 bytes), +0x28 count.
pub struct DMsg {
    entries: Vec<Vec<Field>>,
}

impl DMsg {
    pub fn load(index: &DatIndex, file: u32) -> Result<DMsg> {
        Self::parse(&index.read(file)?).with_context(|| format!("d_msg file {file}"))
    }

    pub fn parse(d: &[u8]) -> Result<DMsg> {
        if !d.starts_with(b"d_msg") {
            bail!("not a d_msg file");
        }
        let xor = if d.get(0xa..0xc) == Some(&[1, 0]) { 0xff } else { 0 };
        let (h, t, fixed, n) = (u32_at(d, 0x18).context("header")? as usize, u32_at(d, 0x1c).context("header")? as usize, u32_at(d, 0x20).context("header")? as usize, u32_at(d, 0x28).context("header")? as usize);
        let tab: Vec<u8> = d.get(h..h + t).context("table")?.iter().map(|b| b ^ xor).collect();
        let body: Vec<u8> = d.get(h + t..).context("body")?.iter().map(|b| b ^ xor).collect();
        let mut entries = Vec::with_capacity(n);
        for i in 0..n {
            let e = if t == 0 {
                body.get(i * fixed..(i + 1) * fixed).unwrap_or_default()
            } else {
                let (o, l) = (u32_at(&tab, i * 8).unwrap_or(0) as usize, u32_at(&tab, i * 8 + 4).unwrap_or(0) as usize);
                body.get(o..o + l).unwrap_or_default()
            };
            entries.push(fields(e));
        }
        Ok(DMsg { entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn fields(&self, i: usize) -> Option<&[Field]> {
        self.entries.get(i).map(|v| v.as_slice())
    }

    /// First text field of entry `i`.
    pub fn text(&self, i: usize) -> Option<String> {
        self.entries.get(i)?.iter().find_map(|f| if let Field::Text(s) = f { Some(s.clone()) } else { None })
    }
}

/// Entry fields: u32 count, (offset, type) pairs; type 0 = text 0x1c bytes in, else a u32.
fn fields(e: &[u8]) -> Vec<Field> {
    let n = u32_at(e, 0).unwrap_or(0) as usize;
    if n == 0 || n > 64 {
        return Vec::new();
    }
    (0..n)
        .filter_map(|j| {
            let (o, ty) = (u32_at(e, 4 + j * 8)? as usize, u32_at(e, 8 + j * 8)?);
            Some(if ty == 0 {
                let s = e.get(o + 0x1c..).unwrap_or_default();
                Field::Text(decode_sjis(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())]))
            } else {
                Field::Num(u32_at(e, o).unwrap_or(0))
            })
        })
        .collect()
}

/// An English noun as the article builder sees it (FUN_1014fda0 / FUN_1014e700 objects).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Noun {
    /// vt+0x18: the bare singular.
    pub bare: String,
    /// vt+4: the singular with "pair of" / "suit of".
    pub singular: String,
    pub plural: String,
    /// vt+0x1c: "an" for the bare form.
    pub an: bool,
    /// vt+0x14: "an" for the vt+4 form.
    pub singular_an: bool,
    /// vt+0x20: 3 countable, 2 mass noun ("some").
    pub mode: u8,
}

/// FUN_1024e280: a leading vowel takes "an".
pub fn vowel(s: &str) -> bool {
    s.starts_with(|c: char| "aieuoAIUEO".contains(c))
}

/// Names the dialogue expander fills in.
pub trait NameSource: Send + Sync {
    fn item(&self, id: u32) -> Option<Noun>;
    fn key_item(&self, id: u32) -> Option<Noun>;
    fn spell(&self, id: u32) -> Option<String>;
    /// Weapon skills < 0x200, job abilities at 0x200 + id.
    fn ability(&self, id: u32) -> Option<String>;
    fn place(&self, id: u32) -> Option<String>;
    fn title(&self, id: u32) -> Option<String>;
}

/// All tables of one language.
pub struct Names {
    pub lang: Lang,
    pub items: Items,
    key_items: DMsg,
    key_index: std::collections::HashMap<u32, usize>,
    spells: DMsg,
    abilities: DMsg,
    places: DMsg,
    titles: DMsg,
}

impl Names {
    pub fn load(index: &DatIndex, lang: Lang) -> Result<Names> {
        let l = lang as usize;
        let key_items = DMsg::load(index, KEY_ITEMS[l])?;
        // FUN_1014dc30: key items are found by field 0 (the id), not by position.
        let key_index = (0..key_items.len())
            .filter_map(|i| match key_items.fields(i)?.first()? {
                Field::Num(id) => Some((*id, i)),
                _ => None,
            })
            .collect();
        Ok(Names {
            lang,
            items: Items::load(index, lang)?,
            key_items,
            key_index,
            spells: DMsg::load(index, SPELLS[l])?,
            abilities: DMsg::load(index, ABILITIES[l])?,
            places: DMsg::load(index, PLACES[l])?,
            titles: DMsg::load(index, TITLES[l])?,
        })
    }

    /// Key item fields (FUN_1014d7e0): ja [id, name, desc]; en [id, type, -, -, singular, plural, desc].
    pub fn key_item_fields(&self, id: u32) -> Option<&[Field]> {
        self.key_items.fields(*self.key_index.get(&id)?)
    }

    pub fn key_item_description(&self, id: u32) -> Option<String> {
        // FUN_1014dba0: description field ja 2, en 6, de 8, fr 10.
        let at = [2, 6, 8, 10][self.lang as usize];
        match self.key_item_fields(id)?.get(at)? {
            Field::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

fn text_at(f: &[Field], i: usize) -> String {
    match f.get(i) {
        Some(Field::Text(s)) => s.clone(),
        _ => String::new(),
    }
}

/// A name with no English grammar (other languages): every form is the name.
fn plain(name: String) -> Noun {
    Noun { bare: name.clone(), singular: name.clone(), plural: name, an: false, singular_an: false, mode: 3 }
}

impl NameSource for Names {
    fn item(&self, id: u32) -> Option<Noun> {
        let it = self.items.item(id)?;
        if self.lang != Lang::En || it.singular.is_empty() {
            return Some(plain(it.name));
        }
        // FUN_1014fda0 and its vtable 0x10329b1c.
        let pair = matches!(it.article, 2 | 3);
        let an = match it.article {
            0 => false,
            1 => true,
            _ => vowel(&it.singular),
        };
        let prefix = match it.article {
            2 => "pair of ",
            3 => "suit of ",
            _ => "",
        };
        Some(Noun {
            bare: it.singular.clone(),
            singular: format!("{prefix}{}", it.singular),
            plural: it.plural,
            an,
            singular_an: if pair { false } else { an },
            mode: if pair { 2 } else { 3 },
        })
    }

    fn key_item(&self, id: u32) -> Option<Noun> {
        let f = self.key_item_fields(id)?;
        if self.lang != Lang::En {
            let name = f.iter().find_map(|x| if let Field::Text(s) = x { (!s.is_empty()).then(|| s.clone()) } else { None })?;
            return Some(plain(name));
        }
        // FUN_1014d7e0 (English) -> FUN_1014e900 -> object vtable 0x10329aa0.
        let ty = if let Some(Field::Num(t)) = f.get(1) { *t } else { 0 };
        let (sing, plur) = (text_at(f, 4), text_at(f, 5));
        Some(match ty {
            4 => Noun { bare: sing.clone(), singular: sing.clone(), plural: sing.clone(), an: vowel(&sing), singular_an: vowel(&sing), mode: 2 },
            2 | 3 => Noun { bare: sing.clone(), singular: sing, plural: plur, an: ty == 3, singular_an: ty == 3, mode: 3 },
            _ => Noun { an: vowel(&sing), singular_an: vowel(&sing), bare: sing.clone(), singular: sing, plural: plur, mode: 3 },
        })
    }

    fn spell(&self, id: u32) -> Option<String> {
        self.spells.text(id as usize)
    }

    fn ability(&self, id: u32) -> Option<String> {
        self.abilities.text(id as usize)
    }

    fn place(&self, id: u32) -> Option<String> {
        self.places.text(id as usize)
    }

    fn title(&self, id: u32) -> Option<String> {
        self.titles.text(id as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vowels() {
        assert!(vowel("adventurer coupon"));
        assert!(!vowel("linkshell"));
    }

    struct Stub;
    impl NameSource for Stub {
        fn item(&self, id: u32) -> Option<Noun> {
            let n = |b: &str, p: &str| Noun { bare: b.into(), singular: b.into(), plural: p.into(), an: vowel(b), singular_an: vowel(b), mode: 3 };
            match id {
                513 => Some(n("linkshell", "linkshells")),
                536 => Some(n("adventurer coupon", "adventurer coupons")),
                _ => None,
            }
        }
        fn key_item(&self, _: u32) -> Option<Noun> {
            None
        }
        fn spell(&self, _: u32) -> Option<String> {
            None
        }
        fn ability(&self, _: u32) -> Option<String> {
            None
        }
        fn place(&self, id: u32) -> Option<String> {
            (id == 230).then(|| "Southern San d'Oria".into())
        }
        fn title(&self, _: u32) -> Option<String> {
            None
        }
    }

    #[test]
    fn grammar_blocks() {
        use crate::dat::dialog::{Ctx, expand};
        let ctx = Ctx { num: [536, 513, 0, 0, 0, 0, 0, 0], names: Some(std::sync::Arc::new(Stub)), ..Default::default() };
        let line = |s: &[u8]| String::from_utf8(expand(s, &ctx).lines.concat()).unwrap();
        // zone 230 #8311: article block, space, item-with-article block (num1)
        assert_eq!(line(b"called \x01\x01\x01 \x01\x05\x24\x82\x81\x80\x80."), "called a linkshell.");
        // #7704: bare item name (num0); an article before a vowel
        assert_eq!(line(b"take this \x01\x05\x23\x82\x80\x80\x80."), "take this adventurer coupon.");
        assert_eq!(line(b"\x01\x01\x01 \x01\x05\x24\x82\x80\x80\x80"), "an adventurer coupon");
        // plural form, place name by index
        assert_eq!(line(b"\x01\x05\x25\x82\x81\x80\x80 in \x01\x05\x37\x82\xe6\x81\x80"), "linkshells in Southern San d'Oria");
    }

    #[test]
    fn real_tables() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(n) = Names::load(&index, Lang::En) else { return };
        assert_eq!(n.spell(1).as_deref(), Some("Cure"));
        assert_eq!(n.ability(0x200 + 16).as_deref(), Some("Mighty Strikes"));
        assert_eq!(n.place(230).as_deref(), Some("Southern San d'Oria"));
        assert_eq!(n.title(1).as_deref(), Some("Fodderchief Flayer"));
        assert_eq!(n.key_item(1).map(|k| k.singular), Some("Zeruhn report".to_string()));
        let ls = n.item(513).expect("linkshell");
        assert_eq!((ls.bare.as_str(), ls.an), ("linkshell", false));
    }
}
