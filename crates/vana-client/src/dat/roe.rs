//! Records of Eminence records in the item record store (FUN_1015b9e0; roe_lists_spec.md 1): objectives 0xe000 + n
//! (FUN_1015cb60) and the Objective List's category tree 0xf200.. (FUN_1015cf20, root 0xf200).

use super::items::{Field, Items, Lang, string_table};

/// The Objective List's root category.
pub const ROOT: u16 = 0xf200;

#[derive(Clone, Debug, Default)]
pub struct Objective {
    pub repeatable: bool,
    /// The "%d/%d" denominator.
    pub goal: u32,
    pub sparks: u32,
    pub exp: u32,
    pub cap: u32,
    pub acc: u32,
    pub name: String,
    /// Description pages 1 and 2 (2 is set on few records).
    pub pages: [String; 2],
}

#[derive(Clone, Debug, Default)]
pub struct Category {
    pub name: String,
    /// Children (a category >= 0xf200 or an objective 0xe000..) with their four conditions {b0, b1, b2, type}.
    pub kids: Vec<(u16, [[u8; 4]; 4])>,
}

fn u32_at(r: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(r[o..o + 4].try_into().unwrap())
}

/// FUN_1015b190: a loaded record has its id at +0 and 0xff as its last byte.
fn record(items: &Items, id: u16) -> Option<Vec<u8>> {
    let (_, r) = items.record(id as u32)?;
    (r.len() == 0x1400 && u32_at(&r, 0) == id as u32 && r[0x13ff] == 0xff).then_some(r)
}

/// FUN_1015be00: field 0 is the name, the `n` descriptions follow the language's extra fields (ja 1, en 4, de 8, fr
/// 5); a count that does not match leaves them all empty.
fn texts(r: &[u8], n: usize, lang: Lang) -> (String, Vec<String>) {
    let start = match lang {
        Lang::Ja => 1,
        Lang::En => 4,
        Lang::De => 8,
        Lang::Fr => 5,
    };
    let f = string_table(r);
    if f.len() != n + start {
        return (String::new(), vec![String::new(); n]);
    }
    let s = |x: &Field| if let Field::Text(t) = x { t.clone() } else { String::new() };
    (s(&f[0]), f[start..].iter().map(s).collect())
}

pub fn objective(items: &Items, id: u16) -> Option<Objective> {
    let r = record(items, id)?;
    let (name, d) = texts(&r[0x20..], 2, items.lang());
    Some(Objective {
        repeatable: u32_at(&r, 8) != 0,
        goal: u32_at(&r, 0xc),
        sparks: u32_at(&r, 0x10),
        exp: u32_at(&r, 0x14),
        cap: u32_at(&r, 0x18),
        acc: u32_at(&r, 0x1c),
        name,
        pages: [d[0].clone(), d[1].clone()],
    })
}

pub fn category(items: &Items, id: u16) -> Option<Category> {
    let r = record(items, id)?;
    let n = (u16::from_le_bytes([r[4], r[5]]) as usize).min(28);
    let kids = (0..n)
        .map(|i| {
            let o = 8 + i * 20;
            (u16::from_le_bytes([r[o], r[o + 1]]), std::array::from_fn(|k| r[o + 4 + 4 * k..o + 8 + 4 * k].try_into().unwrap()))
        })
        .collect();
    let (name, _) = texts(&r[0x238..], 1, items.lang());
    Some(Category { name, kids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dat::DatIndex;

    #[test]
    fn real_records() {
        let Ok(index) = DatIndex::open(&crate::config::Config::load().ffxi_dir()) else { return };
        let Ok(items) = Items::load(&index, Lang::En) else { return };
        let root = category(&items, ROOT).expect("root");
        let ids: Vec<u16> = root.kids.iter().map(|k| k.0).collect();
        assert_eq!(ids, [0xf210, 0xf211, 0xf212, 0xf213, 0xf214, 0xf215, 0xf216, 0xf217, 0xf218, 0xf219, 0xf21b]);
        assert_eq!(root.kids[1].1[0], [1, 0, 0, 1]);
        assert_eq!(category(&items, 0xf210).unwrap().name, "Tutorial");
        let basics = category(&items, 0xf230).unwrap();
        assert_eq!(basics.kids[0].0, 0xe001);
        let o = objective(&items, 0xe001).unwrap();
        assert_eq!((o.name.as_str(), o.repeatable, o.goal, o.sparks, o.exp), ("First Step Forward", false, 1, 100, 300));
        let o = objective(&items, 0xe00c).unwrap();
        assert_eq!((o.repeatable, o.goal, o.sparks, o.acc), (true, 200, 1000, 100));
        assert!(!objective(&items, 0xe331).unwrap().pages[1].is_empty());
        // The whole tree: 121 non-empty categories, 1503 objectives.
        let (mut cats, mut objs, mut todo) = (0, 0, vec![ROOT]);
        while let Some(c) = todo.pop() {
            let Some(cat) = category(&items, c) else { continue };
            cats += !cat.kids.is_empty() as usize;
            for (k, _) in cat.kids {
                match k {
                    0xf200.. => todo.push(k),
                    0xe001..=0xeff7 => objs += 1,
                    _ => {}
                }
            }
        }
        assert_eq!((cats, objs), (121, 1503));
    }
}
