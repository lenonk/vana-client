//! Malformed-input tests for the DAT parsers (issue #87): every parser that takes file bytes must return (an error,
//! an empty result, anything) on random and chunk-shaped input, never panic, abort or hang. DATs come from the
//! install and from mod packs (`modpack.rs`, `overlay.rs`), so one corrupt file must not take the client down.

use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// xorshift64: deterministic, so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    /// Bytes biased towards the values headers trip on: 0, 0xff, small counts and offsets.
    fn byte(&mut self) -> u8 {
        let x = self.next();
        match x % 8 {
            0 | 1 => 0,
            2 => 0xff,
            3 | 4 => (x >> 8) as u8 & 0x1f,
            _ => (x >> 8) as u8,
        }
    }

    fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.byte()).collect()
    }
}

const MAGICS: [&[u8]; 4] = [b"d_msg", b"MMB", b"menu", b"MZB"];

/// Random bytes, sometimes starting with a magic most parsers check first.
fn flat(r: &mut Rng) -> Vec<u8> {
    let len = match r.below(4) {
        0 => r.below(32),
        1 => r.below(128),
        _ => r.below(1024),
    };
    let mut b = r.bytes(len);
    if r.below(4) == 0 {
        let m = MAGICS[r.below(MAGICS.len())];
        b.splice(0..m.len().min(b.len()), m.iter().copied());
    }
    b
}

/// A chunk file (`chunks`): 16-byte headers with a kind and a size that fits, random payloads.
fn chunked(r: &mut Rng) -> Vec<u8> {
    // The kinds the parsers here match on: zone scenery (0x1c MZB, 0x20 IMG, 0x2e MMB, 0x36 RID), title / event
    // (0x06 camera, 0x07 scheduler, 0x19 KeyFrame), folders (0x01) and others.
    const KINDS: [u8; 13] = [0x01, 0x06, 0x07, 0x19, 0x1c, 0x1f, 0x20, 0x29, 0x2a, 0x2b, 0x2e, 0x36, 0x3f];
    let mut out = Vec::new();
    for _ in 0..1 + r.below(4) {
        let size = 16 * (1 + r.below(24));
        let kind = if r.below(4) == 0 { r.below(0x80) as u8 } else { KINDS[r.below(KINDS.len())] };
        let word = kind as u32 | ((size / 16) as u32) << 7;
        out.extend_from_slice(&r.bytes(4));
        out.extend_from_slice(&word.to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend(r.bytes(size - 16));
    }
    out
}

fn inputs(seed: u64, n: usize) -> impl Iterator<Item = Vec<u8>> {
    let mut r = Rng(seed);
    (0..n).map(move |i| if i % 2 == 0 { flat(&mut r) } else { chunked(&mut r) })
}

/// Runs `f` on many inputs (and on each chunk payload of the chunk-shaped ones), returning the first input it panics on.
fn first_panic(seed: u64, f: &dyn Fn(&[u8])) -> Option<Vec<u8>> {
    for b in inputs(seed, 4000) {
        let mut cases = vec![b.clone()];
        if let Ok(cs) = chunks(&b) {
            cases.extend(cs.iter().map(|c| c.data.to_vec()));
        }
        for c in cases {
            if catch_unwind(AssertUnwindSafe(|| f(&c))).is_err() {
                return Some(c);
            }
        }
    }
    None
}

type Parser = Box<dyn Fn(&[u8])>;

#[test]
fn parsers_survive_malformed_input() {
    let parsers: Vec<(&str, Parser)> = vec![
        ("chunks", Box::new(|b| { let _ = chunks(b); })),
        ("autotrans::Dictionary::parse", Box::new(|b| { let _ = autotrans::Dictionary::parse(b); })),
        ("bump::parse", Box::new(|b| { let _ = bump::parse(b); })),
        ("character::load_file", Box::new(|b| { let _ = character::load_file(b); })),
        ("character::parse_skeleton", Box::new(|b| { let _ = character::parse_skeleton(String::new(), b); })),
        ("character::parse_mesh", Box::new(|b| { let _ = character::parse_mesh("", b); })),
        ("character::parse_animation", Box::new(|b| { let _ = character::parse_animation(String::new(), b); })),
        ("collision::parse", Box::new(|b| { let _ = collision::parse(b); })),
        ("cursor::parse", Box::new(|b| { let _ = cursor::parse(b); })),
        ("cursor::parse_cur", Box::new(|b| { let _ = cursor::parse_cur(b); })),
        ("dancer::parse_channels", Box::new(|b| { let _ = dancer::parse_channels(b); })),
        ("dialog::Dialog::parse", Box::new(|b| { let _ = dialog::Dialog::parse(b); })),
        ("dialog::readable", Box::new(|b| { let _ = dialog::readable(b); })),
        ("dialog::expand", Box::new(|b| { let _ = dialog::expand(b, &dialog::Ctx::default()); })),
        ("event::parse", Box::new(|b| { let _ = event::parse(b); })),
        ("furniture::FurnTable::parse", Box::new(|b| { let _ = furniture::FurnTable::parse(b); })),
        ("generator::Tree::build", Box::new(|b| { let _ = generator::Tree::build(b); })),
        ("generator::parse_ops", Box::new(|b| { let _ = generator::parse_ops(b, 0); })),
        ("generator::KeyFrame::parse", Box::new(|b| { let _ = generator::KeyFrame::parse(b); })),
        ("generator::D3m::parse", Box::new(|b| { let _ = generator::D3m::parse(b); })),
        ("items::string_table", Box::new(|b| { let _ = items::string_table(b); })),
        ("msg::Messages::parse", Box::new(|b| { let _ = msg::Messages::parse(b); })),
        ("names::DMsg::parse", Box::new(|b| { let _ = names::DMsg::parse(b); })),
        ("npc::parse_look", Box::new(|b| { let _ = npc::parse_look(b); })),
        ("npc::init_scheduler", Box::new(|b| { let _ = npc::init_scheduler(b); })),
        ("sched::ops", Box::new(|b| { let _ = sched::ops(b); })),
        ("sched::loops", Box::new(|b| { let _ = sched::loops(b); })),
        ("sched::auto_start", Box::new(|b| { let _ = sched::auto_start(b); })),
        ("sched::span", Box::new(|b| { let _ = sched::span(b); })),
        ("sched::length", Box::new(|b| { let _ = sched::length(b); })),
        ("sched::plan", Box::new(|b| { let _ = sched::plan(b, &["x"]); })),
        ("sched::hand_calls", Box::new(|b| { let _ = sched::hand_calls(b); })),
        ("statusinfo::entry", Box::new(|b| { let _ = statusinfo::entry(b, 1); })),
        ("statusinfo::icon", Box::new(|b| { let _ = statusinfo::icon(b, 1); })),
        ("title::parse_ops", Box::new(|b| { let _ = title::parse_ops(b); })),
        ("title::Title::parse", Box::new(|b| { let _ = title::Title::parse(b); })),
        ("ui::parse_sprite_lib", Box::new(|b| { let _ = ui::parse_sprite_lib(b); })),
        ("ui::parse_menu", Box::new(|b| { let _ = ui::parse_menu(b); })),
        ("ui::UiFile::parse", Box::new(|b| { let _ = ui::UiFile::parse(b); })),
        ("weather::Env::parse", Box::new(|b| { let _ = weather::Env::parse(b); })),
        ("weather::ZoneWeather::parse", Box::new(|b| { let _ = weather::ZoneWeather::parse(b); })),
        ("zone::parse_mmb", Box::new(|b| { let _ = zone::parse_mmb(b); })),
        ("zone::sway_curves", Box::new(|b| { let _ = zone::sway_curves(b); })),
        ("zone::camera_lists", Box::new(|b| { let _ = zone::camera_lists(b); })),
        ("zone::light_list", Box::new(|b| { let _ = zone::light_list(b); })),
        ("zone::load_scene", Box::new(|b| { let _ = zone::load_scene(&zone::Keys::for_tests(), b); })),
        ("zone::decode_mzb", Box::new(|b| { let _ = zone::decode_mzb(&zone::Keys::for_tests(), &mut b.to_vec()); })),
        ("zone::decode_mmb", Box::new(|b| zone::decode_mmb(&zone::Keys::for_tests(), &mut b.to_vec()))),
        ("zone::parse_mzb", Box::new(|b| { let _ = zone::parse_mzb(&mut b.to_vec()); })),
        ("zone::parse_img", Box::new(|b| { let _ = zone::parse_img(b); })),
        ("zone::parse_rid", Box::new(|b| { let _ = zone::parse_rid(b); })),
    ];
    let failed: Vec<String> = parsers
        .iter()
        .enumerate()
        .filter_map(|(i, (name, f))| first_panic(0x9e37_79b9_7f4a_7c15 ^ i as u64, f.as_ref()).map(|b| format!("{name}: {:02x?}", &b[..b.len().min(96)])))
        .collect();
    assert!(failed.is_empty(), "{} parser(s) panicked:\n{}", failed.len(), failed.join("\n"));
}

/// One chunk of each kind with an empty or 16-byte payload, through the parsers that dispatch on the kind.
#[test]
fn empty_chunks_of_every_kind() {
    for kind in 0..0x80u32 {
        for size in [16usize, 32] {
            let mut file = vec![0u8; size];
            file[4..8].copy_from_slice(&(kind | ((size / 16) as u32) << 7).to_le_bytes());
            let keys = zone::Keys::for_tests();
            for (name, f) in [("load_scene", &(|b: &[u8]| drop(zone::load_scene(&keys, b))) as &dyn Fn(&[u8])), ("Title::parse", &|b: &[u8]| drop(title::Title::parse(b)))] {
                assert!(catch_unwind(AssertUnwindSafe(|| f(&file))).is_ok(), "{name} panicked on an empty chunk of kind {kind:#x}, size {size}");
            }
        }
    }
}
