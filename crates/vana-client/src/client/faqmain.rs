//! Help Desk's GM call (spec ~/ffxi/lab/research/social_rows_spec.md H3 / H4): "menu    faqmain" walking the
//! questionnaire of DAT 5813 (Shift-JIS text: nodes, slots, DISP conditions, links) through its select, question and
//! leaf pages (FUN_102185d0 / FUN_10218ac0 / FUN_102194a0), the question's text field, and the report c2s 0x0D3
//! (Return on a question or leaf page: type 1; Execute: type 2 with the queue line, or 3 / 1), c2s 0x0D4 on open and
//! 0x0F0 for EXEC.RESCUE; the GM reply page (s2c 0x0B6 assembled into the one-node gmNotice script, c2s 0x0D5 on
//! "Delete this message."). Not done: the language row (area code 2 only), the mouse, netstat's reply mark.

use super::World;
use super::game_ui::GameUi;
use crate::audio::Ui;
use std::collections::HashMap;

const WIN: &str = "menu    faqmain";
const WHITE: u32 = 0x8080_8080;
const FILE: u32 = 5813;
/// Row indent (+0x3c), its right margin (+0x3e), the gap above a slot (+0x40) and above the closing rows (+0x42), the
/// bottom margin the scroll keeps (+0x38).
const INDENT: f32 = 20.0;
const RIGHT: f32 = 10.0;
const GAP: f32 = 10.0;
const GAP2: f32 = 15.0;
const BOTTOM: f32 = 20.0;
/// The question field's room (FUN_101c5be0 mode 8, 150 chars).
const FIELD_MAX: usize = 150;

#[derive(Debug, Default)]
pub struct Script {
    version: u32,
    nodes: HashMap<u16, Node>,
    /// LINK records: (slot, from node, to node).
    links: Vec<(u16, u16, u16)>,
}

#[derive(Debug, Default)]
struct Node {
    question: bool,
    /// The English column of HEADER.
    header: String,
    evals: Vec<String>,
    slots: Vec<Slot>,
    disp: Vec<Disp>,
}

#[derive(Debug)]
struct Slot {
    id: u16,
    text: String,
    /// "Input:REQUIRE" slots: the parameter the field fills.
    var: Option<String>,
}

#[derive(Debug)]
struct Disp {
    slot: u16,
    var: String,
    eq: bool,
    value: String,
}

/// One line's words: bare tokens, quoted strings with \n / \" / \\ undone. The Japanese column is read two bytes at a
/// time where a Shift-JIS lead byte appears (its trail byte may be '\\'); the others byte by byte.
fn tokens(line: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    let slot = line.starts_with(b"SLOT");
    while i < line.len() {
        match line[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            b'"' => {
                let k = out.iter().filter(|t: &&Vec<u8>| t.first() == Some(&0)).count();
                let input = slot && out.get(2).is_some_and(|t: &Vec<u8>| t[1..].starts_with(b"Input:"));
                let sjis = k == 0 || (input && k == 2);
                let mut s = vec![0u8];
                i += 1;
                while i < line.len() && line[i] != b'"' {
                    let c = line[i];
                    if sjis && matches!(c, 0x81..=0x9f | 0xe0..=0xfc) && i + 1 < line.len() {
                        s.extend_from_slice(&line[i..i + 2]);
                        i += 2;
                    } else if c == b'\\' && i + 1 < line.len() {
                        s.push(match line[i + 1] {
                            b'n' => b'\n',
                            o => o,
                        });
                        i += 2;
                    } else {
                        s.push(c);
                        i += 1;
                    }
                }
                i += 1;
                out.push(s);
            }
            _ => {
                let start = i;
                while i < line.len() && !matches!(line[i], b' ' | b'\t' | b'\r') {
                    i += 1;
                }
                out.push(line[start..i].to_vec());
            }
        }
    }
    out
}

/// The session's parse (FUN_10112bd0 over the file): DEF DATASET, SELECTNODE / QUESTIONNODE with HEADER, EVAL, SLOT,
/// DISP, and LINK blocks of UP records.
pub fn parse(b: &[u8]) -> Script {
    let mut s = Script::default();
    let mut cur: Option<(u16, Node)> = None;
    for line in b.split(|&c| c == b'\n') {
        let t = tokens(line);
        // Quoted strings carry a 0 marker byte in front.
        let word = |k: usize| t.get(k).map_or(&b""[..], |v| if v.first() == Some(&0) { &v[1..] } else { &v[..] });
        let text = |k: usize| crate::dat::font::decode_sjis(word(k));
        let num = |k: usize| std::str::from_utf8(word(k)).ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
        match (word(0), word(1)) {
            (b"DEF", b"DATASET") => s.version = num(2),
            (b"DEF", b"SELECTNODE" | b"QUESTIONNODE") => cur = Some((num(2) as u16, Node { question: word(1) == b"QUESTIONNODE", ..Default::default() })),
            (b"END", b"NODE") => {
                if let Some((id, n)) = cur.take() {
                    s.nodes.insert(id, n);
                }
            }
            (b"UP", _) => s.links.push((num(1) as u16, num(2) as u16, num(3) as u16)),
            (k, _) => {
                let Some((_, n)) = cur.as_mut() else { continue };
                match k {
                    b"HEADER" => n.header = text(2),
                    b"EVAL" => n.evals.push(text(1)),
                    b"SLOT" if word(2).starts_with(b"Input:") => n.slots.push(Slot { id: num(1) as u16, text: text(5), var: Some(text(3)) }),
                    b"SLOT" => n.slots.push(Slot { id: num(1) as u16, text: text(3), var: None }),
                    b"DISP" => n.disp.push(Disp { slot: num(1) as u16, var: text(2), eq: word(3) == b"S:eq", value: text(4) }),
                    _ => {}
                }
            }
        }
    }
    s
}

/// The values the DISP conditions and the report read *(inf: the client's evaluator is not traced; PC, in the field)*.
fn value(var: &str) -> &'static str {
    match var {
        "GENERIC.PLATFORM" => "win",
        "GAME.INFIELD" => "on",
        _ => "",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Select,
    Question,
    Leaf,
}

pub struct FaqMain {
    script: Script,
    /// The walk: the root first, the page shown last.
    path: Vec<u16>,
    /// +0x14: a slot row (1..), Execute (slots + 1), 0 Go back, -1 Return.
    cursor: i16,
    scroll: f32,
    follow: bool,
    /// The active text field (+0x344) and the session's parameters the fields set.
    field: Option<Vec<u8>>,
    inputs: HashMap<String, String>,
    /// DAT_10664550 / DAT_1066469b: the last committed text and its slot row, cleared at every open.
    remembered: Option<(i16, String)>,
}

impl FaqMain {
    fn node(&self) -> Option<&Node> {
        self.script.nodes.get(self.path.last()?)
    }

    /// FUN_1010f350(node, 2, 1, 0): the slots whose DISP conditions hold, in file order.
    fn visible(&self) -> Vec<&Slot> {
        let Some(n) = self.node() else { return Vec::new() };
        n.slots.iter().filter(|s| n.disp.iter().filter(|d| d.slot == s.id).all(|d| (value(&d.var) == d.value) == d.eq)).collect()
    }

    /// FUN_1010ea10's states: a select node with slots draws as the select page *(inf: 3 -> state 4)*, any other select
    /// node as a leaf page (state 5), a question node as the question page.
    fn page(&self) -> Page {
        match self.node() {
            Some(n) if n.question => Page::Question,
            _ if self.visible().is_empty() => Page::Leaf,
            _ => Page::Select,
        }
    }

    fn history(&self) -> bool {
        self.path.len() > 1
    }

    /// The row list (+0x25a): the slots, Execute on a question page, Go back with history (always on a leaf page, where
    /// it reads "Delete this message." without), Return.
    fn rows(&self) -> Vec<i16> {
        let n = self.visible().len() as i16;
        let mut r: Vec<i16> = match self.page() {
            Page::Select => (1..=n).collect(),
            Page::Question => (1..=n + 1).collect(),
            Page::Leaf => Vec::new(),
        };
        if self.history() || self.page() == Page::Leaf {
            r.push(0);
        }
        r.push(-1);
        r
    }

    /// The cursor a page opens on (FUN_10219f70): a select page its first slot, a question page its first field (0
    /// without one), a leaf page row 0.
    fn enter_page(&mut self) {
        self.cursor = match self.page() {
            Page::Select => 1,
            Page::Question if !self.visible().is_empty() => 1,
            _ => 0,
        };
        (self.scroll, self.follow, self.field) = (0.0, true, None);
    }
}

/// A GM reply being assembled: msgId, the last seqId, the text, complete.
type Reply = Option<(u32, u16, Vec<u8>, bool)>;

/// s2c 0x0B6 (LAB_101132b0): a new msgId starts over, its next seqId appends (0x400 bytes at most), pktNum 0 ends it.
fn assemble(r: &mut Reply, id: u32, seq: u16, last: bool, text: &[u8]) {
    match r.as_mut() {
        Some(r) if r.0 == id => {
            if seq == r.1.wrapping_add(1) && !r.3 {
                r.2.extend_from_slice(text);
                r.2.truncate(0x400);
                r.1 = seq;
                r.3 = last;
            }
        }
        _ => *r = Some((id, seq, text.to_vec(), last)),
    }
}

pub fn on_message(w: &mut World, id: u32, seq: u16, last: bool, text: &[u8]) {
    assemble(&mut w.gm_reply, id, seq, last, text);
    if last {
        log::info!("help desk: a GM reply is waiting ({} bytes)", w.gm_reply.as_ref().map_or(0, |r| r.2.len()));
    }
}

/// FUN_10115020's gmNotice.fdt: `DEF SELECTNODE 0`, `LABEL "NOTICE"`, `HEADER "<the text>"`: a leaf page.
fn reply_script(text: &[u8]) -> Script {
    let node = Node { header: crate::dat::font::decode_sjis(text), ..Default::default() };
    Script { version: 0, nodes: HashMap::from([(0, node)]), links: Vec::new() }
}

/// faqsub's Help Desk row (FUN_10217a40): c2s 0x0D4 {u16 counter (never 0), u16 0}, the script read from the root; the
/// GM's reply instead while one is waiting.
pub fn open(w: &mut World) -> Option<FaqMain> {
    if let Some((_, _, text, true)) = &w.gm_reply {
        let script = reply_script(text);
        w.faq_counter = w.faq_counter.wrapping_add(1).max(1);
        let mut b = vec![0u8; 4];
        b[..2].copy_from_slice(&w.faq_counter.to_le_bytes());
        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0D4, body: b });
        let mut f = FaqMain { script, path: vec![0], cursor: 0, scroll: 0.0, follow: true, field: None, inputs: HashMap::new(), remembered: None };
        f.enter_page();
        return Some(f);
    }
    let bytes = w.index.clone()?.read(FILE).map_err(|e| log::warn!("help desk: DAT {FILE}: {e:#}")).ok()?;
    w.faq_counter = w.faq_counter.wrapping_add(1).max(1);
    let mut b = vec![0u8; 4];
    b[..2].copy_from_slice(&w.faq_counter.to_le_bytes());
    let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0D4, body: b });
    let mut f = FaqMain { script: parse(&bytes), path: vec![0], cursor: 0, scroll: 0.0, follow: true, field: None, inputs: HashMap::new(), remembered: None };
    f.enter_page();
    Some(f)
}

fn main_mut(w: &mut World) -> Option<&mut FaqMain> {
    w.helpdesk.as_mut()?.main.as_mut()
}

fn main(w: &World) -> Option<&FaqMain> {
    w.helpdesk.as_ref()?.main.as_ref()
}

pub fn typing(w: &World) -> bool {
    main(w).is_some_and(|f| f.field.is_some())
}

/// The field (FUN_101c5be0 mode 8): printable text at the end; Enter commits it into the slot's parameter (and
/// DAT_10664550) through LAB_102176d0, Escape leaves the parameter as it was.
pub fn text_events(w: &mut World, events: &[egui::Event]) {
    use egui::Key as K;
    for ev in events {
        let Some(f) = main_mut(w) else { return };
        let Some(buf) = f.field.as_mut() else { return };
        match ev {
            egui::Event::Text(t) | egui::Event::Paste(t) => {
                for c in t.bytes().filter(|c| (b' '..=b'~').contains(c)) {
                    if buf.len() < FIELD_MAX {
                        buf.push(c);
                    }
                }
            }
            egui::Event::Key { key: K::Backspace, pressed: true, .. } => _ = buf.pop(),
            egui::Event::Key { key: K::Enter, pressed: true, .. } => {
                let text = String::from_utf8_lossy(&std::mem::take(buf)).into_owned();
                f.field = None;
                let row = f.cursor;
                if let Some(var) = f.visible().get(row as usize - 1).and_then(|s| s.var.clone()) {
                    f.inputs.insert(var, text.clone());
                }
                f.remembered = Some((row, text));
                w.sounds.push(Ui::Select as u32);
            }
            egui::Event::Key { key: K::Escape, pressed: true, .. } => {
                f.field = None;
                w.sounds.push(Ui::Cancel as u32);
            }
            _ => {}
        }
    }
}

/// FUN_1021bb70: faqmain closes, faqsub comes back.
fn close(w: &mut World) {
    if let Some(h) = w.helpdesk.as_mut() {
        h.main = None;
    }
}

/// FUN_1021a240 (no field active).
pub fn key(w: &mut World, key: egui::Key) {
    use egui::Key as K;
    let Some(f) = main_mut(w) else { return };
    if f.field.is_some() {
        return;
    }
    let rows = f.rows();
    let at = rows.iter().position(|&r| r == f.cursor);
    match key {
        // Up / Down: the row before / after, wrapping, SE 0001 every time.
        K::ArrowUp | K::ArrowDown => {
            let n = rows.len();
            let k = match (at, key) {
                (None, _) => 0,
                (Some(k), K::ArrowDown) => (k + 1) % n,
                (Some(k), _) => (k + n - 1) % n,
            };
            (f.cursor, f.follow) = (rows[k], true);
            w.sounds.push(Ui::Cursor as u32);
        }
        // Right / Left (messages 3 / 4): the page 150 px down / up (FUN_102180b0; clamped where it is drawn).
        K::ArrowRight | K::ArrowLeft => {
            let before = f.scroll;
            f.scroll = (f.scroll + if key == K::ArrowRight { 150.0 } else { -150.0 }).max(0.0);
            f.follow = false;
            if f.scroll != before {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        // Escape: the cursor to Return (SE 0001 when it moves); faqmain stays open.
        K::Escape => {
            let moved = f.cursor != -1;
            (f.cursor, f.follow) = (-1, true);
            if moved {
                w.sounds.push(Ui::Cursor as u32);
            }
        }
        K::Enter => enter(w),
        _ => {}
    }
}

fn enter(w: &mut World) {
    let Some(f) = main_mut(w) else { return };
    let (page, cursor, history) = (f.page(), f.cursor, f.history());
    match (page, cursor) {
        // Select page Return: SE 0003 and close; a slot follows its link (FUN_1010a420); row 0 goes back (FUN_1010a4a0).
        (Page::Select, c) if c < 0 => {
            w.sounds.push(Ui::Cancel as u32);
            close(w);
        }
        (_, 0) if history => {
            f.path.pop();
            f.enter_page();
            w.sounds.push(Ui::Select as u32);
        }
        (Page::Select, c) => {
            let from = *f.path.last().unwrap_or(&0);
            let Some(slot) = f.visible().get(c as usize - 1).map(|s| s.id) else { return };
            let to = f.script.links.iter().find(|l| l.0 == slot && l.1 == from).map(|l| l.2);
            if let Some(to) = to.filter(|t| f.script.nodes.contains_key(t)) {
                f.path.push(to);
                f.enter_page();
            }
            w.sounds.push(Ui::Select as u32);
        }
        // The GM reply page's "Delete this message." (c2s 0x0D5, then type 1 and close).
        (_, 0) => {
            w.sounds.push(Ui::Select as u32);
            let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0D5, body: vec![0; 8] });
            w.gm_reply = None;
            report(w, 1);
            close(w);
        }
        // A field row: the field opens on the text last committed for this row (else empty).
        (Page::Question, c) if c > 0 && (c as usize) <= f.visible().len() => {
            let start = f.remembered.as_ref().filter(|r| r.0 == c).map_or(String::new(), |r| r.1.clone());
            f.field = Some(start.into_bytes());
            w.sounds.push(Ui::Select as u32);
        }
        (Page::Question, c) if c > 0 => {
            w.sounds.push(Ui::Select as u32);
            execute(w);
        }
        // Return on a question or leaf page (FUN_102197d0): SE 0003, the report as type 1, close.
        _ => {
            w.sounds.push(Ui::Cancel as u32);
            report(w, 1);
            close(w);
        }
    }
}

/// FUN_102197f0: GENERIC.CLILANG (1, English), AREACODE (s2c 0x0B4 @0x12; LSB sends 0) and SELLANG (0) set; a node with
/// GMREPORT.GMCALL reports as type 2 and prints 7031 0x14 "Your call has been placed in the queue.", GMREPORT.NOTICE
/// as 3, else 1; faqmain closes; then EXEC.LOGOUT runs /logout, EXEC.RESCUE sends c2s 0x0F0 {0}. "Input:REQUIRE" is
/// not checked here.
fn execute(w: &mut World) {
    let Some(f) = main_mut(w) else { return };
    for (k, v) in [("GENERIC.CLILANG", "1"), ("GENERIC.AREACODE", "0"), ("GENERIC.SELLANG", "0")] {
        f.inputs.insert(k.into(), v.into());
    }
    let has = |f: &FaqMain, k: &str| f.path.iter().filter_map(|n| f.script.nodes.get(n)).any(|n| n.evals.iter().any(|e| e == k));
    let (call, notice, logout, rescue) = (has(f, "GMREPORT.GMCALL"), has(f, "GMREPORT.NOTICE"), has(f, "EXEC.LOGOUT"), has(f, "EXEC.RESCUE"));
    if call {
        report(w, 2);
        super::post::system_line(w, 0x14, [0; 5], String::new());
    } else {
        report(w, if notice { 3 } else { 1 });
    }
    close(w);
    if logout {
        super::textcmd::run(w, "/logout", super::textcmd::Mode::Internal);
    } else if rescue {
        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0F0, body: vec![0; 4] });
    }
}

/// FUN_10113410(type): blocks after the header {u16 type | pktId << 8, u8 seq | eos << 7, u8 block count}, packets cut
/// at 0x104 bytes: 0 position {zone 0, world 0, our position twice} (FUN_10113940), 1 version {dataset, the walk's node
/// ids} (FUN_101139c0), 3 one "GROUP.KEY:value" string per parameter, padded to 4 (FUN_10113a70). No error codes (2)
/// or lobby history (4): we keep neither *(inf: the parameters are the walk's EVAL keys plus the fields')*.
fn report(w: &mut World, kind: u8) {
    let Some(f) = main(w) else { return };
    let mut blocks: Vec<Vec<u8>> = Vec::new();
    let mut pos = vec![0u8; 0x20];
    pos[1] = 0x20;
    for (k, v) in [w.pos.x, w.pos.y, w.pos.z, w.pos.x, w.pos.y, w.pos.z].iter().enumerate() {
        pos[8 + 4 * k..12 + 4 * k].copy_from_slice(&v.to_le_bytes());
    }
    blocks.push(pos);
    let mut ver = vec![1u8, 0, 0, 0];
    ver.extend_from_slice(&f.script.version.to_le_bytes());
    for n in &f.path {
        ver.extend_from_slice(&(*n as u32).to_le_bytes());
    }
    ver[1] = ver.len() as u8;
    blocks.push(ver);
    let mut params: Vec<(String, String)> = Vec::new();
    for e in f.path.iter().filter_map(|n| f.script.nodes.get(n)).flat_map(|n| n.evals.iter()) {
        if !params.iter().any(|p| &p.0 == e) {
            params.push((e.clone(), value(e).to_string()));
        }
    }
    for (k, v) in &f.inputs {
        params.retain(|p| &p.0 != k);
        params.push((k.clone(), v.clone()));
    }
    for (k, v) in params {
        let mut b = vec![3u8, 0, 0, 0];
        let s = format!("{k}:{v}");
        b.extend_from_slice(&crate::dat::font::encode_sjis(&s)[..s.len().min(0xf4)]);
        b.push(0);
        while b.len() % 4 != 0 {
            b.push(0);
        }
        b[1] = b.len() as u8;
        blocks.push(b);
    }
    w.faq_report = w.faq_report.wrapping_add(1).max(1);
    let id = w.faq_report;
    let mut packets: Vec<Vec<Vec<u8>>> = vec![Vec::new()];
    let mut size = 8;
    for b in blocks {
        if size + b.len() > 0x104 && !packets.last().unwrap().is_empty() {
            packets.push(Vec::new());
            size = 8;
        }
        size += b.len();
        packets.last_mut().unwrap().push(b);
    }
    let last = packets.len() - 1;
    for (seq, p) in packets.into_iter().enumerate() {
        let mut body = vec![kind & 0xf, id, seq as u8 & 0x7f | if seq == last { 0x80 } else { 0 }, p.len() as u8];
        for b in p {
            body.extend_from_slice(&b);
        }
        log::info!("help desk: c2s 0x0D3 type {kind} packet {seq}, {} bytes", body.len() + 4);
        let _ = w.net.commands.send(super::net::NetCommand::Packet { id: 0x0D3, body });
    }
}

/// The page as the modern layer's lines: the header, the slots (a question's with its value line), Execute, Go back /
/// Delete this message., Return; the field being typed in.
pub fn view(w: &World, gu: &GameUi) -> Option<super::helpdesk::PageView> {
    use super::helpdesk::{Line, PageView};
    let f = main(w)?;
    let page = f.page();
    let g9 = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
    let mut lines = Vec::new();
    let header = f.node().map_or(String::new(), |n| n.header.clone());
    if !header.is_empty() {
        lines.push(Line::Text(header, false));
    }
    let slots: Vec<(String, Option<String>)> = f.visible().iter().map(|s| (s.text.clone(), s.var.clone())).collect();
    if page != Page::Leaf {
        for (k, (text, var)) in slots.iter().enumerate() {
            let value = var.as_ref().filter(|_| page == Page::Question).map(|v| f.inputs.get(v).cloned().unwrap_or_default());
            lines.push(Line::Row { id: k as i32 + 1, text: text.clone(), on: true, value });
        }
    }
    if page == Page::Question {
        lines.push(Line::Row { id: slots.len() as i32 + 1, text: g9(0x7d), on: true, value: None });
    }
    if f.history() || page == Page::Leaf {
        lines.push(Line::Row { id: 0, text: if f.history() { g9(0x79) } else { g9(0x7b) }, on: true, value: None });
    }
    lines.push(Line::Row { id: -1, text: g9(0x7a), on: true, value: None });
    let field = f.field.as_ref().map(|b| String::from_utf8_lossy(b).into_owned());
    let title = gu.title_msg(0x3c).unwrap_or_else(|| "Help Desk".into());
    Some(PageView { title, lines, cursor: f.cursor as i32, focused: w.confirm.is_none() && w.search.is_none(), field })
}

/// The modern layer's mouse: the cursor to row `id` (not while typing); true when it moved.
pub fn set_cursor(w: &mut World, id: i32) -> bool {
    let Some(f) = main_mut(w).filter(|f| f.field.is_none()) else { return false };
    let moved = f.cursor as i32 != id;
    (f.cursor, f.follow) = (id as i16, true);
    moved
}

/// One drawn entry: its row (None for the header), place, lines, the question's value line below it.
struct Entry {
    row: Option<i16>,
    x: f32,
    y: f32,
    lines: Vec<String>,
    value: Option<(f32, String)>,
    /// Slots carry the bullet when the cursor is elsewhere; Execute, Go back and Return only the hand.
    bullet: bool,
    h: f32,
}

/// FUN_102185d0 / FUN_10218ac0 / FUN_102194a0: pads (left, right, top) 8 / 20 / 10 on a select page, 8 / 20 / 15 on a
/// question page, 10 / 25 / 15 on a leaf page; the header, then the rows at the text x + 20, slots 10 apart, Execute,
/// Go back and Return 15 apart; a question slot's value 3 below its prompt at + 20, the slot 20 taller. Positions from
/// the window's top left, scroll not applied.
fn layout(gu: &GameUi, w: &World, f: &FaqMain, width: f32) -> (Vec<Entry>, f32) {
    let page = f.page();
    let (left, right, top) = match page {
        Page::Select => (8.0, 20.0, 10.0),
        Page::Question => (8.0, 20.0, 15.0),
        Page::Leaf => (10.0, 25.0, 15.0),
    };
    let inner = width - left - right;
    let row_w = inner - RIGHT - INDENT;
    let g9 = |id: usize| w.battle.as_ref().and_then(|b| b.group9(id)).unwrap_or_default();
    let mut out = Vec::new();
    let header = f.node().map_or(String::new(), |n| n.header.clone());
    let lines = if header.is_empty() { Vec::new() } else { gu.wrap(&header, inner) };
    let mut y = top + 16.0 * lines.len() as f32;
    out.push(Entry { row: None, x: left, y: top, lines, value: None, bullet: false, h: 0.0 });
    let rx = left + INDENT;
    let push = |out: &mut Vec<Entry>, y: &mut f32, gap: f32, row: i16, text: String, value: Option<String>, bullet: bool| {
        *y += gap;
        let lines = gu.wrap(&text, row_w);
        let mut h = 16.0 * lines.len() as f32;
        let value = value.map(|v| {
            let vy = *y + h + GAP - 7.0;
            h += 20.0;
            (vy, v)
        });
        out.push(Entry { row: Some(row), x: rx, y: *y, lines, value, bullet, h });
        *y += h;
    };
    let slots: Vec<(String, Option<String>)> = f.visible().iter().map(|s| (s.text.clone(), s.var.clone())).collect();
    if page != Page::Leaf {
        for (k, (text, var)) in slots.iter().enumerate() {
            let value = var.as_ref().filter(|_| page == Page::Question).map(|v| f.inputs.get(v).cloned().unwrap_or_default());
            push(&mut out, &mut y, GAP, k as i16 + 1, text.clone(), value, true);
        }
    }
    if page == Page::Question {
        push(&mut out, &mut y, GAP2, slots.len() as i16 + 1, g9(0x7d), None, false);
    }
    // Leaf pages start their row 0 at the header + 15 as well.
    if f.history() || page == Page::Leaf {
        let text = if f.history() { g9(0x79) } else { g9(0x7b) };
        push(&mut out, &mut y, GAP2, 0, text, None, false);
    } else {
        y += GAP2;
    }
    push(&mut out, &mut y, GAP2, -1, g9(0x7a), None, false);
    (out, y + BOTTOM)
}

/// The value line (FUN_1021bb20 with '[' ']'): cut to the box width - 30 and 0x30 bytes, "..." when cut.
fn value_text(gu: &GameUi, v: &str, width: f32) -> String {
    let mut cut = String::new();
    for c in v.chars() {
        if cut.len() >= 0x30 || gu.text_width(&format!("{cut}{c}")) > width - 30.0 {
            return format!("[{cut}...]");
        }
        cut.push(c);
    }
    format!("[{cut}]")
}

/// FUN_10217ec0 on a cursor move, then the scroll kept inside the page (FUN_102180b0's clamp).
pub fn keep_scroll(w: &mut World, gu: &GameUi, sw: f32, sh: f32) {
    let Some(f) = main(w) else { return };
    let Some([_, _, fw, fh]) = gu.window_rect(WIN, sw, sh) else { return };
    let view = fh - 5.0;
    let (entries, total) = layout(gu, w, f, fw);
    let top = match f.page() {
        Page::Select => 10.0,
        _ => 15.0,
    };
    let mut scroll = f.scroll;
    if f.follow {
        if f.cursor == 1 {
            scroll = 0.0;
        } else if let Some(e) = entries.iter().find(|e| e.row == Some(f.cursor)) {
            if e.y + e.h + BOTTOM - scroll > view {
                scroll = e.y + e.h + BOTTOM - view;
            } else if e.y - scroll < top {
                scroll = e.y - top;
            }
        }
    }
    scroll = scroll.clamp(0.0, (total - view).max(0.0));
    if let Some(f) = main_mut(w) {
        (f.scroll, f.follow) = (scroll, false);
    }
}

/// The window (faqsub's geometry, children not drawn), the entries clipped to the window rect less 5 px of height, the
/// markers (FUN_10218180): the hand at the row's x on the cursor row, framesus 35 at (text x, y + 4) on other slots; the
/// bar at x1 - 8.
pub fn draw(gu: &mut GameUi, w: &World, sw: f32, sh: f32) -> Option<[f32; 4]> {
    let f = main(w)?;
    let win = gu.lobby_window_off(WIN, sw, sh, (0.0, 0.0), None, false, &(1..=15).collect::<Vec<u16>>())?;
    let [fx, fy, fw, fh] = win.frame;
    let view = fh - 5.0;
    let (entries, total) = layout(gu, w, f, fw);
    let focused = w.confirm.is_none() && w.search.is_none();
    gu.clip(Some([fx, fy, fw, view]));
    let shown = |y: f32, h: f32| y >= fy && y + h <= fy + view;
    for e in &entries {
        let (x, y) = (fx + e.x, fy + e.y - f.scroll);
        for (k, l) in e.lines.iter().enumerate() {
            gu.text(l, x, y + 16.0 * k as f32, 0, WHITE.to_le_bytes());
        }
        let Some(row) = e.row else { continue };
        let on = row == f.cursor && focused;
        let editing = on && f.field.is_some();
        if let Some((vy, v)) = &e.value {
            let (vx, vy, vw) = (x + INDENT, fy + vy - f.scroll, fw - 16.0 - 2.0 * INDENT - RIGHT - 12.0);
            if editing {
                let t = String::from_utf8_lossy(f.field.as_deref().unwrap_or_default()).into_owned();
                gu.text_fit(&format!("{t}{}", super::macroedit::caret(w)), vx, vy, vw, WHITE);
                gu.cursor(vx, vy);
            } else {
                gu.text(&value_text(gu, v, vw), vx, vy, 0, WHITE.to_le_bytes());
            }
        }
        if on && !editing {
            gu.cursor(x, y);
        } else if e.bullet && shown(y, 16.0) {
            gu.sprite("menu    framesus", 35, x - INDENT, y + 4.0, WHITE);
        }
    }
    gu.clip(None);
    gu.scroll_bar(fx + fw - 8.0, fy, fh, total as usize, view as usize, f.scroll as usize);
    if focused && f.field.is_none() {
        gu.bar_site(super::scrollbar::Owner::Faq, fx + fw - 8.0, fy, fh, total as usize, view as usize, f.scroll as usize);
    }
    Some(win.frame)
}

/// FUN_1021a240 messages 0x16 / 0x17 (the caps, the wheel) 15 px, 0x18 / 0x19 (the track) 150 px, 0x1b (the thumb)
/// the offset; `bar` is (total, view, top) in pixels as drawn.
pub fn bar(w: &mut World, m: super::scrollbar::Msg, bar: (usize, usize, usize)) -> bool {
    use super::scrollbar::Msg;
    let Some(f) = main_mut(w) else { return false };
    let max = bar.0.saturating_sub(bar.1) as f32;
    let before = f.scroll;
    f.scroll = match m {
        Msg::Up => f.scroll - 15.0,
        Msg::Down => f.scroll + 15.0,
        Msg::PageUp => f.scroll - 150.0,
        Msg::PageDown => f.scroll + 150.0,
        Msg::Set(t) => t as f32,
    }
    .clamp(0.0, max.max(0.0));
    f.follow = false;
    f.scroll != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script() {
        let src: &[u8] = b"DEF DATASET 2003022101\r\n  DEF SELECTNODE 0\r\n    LABEL \"ROOT\"\r\n    HEADER \"\" \"Pick \\\"one\\\".\\n\" \"\" \"\"\r\n    EVAL \"GAME.INFIELD\"\r\n    SLOT 1 \"\x83\\\" \"Stuck.\" \"x\" \"y\"\r\n    SLOT 2 \"j\" \"PS2 only\" \"\" \"\"\r\n    DISP 2 \"GENERIC.PLATFORM\" \"S:eq\" \"ps2\"\r\n  END NODE\r\n  DEF QUESTIONNODE 3\r\n    SLOT 1 \"Input:REQUIRE\" \"GMCALL.INPUT\" \"j\" \"Describe it.\" \"d\" \"f\"\r\n  END NODE\r\n  DEF LINK 1\r\n    UP 1 0 3\r\n  END LINK\r\nEND DATASET\r\n";
        let s = parse(src);
        assert_eq!(s.version, 2003022101);
        let root = &s.nodes[&0];
        assert_eq!(root.header, "Pick \"one\".\n");
        assert_eq!((root.slots[0].text.as_str(), root.slots.len(), root.disp[0].eq), ("Stuck.", 2, true));
        let q = &s.nodes[&3];
        assert!(q.question);
        assert_eq!((q.slots[0].text.as_str(), q.slots[0].var.as_deref()), ("Describe it.", Some("GMCALL.INPUT")));
        assert_eq!(s.links, vec![(1, 0, 3)]);
        let mut f = FaqMain { script: s, path: vec![0], cursor: 0, scroll: 0.0, follow: false, field: None, inputs: HashMap::new(), remembered: None };
        assert_eq!(f.visible().len(), 1);
        assert_eq!((f.page(), f.rows()), (Page::Select, vec![1, -1]));
        f.path.push(3);
        assert_eq!((f.page(), f.rows()), (Page::Question, vec![1, 2, 0, -1]));
    }
}


#[cfg(test)]
mod reply_tests {
    use super::*;

    /// Chunks of one msgId append in order; a repeat or a gap is dropped; pktNum 0 completes; a new msgId restarts.
    #[test]
    fn assembly() {
        let mut r: Reply = None;
        assemble(&mut r, 7, 1, false, b"Hello ");
        assemble(&mut r, 7, 1, false, b"again ");
        assemble(&mut r, 7, 3, false, b"gap ");
        assemble(&mut r, 7, 2, true, b"there.");
        assert_eq!(r.as_ref().map(|r| (r.2.as_slice(), r.3)), Some((&b"Hello there."[..], true)));
        assemble(&mut r, 8, 1, false, b"New");
        assert_eq!(r.as_ref().map(|r| (r.0, r.2.as_slice(), r.3)), Some((8, &b"New"[..], false)));
        let s = reply_script(b"Hi");
        assert_eq!(s.nodes.get(&0).map(|n| (n.header.as_str(), n.slots.len())), Some(("Hi", 0)));
    }
}
